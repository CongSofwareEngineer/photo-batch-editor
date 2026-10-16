//! Windows: nhúng icon (`assets/app.ico`) + thông tin version (từ `version.json`) vào exe —
//! Explorer › Properties › Details hiện đúng version, exe có icon riêng, và antivirus ít nghi
//! một exe "trần" không có version resource.

use std::path::Path;

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let version_file = root.join("version.json");
    let icon = root.join("assets/app.ico");
    println!("cargo:rerun-if-changed={}", version_file.display());
    println!("cargo:rerun-if-changed={}", icon.display());

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let (version, build) = read_version(&version_file);
    let mut nums = [0u64; 4];
    for (i, p) in version.split('.').take(3).enumerate() {
        nums[i] = p.parse().unwrap_or(0);
    }
    nums[3] = build;
    let full = format!("{}.{}.{}.{}", nums[0], nums[1], nums[2], nums[3]);
    let packed = (nums[0] << 48) | (nums[1] << 32) | (nums[2] << 16) | nums[3];

    let mut res = winresource::WindowsResource::new();
    res.set_icon(&icon.to_string_lossy())
        .set("CompanyName", "Photo Batch Editor")
        .set("FileDescription", "Photo Batch Editor")
        .set("ProductName", "Photo Batch Editor")
        .set("InternalName", "photo-batch-editor")
        .set("OriginalFilename", "photo-batch-editor.exe")
        .set("LegalCopyright", "Copyright (c) Photo Batch Editor")
        .set("FileVersion", &full)
        .set("ProductVersion", &full)
        .set_version_info(winresource::VersionInfo::FILEVERSION, packed)
        .set_version_info(winresource::VersionInfo::PRODUCTVERSION, packed);
    if let Err(e) = res.compile() {
        println!("cargo:warning=Không nhúng được icon/version vào exe: {e}");
    }
}

/// `("X.Y.Z", build)` từ `version.json`; `("0.0.0", 0)` nếu thiếu / hỏng.
fn read_version(path: &Path) -> (String, u64) {
    let Ok(text) = std::fs::read_to_string(path) else {
        return ("0.0.0".into(), 0);
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
        return ("0.0.0".into(), 0);
    };
    let version = v["version"].as_str().unwrap_or("0.0.0").to_string();
    let build = v["build"].as_u64().unwrap_or(0);
    (version, build)
}
