//! Đăng nhập cục bộ: một tài khoản lưu dạng hash PBKDF2 có salt trong `auth.json`.
//! Port của `core/auth.py`. Đây là khoá tiện lợi cho app desktop, không phải ranh giới bảo mật.

use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};
use sha2::Sha256;

pub const DEFAULT_USERNAME: &str = "admin";
pub const DEFAULT_PASSWORD: &str = "admin";
pub const ITERATIONS: u32 = 120_000;
pub const AUTH_FORMAT_VERSION: i64 = 1;

pub fn hash_password(password: &str, salt: &[u8], iterations: u32) -> String {
    let mut out = [0u8; 32];
    pbkdf2::pbkdf2_hmac::<Sha256>(password.as_bytes(), salt, iterations, &mut out);
    hex::encode(out)
}

fn random_salt() -> [u8; 16] {
    let mut salt = [0u8; 16];
    getrandom::getrandom(&mut salt).expect("os random");
    salt
}

#[derive(Debug)]
pub struct AuthError(pub String);

impl std::fmt::Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl std::error::Error for AuthError {}

/// Đọc/ghi `auth.json`. File thiếu hoặc hỏng nghĩa là tài khoản mặc định.
pub struct AuthStore {
    pub path: PathBuf,
    data: Map<String, Value>,
}

impl AuthStore {
    pub fn new(path: &Path) -> Self {
        let data = Self::load(path);
        AuthStore {
            path: path.to_path_buf(),
            data,
        }
    }

    fn default_data() -> Map<String, Value> {
        let salt = random_salt();
        let mut m = Map::new();
        m.insert("version".into(), json!(AUTH_FORMAT_VERSION));
        m.insert("username".into(), json!(DEFAULT_USERNAME));
        m.insert("salt".into(), json!(hex::encode(salt)));
        m.insert("iterations".into(), json!(ITERATIONS));
        m.insert("hash".into(), json!(hash_password(DEFAULT_PASSWORD, &salt, ITERATIONS)));
        m.insert("remember".into(), json!(false));
        m.insert("is_default".into(), json!(true));
        m
    }

    fn load(path: &Path) -> Map<String, Value> {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(_) => return Self::default_data(),
        };
        let data: Value = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(_) => return Self::default_data(),
        };
        let obj = match data.as_object() {
            Some(o) => o,
            None => return Self::default_data(),
        };
        let username_ok = obj
            .get("username")
            .and_then(|v| v.as_str())
            .map(|s| !s.is_empty())
            .unwrap_or(false);
        let salt_str = obj.get("salt").and_then(|v| v.as_str());
        let hash_ok = obj.get("hash").and_then(|v| v.as_str()).is_some();
        let iterations_ok = obj
            .get("iterations")
            .and_then(|v| v.as_i64())
            .map(|i| i > 0)
            .unwrap_or(false);
        if !(username_ok && salt_str.is_some() && hash_ok && iterations_ok) {
            return Self::default_data();
        }
        if hex::decode(salt_str.unwrap()).is_err() {
            return Self::default_data();
        }
        let mut m = obj.clone();
        let remember = matches!(m.get("remember"), Some(Value::Bool(true)));
        let is_default = matches!(m.get("is_default"), Some(Value::Bool(true)));
        m.insert("remember".into(), json!(remember));
        m.insert("is_default".into(), json!(is_default));
        m
    }

    fn save(&self) -> std::io::Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(&Value::Object(self.data.clone()))
            .expect("serialize auth");
        let tmp = self.path.with_file_name(format!(
            "{}.tmp",
            self.path.file_name().and_then(|n| n.to_str()).unwrap_or("auth.json")
        ));
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, &self.path)?;
        Ok(())
    }

    fn get_str(&self, key: &str) -> &str {
        self.data.get(key).and_then(|v| v.as_str()).unwrap_or("")
    }

    fn iterations(&self) -> u32 {
        self.data.get("iterations").and_then(|v| v.as_i64()).unwrap_or(0) as u32
    }

    pub fn username(&self) -> &str {
        self.get_str("username")
    }

    pub fn uses_default_credentials(&self) -> bool {
        matches!(self.data.get("is_default"), Some(Value::Bool(true)))
    }

    pub fn is_remembered(&self) -> bool {
        matches!(self.data.get("remember"), Some(Value::Bool(true)))
    }

    pub fn verify(&self, username: &str, password: &str) -> bool {
        if username.trim().to_lowercase() != self.username().to_lowercase() {
            // vẫn hash để sai tên mất thời gian như sai mật khẩu
            let _ = hash_password(password, b"0000000000000000", self.iterations());
            return false;
        }
        let salt = match hex::decode(self.get_str("salt")) {
            Ok(s) => s,
            Err(_) => return false,
        };
        let digest = hash_password(password, &salt, self.iterations());
        digest == self.get_str("hash")
    }

    pub fn login(&mut self, username: &str, password: &str, remember: bool) -> bool {
        if !self.verify(username, password) {
            return false;
        }
        self.set_remembered(remember);
        true
    }

    pub fn set_remembered(&mut self, remember: bool) {
        self.data.insert("remember".into(), json!(remember));
        let _ = self.save();
    }

    pub fn logout(&mut self) {
        self.set_remembered(false);
    }

    pub fn change_credentials(
        &mut self,
        current_password: &str,
        new_username: &str,
        new_password: &str,
    ) -> Result<(), AuthError> {
        let new_username = new_username.trim().to_string();
        let current_user = self.username().to_string();
        if !self.verify(&current_user, current_password) {
            return Err(AuthError("The current password is incorrect.".into()));
        }
        if new_username.is_empty() {
            return Err(AuthError("The user name cannot be empty.".into()));
        }
        if new_username.chars().count() > 64 {
            return Err(AuthError("The user name is too long (64 characters max).".into()));
        }
        if !new_password.is_empty() && new_password.chars().count() < 4 {
            return Err(AuthError("The new password must have at least 4 characters.".into()));
        }
        let salt = random_salt();
        let password = if new_password.is_empty() {
            current_password
        } else {
            new_password
        };
        let old = self.data.clone();
        self.data.insert("username".into(), json!(new_username));
        self.data.insert("salt".into(), json!(hex::encode(salt)));
        self.data.insert("iterations".into(), json!(ITERATIONS));
        self.data.insert("hash".into(), json!(hash_password(password, &salt, ITERATIONS)));
        self.data.insert(
            "is_default".into(),
            json!(new_username == DEFAULT_USERNAME && password == DEFAULT_PASSWORD),
        );
        if let Err(e) = self.save() {
            self.data = old;
            return Err(AuthError(format!("Could not save the account: {e}")));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_account() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("auth.json");
        let a = AuthStore::new(&path);
        assert_eq!(a.username(), "admin");
        assert!(a.uses_default_credentials());
        assert!(!a.is_remembered());
        assert!(a.verify("admin", "admin"));
        assert!(a.verify(" Admin ", "admin"));
        assert!(!a.verify("admin", "wrong"));
        assert!(!a.verify("root", "admin"));
        assert!(!path.exists()); // chưa ghi gì cho tới khi login
    }

    #[test]
    fn login_remembered_across_restarts() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("auth.json");
        assert!(!AuthStore::new(&path).login("admin", "nope", true));
        assert!(AuthStore::new(&path).login("admin", "admin", true));
        let data: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert!(!data["hash"].as_str().unwrap().contains("admin"));
        assert_eq!(data["remember"], json!(true));
        let b = AuthStore::new(&path);
        assert!(b.is_remembered());
        let mut b = b;
        b.logout();
        assert!(!AuthStore::new(&path).is_remembered());
        assert!(AuthStore::new(&path).login("admin", "admin", false));
        assert!(!AuthStore::new(&path).is_remembered());
    }

    #[test]
    fn change_credentials() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("auth.json");
        let mut a = AuthStore::new(&path);
        assert!(a.change_credentials("bad", "admin", "secret").is_err());
        assert!(a.change_credentials("admin", "  ", "secret").is_err());
        assert!(a.change_credentials("admin", "boss", "abc").is_err());
        a.change_credentials("admin", " boss ", "s3cret").unwrap();
        let b = AuthStore::new(&path);
        assert_eq!(b.username(), "boss");
        assert!(!b.uses_default_credentials());
        assert!(b.verify("boss", "s3cret"));
        assert!(!b.verify("admin", "admin"));
        let mut b = b;
        b.change_credentials("s3cret", "chief", "").unwrap(); // mật khẩu mới rỗng -> giữ cũ
        assert!(AuthStore::new(&path).verify("chief", "s3cret"));
    }

    #[test]
    fn broken_file_falls_back_to_default() {
        for content in [
            "{broken",
            "[]",
            "{\"username\": \"x\"}",
            "{\"username\": \"x\", \"salt\": \"zz\", \"hash\": \"a\", \"iterations\": 5}",
        ] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("auth.json");
            std::fs::write(&path, content).unwrap();
            assert!(AuthStore::new(&path).verify("admin", "admin"), "{content}");
        }
    }
}
