//! Quét thư mục, thư mục xuất và đặt tên file xuất. Port của `core/scanner.py`.
//!
//! Lưu ý: thuộc tính hidden/system của Windows chưa xử lý (sẽ thêm ở giai đoạn sau);
//! hiện chỉ coi file/thư mục bắt đầu bằng dấu chấm là ẩn, như trên macOS/Linux.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

pub const SUPPORTED_EXTENSIONS: [&str; 7] =
    ["jpg", "jpeg", "png", "tif", "tiff", "webp", "bmp"];
pub const SKIPPED_NAMES: [&str; 2] = ["thumbs.db", "desktop.ini"];
pub const OUTPUT_SUFFIX: &str = "_update";

#[derive(Debug)]
pub struct OutputFolderError(pub String);

impl std::fmt::Display for OutputFolderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl std::error::Error for OutputFolderError {}

/// Một file input và nơi file JPEG của nó sẽ được ghi.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    pub input_path: PathBuf,
    pub output_path: PathBuf,
    pub rel_dir: String, // thư mục con so với input, "" ở gốc
    pub renamed: bool,   // tên output đổi vì trùng tên
}

fn ext_lower(name: &str) -> String {
    Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default()
}

pub fn is_supported(name: &str) -> bool {
    SUPPORTED_EXTENSIONS.contains(&ext_lower(name).as_str())
}

fn is_hidden(name: &str) -> bool {
    name.starts_with('.')
}

fn sort_key(rel: &Path) -> Vec<String> {
    rel.components()
        .map(|c| c.as_os_str().to_string_lossy().to_lowercase())
        .collect()
}

/// Mọi ảnh được hỗ trợ dưới `folder` (đệ quy), sắp xếp theo đường dẫn tương đối.
pub fn scan_folder(folder: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = vec![];
    let mut stack: Vec<PathBuf> = vec![folder.to_path_buf()];
    while let Some(current) = stack.pop() {
        let entries = match std::fs::read_dir(&current) {
            Ok(it) => it,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let name_lower = name.to_lowercase();
            if is_hidden(&name) || SKIPPED_NAMES.contains(&name_lower.as_str()) {
                continue;
            }
            let file_type = match entry.file_type() {
                Ok(t) => t,
                Err(_) => continue,
            };
            if file_type.is_dir() {
                stack.push(entry.path());
            } else if file_type.is_file() && is_supported(&name) {
                found.push(entry.path());
            }
        }
    }
    found.sort_by(|a, b| {
        let ka = sort_key(a.strip_prefix(folder).unwrap_or(a));
        let kb = sort_key(b.strip_prefix(folder).unwrap_or(b));
        ka.cmp(&kb)
    });
    found
}

fn resolve(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| {
        std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
    })
}

pub fn is_drive_root(folder: &Path) -> bool {
    let resolved = resolve(folder);
    match resolved.parent() {
        None => true,
        Some(p) => p == resolved,
    }
}

/// Thư mục anh em `<name>_update`; gốc ổ đĩa bị từ chối.
pub fn default_output_dir(input_dir: &Path) -> Result<PathBuf, OutputFolderError> {
    let resolved = resolve(input_dir);
    if is_drive_root(&resolved) {
        return Err(OutputFolderError(
            "Cannot process a drive root. Please choose a folder.".to_string(),
        ));
    }
    let name = resolved
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    let parent = resolved.parent().unwrap_or(&resolved);
    Ok(parent.join(format!("{name}{OUTPUT_SUFFIX}")))
}

/// `<name>_update` nếu trống, nếu không thì `<name>_update_2`, `_3`…
pub fn next_free_output_dir(input_dir: &Path) -> Result<PathBuf, OutputFolderError> {
    let base = default_output_dir(input_dir)?;
    if !base.exists() {
        return Ok(base);
    }
    let base_name = base.file_name().and_then(|n| n.to_str()).unwrap_or("out").to_string();
    let mut n = 2;
    loop {
        let candidate = base.with_file_name(format!("{base_name}_{n}"));
        if !candidate.exists() {
            return Ok(candidate);
        }
        n += 1;
    }
}

/// Tính mọi đường dẫn xuất từ trước để hai worker không ghi trùng tên.
pub fn plan_jobs(input_dir: &Path, files: &[PathBuf], output_dir: &Path) -> Vec<Job> {
    // Nhóm theo thư mục con tương đối (posix).
    let mut by_dir: HashMap<String, Vec<PathBuf>> = HashMap::new();
    let mut order: Vec<String> = vec![];
    for f in files {
        let parent = f.parent().unwrap_or(Path::new(""));
        let rel = parent.strip_prefix(input_dir).unwrap_or(parent);
        let rel_posix = rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy().to_string())
            .collect::<Vec<_>>()
            .join("/");
        let key = if rel_posix == "." { String::new() } else { rel_posix };
        if !by_dir.contains_key(&key) {
            order.push(key.clone());
        }
        by_dir.entry(key).or_default().push(f.clone());
    }

    let mut planned: HashMap<PathBuf, Job> = HashMap::new();
    for rel_dir in &order {
        let group = by_dir.get(rel_dir).unwrap();
        let mut used: HashSet<String> = HashSet::new(); // không phân biệt hoa thường
        let mut sorted = group.clone();
        sorted.sort_by_key(|p| {
            p.file_name().unwrap_or_default().to_string_lossy().to_lowercase()
        });
        for f in &sorted {
            let stem = f.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            let mut name = format!("{stem}.jpg");
            let mut n = 0;
            while used.contains(&name.to_lowercase()) {
                n += 1;
                name = format!("{stem}_{n}.jpg");
            }
            used.insert(name.to_lowercase());
            let renamed = n > 0;
            let mut out = output_dir.to_path_buf();
            for part in rel_dir.split('/').filter(|p| !p.is_empty()) {
                out.push(part);
            }
            out.push(&name);
            planned.insert(
                f.clone(),
                Job {
                    input_path: f.clone(),
                    output_path: out,
                    rel_dir: rel_dir.clone(),
                    renamed,
                },
            );
        }
    }
    files.iter().map(|f| planned.get(f).unwrap().clone()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(p: &Path) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, b"x").unwrap();
    }

    #[test]
    fn extension_filter_case_insensitive() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        for name in [
            "a.JPG", "b.jpeg", "c.PNG", "d.tif", "e.TIFF", "f.webp", "g.bmp", "h.txt", "i.cr2",
            "j.gif", ".hidden.jpg", "Thumbs.db", "desktop.ini",
        ] {
            touch(&root.join(name));
        }
        let names: Vec<String> = scan_folder(root)
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
            .collect();
        assert_eq!(
            names,
            vec!["a.JPG", "b.jpeg", "c.PNG", "d.tif", "e.TIFF", "f.webp", "g.bmp"]
        );
    }

    #[test]
    fn subfolders_and_order() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        touch(&root.join("b.jpg"));
        touch(&root.join("A.jpg"));
        touch(&root.join("outdoor/x.png"));
        touch(&root.join(".cache/y.png"));
        let files = scan_folder(root);
        let rel: Vec<String> = files
            .iter()
            .map(|p| {
                p.strip_prefix(root)
                    .unwrap()
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().to_string())
                    .collect::<Vec<_>>()
                    .join("/")
            })
            .collect();
        assert_eq!(rel, vec!["A.jpg", "b.jpg", "outdoor/x.png"]);
        let out = root.parent().unwrap().join("out");
        let jobs = plan_jobs(root, &files, &out);
        assert_eq!(jobs[2].output_path, out.join("outdoor").join("x.jpg"));
        assert_eq!(jobs[2].rel_dir, "outdoor");
        assert_eq!(jobs[0].rel_dir, "");
    }

    #[test]
    fn name_collision() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        for n in ["a.png", "a.jpg", "a.tif"] {
            touch(&root.join(n));
        }
        let jobs = plan_jobs(root, &scan_folder(root), &root.parent().unwrap().join("o"));
        let by_input: HashMap<String, &Job> = jobs
            .iter()
            .map(|j| (j.input_path.file_name().unwrap().to_string_lossy().to_string(), j))
            .collect();
        assert_eq!(by_input["a.jpg"].output_path.file_name().unwrap(), "a.jpg");
        assert!(!by_input["a.jpg"].renamed);
        assert_eq!(by_input["a.png"].output_path.file_name().unwrap(), "a_1.jpg");
        assert!(by_input["a.png"].renamed);
        assert_eq!(by_input["a.tif"].output_path.file_name().unwrap(), "a_2.jpg");
        let unique: HashSet<_> = jobs.iter().map(|j| &j.output_path).collect();
        assert_eq!(unique.len(), 3);
    }

    #[test]
    fn name_keeps_unicode_and_lowercase_ext() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        touch(&root.join("IMG_001.PNG"));
        touch(&root.join("ảnh cưới 01.jpeg"));
        let mut names: Vec<String> = plan_jobs(root, &scan_folder(root), &root.join("o"))
            .iter()
            .map(|j| j.output_path.file_name().unwrap().to_string_lossy().to_string())
            .collect();
        names.sort();
        assert_eq!(names, vec!["IMG_001.jpg", "ảnh cưới 01.jpg"]);
    }

    #[test]
    fn output_folder_names() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().canonicalize().unwrap();
        let src = base.join("clientA");
        std::fs::create_dir(&src).unwrap();
        assert_eq!(default_output_dir(&src).unwrap(), base.join("clientA_update"));
        assert_eq!(next_free_output_dir(&src).unwrap(), base.join("clientA_update"));
        std::fs::create_dir(base.join("clientA_update")).unwrap();
        assert_eq!(next_free_output_dir(&src).unwrap(), base.join("clientA_update_2"));
        std::fs::create_dir(base.join("clientA_update_2")).unwrap();
        assert_eq!(next_free_output_dir(&src).unwrap(), base.join("clientA_update_3"));
    }

    #[test]
    fn drive_root_rejected() {
        assert!(default_output_dir(Path::new("/")).is_err());
    }
}
