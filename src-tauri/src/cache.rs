// 工单列表本地缓存（整存整取，按 seachType 分文件）
use crate::state::{app_data_dir, atomic_write};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use tauri::AppHandle;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct CachedTickets {
    pub fetched_at: i64,
    pub count: i64,
    #[serde(default)]
    pub page_index: i64,
    #[serde(default)]
    pub page_size: i64,
    pub data: Value,
}

pub fn cache_dir(app: &AppHandle) -> Option<PathBuf> {
    app_data_dir(app).map(|d| {
        let dir = d.join("cache");
        let _ = fs::create_dir_all(&dir);
        dir
    })
}

fn tickets_filename(seach_type: i64) -> String {
    format!("tickets_{}.json", seach_type)
}

pub fn read_tickets(app: &AppHandle, seach_type: i64, page_index: i64, page_size: i64) -> Option<CachedTickets> {
    let p = cache_dir(app)?.join(tickets_filename(seach_type));
    let c = read_tickets_at(&p)?;
    // 页码或页大小不匹配视为未命中（pageSize 切换后自动失效）
    if c.page_index == page_index && c.page_size == page_size {
        Some(c)
    } else {
        None
    }
}

pub fn write_tickets(
    app: &AppHandle,
    seach_type: i64,
    page_index: i64,
    page_size: i64,
    data: &Value,
    count: i64,
    fetched_at: i64,
) -> Result<(), String> {
    let p = cache_dir(app)
        .ok_or_else(|| "无法定位缓存目录".to_string())?
        .join(tickets_filename(seach_type));
    write_tickets_at(&p, page_index, page_size, data, count, fetched_at)
}

pub fn read_views(app: &AppHandle) -> Option<Value> {
    let p = cache_dir(app)?.join("views.json");
    let s = fs::read_to_string(&p).ok()?;
    serde_json::from_str::<Value>(&s).ok()
}

pub fn write_views(app: &AppHandle, views: &Value) -> Result<(), String> {
    let p = cache_dir(app)
        .ok_or_else(|| "无法定位缓存目录".to_string())?
        .join("views.json");
    let s = serde_json::to_string_pretty(views).map_err(|e| e.to_string())?;
    atomic_write(&p, &s)
}

pub fn invalidate_all_whitelist(app: &AppHandle, whitelist: &[i64]) {
    if let Some(dir) = cache_dir(app) {
        invalidate_at(&dir, whitelist);
    }
}

pub fn clear_all(app: &AppHandle) {
    if let Some(dir) = cache_dir(app) {
        let _ = fs::remove_dir_all(&dir);
    }
}

// —— 纯函数核心（不依赖 AppHandle，可单测）——

fn read_tickets_at(path: &Path) -> Option<CachedTickets> {
    let s = fs::read_to_string(path).ok()?;
    serde_json::from_str(&s).ok()
}

fn write_tickets_at(path: &Path, page_index: i64, page_size: i64, data: &Value, count: i64, fetched_at: i64) -> Result<(), String> {
    let cached = CachedTickets { fetched_at, count, page_index, page_size, data: data.clone() };
    let s = serde_json::to_string_pretty(&cached).map_err(|e| e.to_string())?;
    atomic_write(path, &s)
}

fn invalidate_at(dir: &Path, whitelist: &[i64]) {
    for st in whitelist {
        let _ = fs::remove_file(dir.join(tickets_filename(*st)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::env;

    fn tmp_file(name: &str) -> PathBuf {
        let mut p = env::temp_dir();
        p.push(format!("itsm_cache_test_{}_{}.json", std::process::id(), name));
        let _ = fs::remove_file(&p);
        p
    }

    #[test]
    fn write_then_read_roundtrip() {
        let p = tmp_file("rt");
        let data = json!([{"incidentId": "X"}]);
        write_tickets_at(&p, 1, 50, &data, 1, 1000).unwrap();
        let c = read_tickets_at(&p).unwrap();
        assert_eq!(c.count, 1);
        assert_eq!(c.fetched_at, 1000);
        assert_eq!(c.page_index, 1);
        assert_eq!(c.page_size, 50);
        assert_eq!(c.data, data);
        let _ = fs::remove_file(&p);
    }

    #[test]
    fn read_missing_returns_none() {
        let p = tmp_file("none");
        let _ = fs::remove_file(&p);
        assert!(read_tickets_at(&p).is_none());
    }

    #[test]
    fn read_corrupt_returns_none() {
        let p = tmp_file("corrupt");
        fs::write(&p, "{not json").unwrap();
        assert!(read_tickets_at(&p).is_none());
        let _ = fs::remove_file(&p);
    }

    #[test]
    fn invalidate_at_removes_only_whitelist() {
        let dir = env::temp_dir().join(format!("itsm_cache_inv_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("tickets_1.json"), "{}").unwrap();
        fs::write(dir.join("tickets_2.json"), "{}").unwrap();
        fs::write(dir.join("tickets_9.json"), "{}").unwrap();
        invalidate_at(&dir, &[1, 2]);
        assert!(!dir.join("tickets_1.json").exists());
        assert!(!dir.join("tickets_2.json").exists());
        assert!(dir.join("tickets_9.json").exists());
        let _ = fs::remove_dir_all(&dir);
    }
}
