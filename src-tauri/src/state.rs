// 全局应用状态、凭证、路径、时间、原子写工具
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

pub const DEFAULT_TENANT: &str = "466553071648915456";
pub const DEFAULT_SEACH_TYPE: i64 = 2;

pub struct AppState {
    pub token: Mutex<Option<String>>,
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

pub fn get_token(state: &tauri::State<AppState>) -> Result<String, String> {
    state
        .token
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(|| "未登录".into())
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
