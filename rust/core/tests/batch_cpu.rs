//! Batch CPU trên thư mục ảnh thật (fixture giống conftest.photo_folder). Xem rust-port.md.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use pbe_core::batch::{run_batch, BatchOptions, FileResult, LOG_FILENAME};
use pbe_core::settings::{AdjustmentSettings, ImageSizeSettings};
use serde_json::Value;

fn src_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("batch_src")
}

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let dst = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &dst);
        } else {
            std::fs::copy(entry.path(), dst).unwrap();
        }
    }
}

fn make_client_folder() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let client = dir.path().join("clientA");
    copy_tree(&src_dir().join("clientA"), &client);
    // canonicalize để khớp với input_dir đã phân giải symlink trong run_batch.
    let client = client.canonicalize().unwrap();
    (dir, client)
}

fn digest(folder: &Path) -> HashMap<String, Vec<u8>> {
    let mut out = HashMap::new();
    fn walk(root: &Path, dir: &Path, out: &mut HashMap<String, Vec<u8>>) {
        for e in std::fs::read_dir(dir).unwrap() {
            let e = e.unwrap();
            let p = e.path();
            if p.is_dir() {
                walk(root, &p, out);
            } else {
                let rel = p.strip_prefix(root).unwrap().to_string_lossy().to_string();
                out.insert(rel, std::fs::read(&p).unwrap());
            }
        }
    }
    walk(folder, folder, &mut out);
    out
}

fn expected_dims() -> Value {
    let text = std::fs::read_to_string(src_dir().join("expected.json")).unwrap();
    serde_json::from_str(&text).unwrap()
}

fn rglob_jpg(dir: &Path) -> Vec<String> {
    let mut out = vec![];
    fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
        for e in std::fs::read_dir(dir).unwrap() {
            let e = e.unwrap();
            let p = e.path();
            if p.is_dir() {
                walk(root, &p, out);
            } else if p.extension().and_then(|x| x.to_str()) == Some("jpg") {
                out.push(p.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/"));
            }
        }
    }
    walk(dir, dir, &mut out);
    out.sort();
    out
}

fn has_tmp(dir: &Path) -> bool {
    fn walk(dir: &Path) -> bool {
        for e in std::fs::read_dir(dir).unwrap() {
            let e = e.unwrap();
            let p = e.path();
            if p.is_dir() {
                if walk(&p) {
                    return true;
                }
            } else if p.extension().and_then(|x| x.to_str()) == Some("tmp") {
                return true;
            }
        }
        false
    }
    walk(dir)
}

#[test]
fn batch_of_10() {
    let (_tmp, folder) = make_client_folder();
    let before = digest(&folder);
    let s = AdjustmentSettings {
        exposure: 0.3,
        shadows: 20.0,
        vibrance: 15.0,
        clarity: 10.0,
        ..Default::default()
    };
    let report = run_batch(&folder, &s, BatchOptions::default()).unwrap();

    assert_eq!(digest(&folder), before, "ảnh gốc không được đổi");
    let out = folder.parent().unwrap().join("clientA_update");
    assert_eq!(report.output_dir, out);
    let jpgs = rglob_jpg(&out);
    assert_eq!(jpgs.len(), 10, "{jpgs:?}");
    assert!(jpgs.contains(&"outdoor/ảnh cưới 01.jpg".to_string()));
    assert!(jpgs.contains(&"outdoor/gray.jpg".to_string()));
    assert_eq!(report.files.len(), 10);
    assert!(!report.cancelled);
    let bad: Vec<&str> = report.files.iter().filter(|f| f.status != "ok").map(|f| f.message.as_str()).collect();
    assert!(bad.is_empty(), "{bad:?}");
    assert_eq!(report.device_used, "cpu");

    let exp = expected_dims();
    for f in &report.files {
        let rel = f.input_path.strip_prefix(&folder).unwrap().to_string_lossy().replace('\\', "/");
        let (ew, eh) = (
            exp[&rel]["width"].as_u64().unwrap() as usize,
            exp[&rel]["height"].as_u64().unwrap() as usize,
        );
        assert_eq!((f.width, f.height), (Some(ew), Some(eh)), "{rel} dims");
        let op = f.output_path.as_ref().unwrap();
        assert_eq!(f.size_bytes, Some(std::fs::metadata(op).unwrap().len()));
    }
    // thứ tự theo đường dẫn tương đối (casefold)
    let order: Vec<String> = report
        .files
        .iter()
        .map(|f| f.input_path.strip_prefix(&folder).unwrap().to_string_lossy().to_lowercase())
        .collect();
    let mut sorted = order.clone();
    sorted.sort();
    assert_eq!(order, sorted);
    assert!(!has_tmp(&out));
}

#[test]
fn corrupt_file_is_error() {
    let (_tmp, folder) = make_client_folder();
    std::fs::write(folder.join("IMG_003.png"), b"this is not a png").unwrap();
    let report = run_batch(
        &folder,
        &AdjustmentSettings { contrast: 10.0, ..Default::default() },
        BatchOptions::default(),
    )
    .unwrap();
    assert_eq!(report.succeeded(), 9);
    assert_eq!(report.count(&["error"]), 1);
    let bad = report.files.iter().find(|f| f.status == "error").unwrap();
    assert_eq!(bad.input_path.file_name().unwrap(), "IMG_003.png");
    assert!(bad.message.to_lowercase().contains("orrupt"), "{}", bad.message);
    assert!(bad.output_path.is_none());
    assert!(bad.width.is_none());
}

#[test]
fn process_log_written() {
    let (_tmp, folder) = make_client_folder();
    let report = run_batch(&folder, &AdjustmentSettings::default(), BatchOptions::default()).unwrap();
    let text = std::fs::read_to_string(report.output_dir.join(LOG_FILENAME)).unwrap();
    assert!(text.contains(&format!("Device used: {}", report.device_used)));
    assert!(text.contains("IMG_000.png"));
    assert!(text.contains("\"exposure\": 0"));
}

#[test]
fn sizes_with_image_size() {
    let (_tmp, folder) = make_client_folder();
    let s = AdjustmentSettings {
        image_size: ImageSizeSettings::new("long_edge", 50.0, "bicubic_sharper", false),
        ..Default::default()
    };
    let report = run_batch(&folder, &s, BatchOptions::default()).unwrap();
    for f in &report.files {
        assert!(f.width.is_some() && f.height.is_some() && f.output_path.is_some());
        assert_eq!(f.width.unwrap().max(f.height.unwrap()), 50, "{:?}", f.input_path);
    }
}

#[test]
fn cancel_stops_batch() {
    let (_tmp, folder) = make_client_folder();
    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_cb = cancel.clone();
    let seen = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let seen_cb = seen.clone();
    let cb = move |_d: usize, _t: usize, _c: &str, res: Option<&FileResult>| {
        if let Some(r) = res {
            seen_cb.lock().unwrap().push(r.status.clone());
            cancel_cb.store(true, Ordering::SeqCst);
        }
    };
    let report = run_batch(
        &folder,
        &AdjustmentSettings { noise_reduction: 50.0, ..Default::default() },
        BatchOptions {
            cancel: Some(cancel.clone()),
            on_progress: Some(&cb),
            workers: Some(1),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(report.cancelled);
    let statuses: Vec<&str> = report.files.iter().map(|f| f.status.as_str()).collect();
    assert!(statuses.contains(&"cancelled"), "{statuses:?}");
    assert!(statuses.iter().filter(|s| **s == "ok").count() >= 1);
    assert_eq!(report.files.len(), 10);
    for f in &report.files {
        if f.status == "cancelled" {
            assert!(f.output_path.is_none());
        }
    }
    assert!(!has_tmp(&report.output_dir));
}
