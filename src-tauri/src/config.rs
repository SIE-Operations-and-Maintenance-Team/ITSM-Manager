// 用户刷新配置：白名单视图 + 间隔
use crate::state::{app_data_dir, atomic_write};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use tauri::AppHandle;

const MIN_INTERVAL: u64 = 30;
const MAX_INTERVAL: u64 = 1800;
const DEFAULT_INTERVAL: u64 = 300;

/// 默认分页大小；view_page_sizes 未覆盖的视图用此值
pub const DEFAULT_PAGE_SIZE: i64 = 50;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Config {
    pub whitelist: Vec<i64>,
    pub interval_sec: u64,
    /// 补单默认客户组（前端 autocomplete 选，None 表示未设）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_customer_group_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_customer_group_name: Option<String>,
    /// 补单默认提单人
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_requestor_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_requestor_name: Option<String>,
    /// 补单默认支持组（save body 的 assign 字段必填）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_support_group_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_support_group_name: Option<String>,
    /// 每视图分页大小（key=seachType 字符串，value=50/100/200）。空=全用 DEFAULT_PAGE_SIZE
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub view_page_sizes: HashMap<String, i64>,
}

impl Config {
    pub fn default_with(seach_type: i64) -> Self {
        Self {
            whitelist: vec![seach_type],
            interval_sec: DEFAULT_INTERVAL,
            default_customer_group_id: None,
            default_customer_group_name: None,
            default_requestor_id: None,
            default_requestor_name: None,
            default_support_group_id: None,
            default_support_group_name: None,
            view_page_sizes: HashMap::new(),
        }
    }

    /// 取某视图的分页大小；未配置或非法值回退默认 50
    pub fn page_size_for(&self, seach_type: i64) -> i64 {
        self.view_page_sizes
            .get(&seach_type.to_string())
            .copied()
            .filter(|&n| matches!(n, 50 | 100 | 200))
            .unwrap_or(DEFAULT_PAGE_SIZE)
    }

    pub fn clamp_interval(sec: u64) -> u64 {
        if sec == 0 { 0 } else { sec.clamp(MIN_INTERVAL, MAX_INTERVAL) }
    }

    pub fn dedup(mut self) -> Self {
        self.whitelist.sort_unstable();
        self.whitelist.dedup();
        self
    }

    pub fn normalize(self) -> Self {
        Self { interval_sec: Self::clamp_interval(self.interval_sec), ..self }.dedup()
    }
}

fn config_path(app: &AppHandle) -> Option<std::path::PathBuf> {
    app_data_dir(app).map(|d| d.join("config.json"))
}

pub fn load(app: &AppHandle, fallback_st: i64) -> Config {
    if let Some(p) = config_path(app) {
        if let Ok(s) = fs::read_to_string(&p) {
            if let Ok(c) = serde_json::from_str::<Config>(&s) {
                return c.normalize();
            }
        }
    }
    let def = Config::default_with(fallback_st);
    let _ = save(app, &def);
    def
}

pub fn save(app: &AppHandle, cfg: &Config) -> Result<(), String> {
    let p = config_path(app).ok_or_else(|| "无法定位 app_data_dir".to_string())?;
    let normalized = cfg.clone().normalize();
    let s = serde_json::to_string_pretty(&normalized).map_err(|e| e.to_string())?;
    atomic_write(&p, &s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_with_sets_single_whitelist_and_300() {
        let c = Config::default_with(7);
        assert_eq!(c.whitelist, vec![7]);
        assert_eq!(c.interval_sec, 300);
    }

    #[test]
    fn clamp_below_min() {
        assert_eq!(Config::clamp_interval(0), 0);   // 0 = pause, preserved (not clamped to 30)
        assert_eq!(Config::clamp_interval(29), 30);
    }

    #[test]
    fn clamp_above_max() {
        assert_eq!(Config::clamp_interval(1801), 1800);
        assert_eq!(Config::clamp_interval(99999), 1800);
    }

    #[test]
    fn clamp_in_range() {
        assert_eq!(Config::clamp_interval(120), 120);
    }

    #[test]
    fn dedup_removes_duplicates_sorted() {
        let c = Config { whitelist: vec![3, 1, 2, 1, 3], interval_sec: 300, default_customer_group_id: None, default_customer_group_name: None, default_requestor_id: None, default_requestor_name: None, default_support_group_id: None, default_support_group_name: None, view_page_sizes: HashMap::new() }.dedup();
        assert_eq!(c.whitelist, vec![1, 2, 3]);
    }

    #[test]
    fn normalize_clamps_and_dedups() {
        let c = Config { whitelist: vec![2, 2, 1], interval_sec: 5, default_customer_group_id: None, default_customer_group_name: None, default_requestor_id: None, default_requestor_name: None, default_support_group_id: None, default_support_group_name: None, view_page_sizes: HashMap::new() }.normalize();
        assert_eq!(c.whitelist, vec![1, 2]);
        assert_eq!(c.interval_sec, 30);
    }

    #[test]
    fn serialize_deserialize_roundtrip() {
        let c = Config { whitelist: vec![1, 2], interval_sec: 300, default_customer_group_id: None, default_customer_group_name: None, default_requestor_id: None, default_requestor_name: None, default_support_group_id: None, default_support_group_name: None, view_page_sizes: HashMap::new() };
        let s = serde_json::to_string(&c).unwrap();
        let c2: Config = serde_json::from_str(&s).unwrap();
        assert_eq!(c, c2);
    }

    #[test]
    fn legacy_config_without_new_fields_loads() {
        // 旧 config.json 没有新字段，反序列化应填充 None
        let legacy = r#"{"whitelist":[2],"interval_sec":300}"#;
        let c: Config = serde_json::from_str(legacy).unwrap();
        assert_eq!(c.whitelist, vec![2]);
        assert_eq!(c.default_customer_group_id, None);
        assert_eq!(c.default_requestor_id, None);
        assert!(c.view_page_sizes.is_empty());
    }
}
