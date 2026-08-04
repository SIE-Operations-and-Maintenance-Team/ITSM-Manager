// 全局应用状态、凭证、路径、时间、原子写工具
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

pub const DEFAULT_TENANT: &str = "466553071648915456";
pub const DEFAULT_SEACH_TYPE: i64 = 2;

/// 共享 token 存储：克隆后仍指向同一把锁，允许 MCP handler 等非 Tauri 线程读取当前登录 token
#[derive(Clone, Default)]
pub struct TokenStore(Arc<Mutex<Option<String>>>);

impl TokenStore {
    pub fn get(&self) -> Result<String, String> {
        self.0
            .lock()
            .expect("token mutex poisoned")
            .clone()
            .ok_or_else(|| "未登录".into())
    }

    pub fn set(&self, token: Option<String>) {
        *self.0.lock().expect("token mutex poisoned") = token;
    }
}

pub struct AppState {
    pub token: TokenStore,
    pub tenant_id: Mutex<String>,
    pub user_name: Mutex<String>,
    pub client: reqwest::Client,
    /// 每视图当前页码（内存态，重启回 1；前端切页/切视图时上报）
    pub current_pages: Mutex<HashMap<i64, i64>>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Creds {
    pub token: String,
    pub tenant_id: String,
    pub user_name: String,
}

/// "记住密码"保存的账密（经 keyring crate 存 Windows Credential Manager，
/// service=itsm-manager / user=default；commands.rs load_stored_cred 内嵌旧明文惰性迁移）
#[derive(Serialize, Deserialize, Clone)]
pub struct StoredCred {
    pub account: String,
    pub password: String,
}

pub fn app_data_dir(app: &tauri::AppHandle) -> Option<PathBuf> {
    use tauri::Manager;
    app.path().app_data_dir().ok().map(|dir| {
        let _ = fs::create_dir_all(&dir);
        dir
    })
}

pub fn creds_path(app: &tauri::AppHandle) -> Option<PathBuf> {
    app_data_dir(app).map(|dir| dir.join("credentials.json"))
}

pub fn stored_cred_path(app: &tauri::AppHandle) -> Option<PathBuf> {
    app_data_dir(app).map(|dir| dir.join("stored-cred.json"))
}

pub fn get_token(state: &tauri::State<AppState>) -> Result<String, String> {
    state.token.get()
}

pub fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// 临时文件 + rename 原子写，防写一半崩溃污染
pub fn atomic_write(path: &std::path::Path, content: &str) -> Result<(), String> {
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, content).map_err(|e| e.to_string())?;
    fs::rename(&tmp, path).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_store_missing_returns_not_logged_in() {
        let store = TokenStore::default();
        assert_eq!(store.get().unwrap_err(), "未登录");
    }

    #[test]
    fn token_store_clones_share_updates() {
        let store = TokenStore::default();
        let clone = store.clone();
        store.set(Some("token-a".into()));
        assert_eq!(clone.get().unwrap(), "token-a");
        clone.set(Some("token-b".into()));
        assert_eq!(store.get().unwrap(), "token-b");
    }

    #[test]
    fn token_store_clear_removes_token() {
        let store = TokenStore::default();
        store.set(Some("token-a".into()));
        store.set(None);
        assert_eq!(store.get().unwrap_err(), "未登录");
    }
}
