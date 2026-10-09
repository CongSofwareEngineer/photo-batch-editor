//! Xử lý hàng loạt (CPU). Port phần CPU của `core/batch.py`.
//!
//! GĐ3: chế độ CPU đa luồng (process pool của Python → thread pool). Chế độ GPU (CuPy pipeline
//! 3 tầng) không port vì máy dev không có GPU; `device="gpu"` sẽ chạy bằng CPU như khi fallback.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::io_utils::{read_image, write_jpeg, ImageReadError, Pixels};
use crate::pipeline::{self, Input};
use crate::scanner::{default_output_dir, plan_jobs, scan_folder, Job};
use crate::settings::AdjustmentSettings;

pub const CORRUPT_FILE: &str = "Corrupt file";
pub const LOG_FILENAME: &str = "process_log.txt";
pub const GPU_FALLBACK_WARNING: &str = "Photo too large for VRAM, processed on CPU";
pub const GPU_ERROR_WARNING: &str = "GPU error, processed on CPU";

#[derive(Debug, Clone)]
pub struct FileResult {
    pub input_path: PathBuf,
    pub output_path: Option<PathBuf>,
    pub rel_dir: String,
    pub status: String, // "ok" | "warning" | "error" | "cancelled"
    pub message: String,
    pub width: Option<usize>,
    pub height: Option<usize>,
    pub size_bytes: Option<u64>,
    pub seconds: f64,
    pub device: String,
}

#[derive(Debug)]
pub struct BatchReport {
    pub input_dir: PathBuf,
    pub output_dir: PathBuf,
    pub cancelled: bool,
    pub device_requested: String,
    pub device_used: String, // "cpu" | "gpu" | "mixed"
    pub device_name: String,
    pub total_seconds: f64,
    pub files: Vec<FileResult>,
}

impl BatchReport {
    pub fn count(&self, statuses: &[&str]) -> usize {
        self.files.iter().filter(|f| statuses.contains(&f.status.as_str())).count()
    }
    pub fn succeeded(&self) -> usize {
        self.count(&["ok", "warning"])
    }
}

fn error_result(job: &Job, message: String, seconds: f64, device: &str) -> FileResult {
    FileResult {
        input_path: job.input_path.clone(),
        output_path: None,
        rel_dir: job.rel_dir.clone(),
        status: "error".into(),
        message,
        width: None,
        height: None,
        size_bytes: None,
        seconds,
        device: device.into(),
    }
}

/// Đọc, xử lý và ghi một ảnh. Không bao giờ panic.
pub fn process_job(job: &Job, settings: &AdjustmentSettings, device: &str) -> FileResult {
    let t0 = Instant::now();
    if std::path::absolute(&job.input_path).ok() == std::path::absolute(&job.output_path).ok() {
        return error_result(job, "Output would overwrite the original".into(), 0.0, device);
    }
    let loaded = match read_image(&job.input_path) {
        Ok(l) => l,
        Err(ImageReadError(msg)) => {
            let text = if msg.is_empty() { CORRUPT_FILE.to_string() } else { msg };
            return error_result(job, text, t0.elapsed().as_secs_f64(), device);
        }
    };
    let (out, warnings) = {
        let input = match &loaded.pixels {
            Pixels::U8(i) => Input::U8(i),
            Pixels::U16(i) => Input::U16(i),
            Pixels::F32(i) => Input::F32(i),
        };
        pipeline::process(input, settings, None)
    };
    let (ow, oh) = (out.w, out.h);
    let write_warnings =
        match write_jpeg(&job.output_path, &out, loaded.exif.as_deref(), loaded.icc.as_deref()) {
            Ok(w) => w,
            Err(e) => {
                return error_result(job, format!("Write failed: {e}"), t0.elapsed().as_secs_f64(), device)
            }
        };
    // gộp cảnh báo, bỏ trùng giữ thứ tự
    let mut notes: Vec<String> = vec![];
    for w in warnings.into_iter().chain(write_warnings) {
        if !notes.contains(&w) {
            notes.push(w);
        }
    }
    let status = if notes.is_empty() { "ok" } else { "warning" };
    if job.renamed {
        let name = job.output_path.file_name().unwrap().to_string_lossy();
        notes.push(format!("Saved as {name} (name collision)"));
    }
    let size_bytes = std::fs::metadata(&job.output_path).ok().map(|m| m.len());
    FileResult {
        input_path: job.input_path.clone(),
        output_path: Some(job.output_path.clone()),
        rel_dir: job.rel_dir.clone(),
        status: status.into(),
        message: notes.join("; "),
        width: Some(ow),
        height: Some(oh),
        size_bytes,
        seconds: t0.elapsed().as_secs_f64(),
        device: device.into(),
    }
}

pub fn default_cpu_workers(settings: &AdjustmentSettings) -> usize {
    if settings.sr_factor() > 1 {
        return 1; // (tương lai) SR dùng hết nhân sẵn
    }
    let cpus = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2);
    cpus.saturating_sub(1).max(1).min(4)
}

type ProgressFn<'a> = dyn Fn(usize, usize, &str, Option<&FileResult>) + Sync + 'a;

#[derive(Default)]
pub struct BatchOptions<'a> {
    pub device: Option<String>,
    pub output_dir: Option<PathBuf>,
    pub workers: Option<usize>,
    pub cancel: Option<Arc<AtomicBool>>,
    pub on_progress: Option<&'a ProgressFn<'a>>,
    pub files: Option<Vec<PathBuf>>,
}

/// Xử lý mọi ảnh của `input_dir` vào `output_dir` (mặc định `<name>_update`).
pub fn run_batch(
    input_dir: &Path,
    settings: &AdjustmentSettings,
    opts: BatchOptions,
) -> Result<BatchReport, String> {
    let start = Instant::now();
    // Giống Python `.resolve()`: phân giải symlink khi có thể (vd. /var -> /private/var trên macOS).
    let input_dir = std::fs::canonicalize(input_dir)
        .or_else(|_| std::path::absolute(input_dir))
        .map_err(|e| e.to_string())?;
    let output_dir = match opts.output_dir {
        Some(d) => d,
        None => default_output_dir(&input_dir).map_err(|e| e.to_string())?,
    };
    let files = opts.files.unwrap_or_else(|| scan_folder(&input_dir));
    let jobs = plan_jobs(&input_dir, &files, &output_dir);
    std::fs::create_dir_all(&output_dir).map_err(|e| e.to_string())?;
    let cancel = opts.cancel.unwrap_or_else(|| Arc::new(AtomicBool::new(false)));
    let device = opts.device.unwrap_or_else(|| "cpu".into());
    let device_name = format!("CPU ({} threads)", std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1));

    let total = jobs.len();
    let results: Mutex<Vec<Option<FileResult>>> = Mutex::new(vec![None; total]);
    let done_count = AtomicUsize::new(0);
    let cursor = AtomicUsize::new(0);
    let progress = opts.on_progress;

    let emit = |current: &str, res: Option<&FileResult>| {
        if res.is_some() {
            done_count.fetch_add(1, Ordering::SeqCst);
        }
        if let Some(cb) = progress {
            cb(done_count.load(Ordering::SeqCst), total, current, res);
        }
    };

    if total > 0 {
        let workers = opts
            .workers
            .unwrap_or_else(|| default_cpu_workers(settings))
            .clamp(1, total);
        let emit = &emit;
        let results = &results;
        let cursor = &cursor;
        let cancel = &cancel;
        let jobs = &jobs;
        let dev: &str = &device;
        std::thread::scope(|scope| {
            for _ in 0..workers {
                scope.spawn(move || loop {
                    if cancel.load(Ordering::SeqCst) {
                        break;
                    }
                    let idx = cursor.fetch_add(1, Ordering::SeqCst);
                    if idx >= jobs.len() {
                        break;
                    }
                    let job = &jobs[idx];
                    // phát "đang xử lý" (res = None) dưới khoá để nối tiếp tuần tự
                    {
                        let _g = results.lock().unwrap();
                        emit(&job.input_path.file_name().unwrap().to_string_lossy(), None);
                    }
                    let res = process_job(job, settings, dev);
                    let mut guard = results.lock().unwrap();
                    guard[idx] = Some(res.clone());
                    emit(&job.input_path.file_name().unwrap().to_string_lossy(), Some(&res));
                });
            }
        });
    }

    // Kết quả cuối: job chưa chạy -> "cancelled".
    let results = results.into_inner().unwrap();
    let mut files_out: Vec<FileResult> = vec![];
    for (job, res) in jobs.iter().zip(results.into_iter()) {
        files_out.push(res.unwrap_or_else(|| FileResult {
            input_path: job.input_path.clone(),
            output_path: None,
            rel_dir: job.rel_dir.clone(),
            status: "cancelled".into(),
            message: "Cancelled".into(),
            width: None,
            height: None,
            size_bytes: None,
            seconds: 0.0,
            device: device.clone(),
        }));
    }
    // dọn file .tmp còn sót
    for job in &jobs {
        let tmp = job.output_path.with_file_name(format!(
            "{}.tmp",
            job.output_path.file_name().and_then(|n| n.to_str()).unwrap_or("out.jpg")
        ));
        if tmp.exists() {
            let _ = std::fs::remove_file(&tmp);
        }
    }

    let used: std::collections::HashSet<&str> = files_out
        .iter()
        .filter(|f| f.status == "ok" || f.status == "warning")
        .map(|f| f.device.as_str())
        .collect();
    let device_used = if used.len() == 1 {
        used.into_iter().next().unwrap().to_string()
    } else if used.is_empty() {
        device.clone()
    } else {
        "mixed".to_string()
    };

    let report = BatchReport {
        input_dir: input_dir.clone(),
        output_dir: output_dir.clone(),
        cancelled: cancel.load(Ordering::SeqCst),
        device_requested: device,
        device_used,
        device_name,
        total_seconds: start.elapsed().as_secs_f64(),
        files: files_out,
    };
    write_process_log(&report, settings);
    Ok(report)
}

pub fn write_process_log(report: &BatchReport, settings: &AdjustmentSettings) {
    let mut lines = vec![
        "Photo Batch Editor — process log".to_string(),
        format!("Input folder: {}", report.input_dir.display()),
        format!("Output folder: {}", report.output_dir.display()),
        format!("Device requested: {}", report.device_requested),
        format!("Device used: {} — {}", report.device_used, report.device_name),
        format!("Run time: {:.1} s", report.total_seconds),
        format!("Cancelled: {}", if report.cancelled { "yes" } else { "no" }),
        format!(
            "Result: {}/{} succeeded, {} with warnings, {} failed, {} cancelled",
            report.succeeded(),
            report.files.len(),
            report.count(&["warning"]),
            report.count(&["error"]),
            report.count(&["cancelled"]),
        ),
        String::new(),
        "Settings:".to_string(),
        serde_json::to_string_pretty(&settings.to_dict()).unwrap_or_default(),
        String::new(),
        "Files:".to_string(),
    ];
    for r in &report.files {
        let rel = r
            .input_path
            .strip_prefix(&report.input_dir)
            .unwrap_or(&r.input_path)
            .display();
        let out = r.output_path.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "-".into());
        let size = match (r.width, r.height) {
            (Some(w), Some(h)) => format!("{w}x{h}"),
            _ => "-".into(),
        };
        lines.push(format!(
            "[{:9}] {} -> {} | {} | {} | {:.2} s | {}",
            r.status.to_uppercase(),
            rel,
            out,
            size,
            r.device,
            r.seconds,
            r.message
        ));
    }
    let _ = std::fs::write(report.output_dir.join(LOG_FILENAME), lines.join("\n") + "\n");
}
