// 用户刷新配置：白名单视图 + 间隔
use crate::state::{app_data_dir, atomic_write};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use tauri::AppHandle;

const MIN_INTERVAL: u64 = 30;
const MAX_INTERVAL: u64 = 1800;
const DEFAULT_INTERVAL: u64 = 300;
const DEFAULT_MCP_PORT: u16 = 17540;

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
    /// 详情面板宽度百分比（20–70）；None = 默认 35%（由前端 CSS 变量回退）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail_width_pct: Option<f64>,
    /// 开机自启。默认关 —— 用户主动开。
    #[serde(default)]
    pub autostart_enabled: bool,
    /// 关闭按钮(×)进托盘而非退出。默认开（老 config 缺字段补 true）。
    #[serde(default = "default_true")]
    pub minimize_to_tray: bool,
    /// 首次关闭进托盘气泡已弹过。弹后置 true，不再弹。
    #[serde(default)]
    pub tray_hint_shown: bool,
    /// 暂停前的有效刷新间隔，用于托盘"恢复"。总是 > 0。
    #[serde(default = "default_last_interval")]
    pub last_interval_sec: u64,
    /// 启用自动接单（仅对 auto_claim_seach_type 视图生效）。默认关——用户主动开。
    #[serde(default)]
    pub auto_claim_enabled: bool,
    /// 自动接单目标视图 seachType；首次启动由前端按 viewName='待我接单' 自动填。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_claim_seach_type: Option<i64>,
    /// 启用本机 MCP server。默认开；配置变化需重启应用生效。
    #[serde(default = "default_true")]
    pub mcp_enabled: bool,
    /// 本机 MCP server 端口，合法范围 1024..=65535。
    #[serde(default = "default_mcp_port")]
    pub mcp_port: u16,
}

fn default_true() -> bool { true }
fn default_last_interval() -> u64 { DEFAULT_INTERVAL }
fn default_mcp_port() -> u16 { DEFAULT_MCP_PORT }

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
            detail_width_pct: None,
            autostart_enabled: false,
            minimize_to_tray: true,
            tray_hint_shown: false,
            last_interval_sec: DEFAULT_INTERVAL,
            auto_claim_enabled: false,
            auto_claim_seach_type: None,
            mcp_enabled: true,
            mcp_port: DEFAULT_MCP_PORT,
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

    pub fn clamp_mcp_port(port: u16) -> u16 {
        if port >= 1024 { port } else { DEFAULT_MCP_PORT }
    }

    pub fn clamp_detail_width(pct: Option<f64>) -> Option<f64> {
        pct.map(|p| p.clamp(20.0, 70.0))
    }

    pub fn dedup(mut self) -> Self {
        self.whitelist.sort_unstable();
        self.whitelist.dedup();
        self
    }

    pub fn normalize(self) -> Self {
        Self {
            interval_sec: Self::clamp_interval(self.interval_sec),
            detail_width_pct: Self::clamp_detail_width(self.detail_width_pct),
            last_interval_sec: Self::clamp_interval(self.last_interval_sec).max(MIN_INTERVAL),
            mcp_port: Self::clamp_mcp_port(self.mcp_port),
            ..self
        }.dedup()
    }

    /// 托盘"暂停/恢复"切换的取值决策（纯函数）
    pub fn toggled_pause(mut self) -> Self {
        if self.interval_sec > 0 {
            self.last_interval_sec = self.interval_sec;
            self.interval_sec = 0;
        } else {
            self.interval_sec = self.last_interval_sec.max(MIN_INTERVAL);
        }
        self
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
        let c = Config { whitelist: vec![3, 1, 2, 1, 3], interval_sec: 300, default_customer_group_id: None, default_customer_group_name: None, default_requestor_id: None, default_requestor_name: None, default_support_group_id: None, default_support_group_name: None, view_page_sizes: HashMap::new(), detail_width_pct: None, autostart_enabled: false, minimize_to_tray: true, tray_hint_shown: false, last_interval_sec: 300, auto_claim_enabled: false, auto_claim_seach_type: None, mcp_enabled: true, mcp_port: 17540 }.dedup();
        assert_eq!(c.whitelist, vec![1, 2, 3]);
    }

    #[test]
    fn normalize_clamps_and_dedups() {
        let c = Config { whitelist: vec![2, 2, 1], interval_sec: 5, default_customer_group_id: None, default_customer_group_name: None, default_requestor_id: None, default_requestor_name: None, default_support_group_id: None, default_support_group_name: None, view_page_sizes: HashMap::new(), detail_width_pct: None, autostart_enabled: false, minimize_to_tray: true, tray_hint_shown: false, last_interval_sec: 300, auto_claim_enabled: false, auto_claim_seach_type: None, mcp_enabled: true, mcp_port: 17540 }.normalize();
        assert_eq!(c.whitelist, vec![1, 2]);
        assert_eq!(c.interval_sec, 30);
    }

    #[test]
    fn serialize_deserialize_roundtrip() {
        let c = Config { whitelist: vec![1, 2], interval_sec: 300, default_customer_group_id: None, default_customer_group_name: None, default_requestor_id: None, default_requestor_name: None, default_support_group_id: None, default_support_group_name: None, view_page_sizes: HashMap::new(), detail_width_pct: None, autostart_enabled: false, minimize_to_tray: true, tray_hint_shown: false, last_interval_sec: 300, auto_claim_enabled: false, auto_claim_seach_type: None, mcp_enabled: true, mcp_port: 17540 };
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
        assert_eq!(c.detail_width_pct, None);
    }

    #[test]
    fn clamp_detail_width_below_min() {
        assert_eq!(Config::clamp_detail_width(Some(15.0)), Some(20.0));
    }

    #[test]
    fn clamp_detail_width_above_max() {
        assert_eq!(Config::clamp_detail_width(Some(80.0)), Some(70.0));
    }

    #[test]
    fn clamp_detail_width_in_range() {
        assert_eq!(Config::clamp_detail_width(Some(42.5)), Some(42.5));
    }

    #[test]
    fn clamp_detail_width_none_stays_none() {
        assert_eq!(Config::clamp_detail_width(None), None);
    }

    #[test]
    fn normalize_clamps_detail_width() {
        let c = Config {
            whitelist: vec![1], interval_sec: 300,
            default_customer_group_id: None, default_customer_group_name: None,
            default_requestor_id: None, default_requestor_name: None,
            default_support_group_id: None, default_support_group_name: None,
            view_page_sizes: HashMap::new(),
            detail_width_pct: Some(10.0),
            autostart_enabled: false, minimize_to_tray: true,
            tray_hint_shown: false, last_interval_sec: 300,
            auto_claim_enabled: false, auto_claim_seach_type: None,
            mcp_enabled: true, mcp_port: 17540,
        }.normalize();
        assert_eq!(c.detail_width_pct, Some(20.0));
    }

    #[test]
    fn legacy_config_new_bool_fields_defaults() {
        // 老 config 无 4 个新字段，反序列化应填充默认
        let legacy = r#"{"whitelist":[2],"interval_sec":300}"#;
        let c: Config = serde_json::from_str(legacy).unwrap();
        assert_eq!(c.autostart_enabled, false);
        assert_eq!(c.minimize_to_tray, true, "minimize_to_tray 老配置默认开");
        assert_eq!(c.tray_hint_shown, false);
        assert_eq!(c.last_interval_sec, 300);
    }

    #[test]
    fn last_interval_clamped_no_zero() {
        // normalize 强制 last_interval_sec >= 30，不允许 0
        let base = || Config {
            whitelist: vec![1], interval_sec: 300,
            default_customer_group_id: None, default_customer_group_name: None,
            default_requestor_id: None, default_requestor_name: None,
            default_support_group_id: None, default_support_group_name: None,
            view_page_sizes: HashMap::new(), detail_width_pct: None,
            autostart_enabled: false, minimize_to_tray: true,
            tray_hint_shown: false, last_interval_sec: 0,
            auto_claim_enabled: false, auto_claim_seach_type: None,
            mcp_enabled: true, mcp_port: 17540,
        };
        assert_eq!(base().normalize().last_interval_sec, 30, "0 -> 最小 30");
        let mut c = base(); c.last_interval_sec = 5;
        assert_eq!(c.normalize().last_interval_sec, 30, "低于最小 -> 30");
        let mut c = base(); c.last_interval_sec = 99999;
        assert_eq!(c.normalize().last_interval_sec, 1800, "超过最大 -> 1800");
    }

    #[test]
    fn normalize_keeps_last_when_interval_zero() {
        let mut c = Config {
            whitelist: vec![1], interval_sec: 0,
            default_customer_group_id: None, default_customer_group_name: None,
            default_requestor_id: None, default_requestor_name: None,
            default_support_group_id: None, default_support_group_name: None,
            view_page_sizes: HashMap::new(), detail_width_pct: None,
            autostart_enabled: false, minimize_to_tray: true,
            tray_hint_shown: false, last_interval_sec: 120,
            auto_claim_enabled: false, auto_claim_seach_type: None,
            mcp_enabled: true, mcp_port: 17540,
        };
        c = c.normalize();
        assert_eq!(c.interval_sec, 0, "interval 0 = 暂停，保留");
        assert_eq!(c.last_interval_sec, 120, "last 保留原值");
    }

    #[test]
    fn toggled_pause_to_zero_and_back() {
        let mut c = Config {
            whitelist: vec![1], interval_sec: 300,
            default_customer_group_id: None, default_customer_group_name: None,
            default_requestor_id: None, default_requestor_name: None,
            default_support_group_id: None, default_support_group_name: None,
            view_page_sizes: HashMap::new(), detail_width_pct: None,
            autostart_enabled: false, minimize_to_tray: true,
            tray_hint_shown: false, last_interval_sec: 300,
            auto_claim_enabled: false, auto_claim_seach_type: None,
            mcp_enabled: true, mcp_port: 17540,
        };
        // 暂停：记 last=300，interval->0
        c = c.toggled_pause();
        assert_eq!(c.interval_sec, 0);
        assert_eq!(c.last_interval_sec, 300);
        // 恢复：interval<-last
        c = c.toggled_pause();
        assert_eq!(c.interval_sec, 300);
    }

    #[test]
    fn default_with_auto_claim_off() {
        let c = Config::default_with(2);
        assert!(!c.auto_claim_enabled);
        assert_eq!(c.auto_claim_seach_type, None);
    }

    #[test]
    fn old_config_without_auto_claim_fields_compat() {
        // 旧 config 缺 auto_claim_* 字段，应反序列化成功并补默认
        let json = r#"{
            "whitelist":[2],
            "interval_sec":300,
            "view_page_sizes":{},
            "autostart_enabled":false,
            "minimize_to_tray":true,
            "tray_hint_shown":false,
            "last_interval_sec":300
        }"#;
        let c: Config = serde_json::from_str(json).unwrap();
        assert!(!c.auto_claim_enabled);
        assert_eq!(c.auto_claim_seach_type, None);
    }

    #[test]
    fn normalize_preserves_auto_claim() {
        let mut c = Config::default_with(2);
        c.auto_claim_enabled = true;
        c.auto_claim_seach_type = Some(5);
        let n = c.clone().normalize();
        assert!(n.auto_claim_enabled);
        assert_eq!(n.auto_claim_seach_type, Some(5));
    }

    #[test]
    fn default_with_enables_mcp_on_17540() {
        let c = Config::default_with(2);
        assert!(c.mcp_enabled);
        assert_eq!(c.mcp_port, 17540);
    }

    #[test]
    fn legacy_config_defaults_mcp_fields() {
        let legacy = r#"{"whitelist":[2],"interval_sec":300}"#;
        let c: Config = serde_json::from_str(legacy).unwrap();
        assert!(c.mcp_enabled);
        assert_eq!(c.mcp_port, 17540);
    }

    #[test]
    fn clamp_mcp_port_accepts_user_port() {
        assert_eq!(Config::clamp_mcp_port(1024), 1024);
        assert_eq!(Config::clamp_mcp_port(17541), 17541);
        assert_eq!(Config::clamp_mcp_port(65535), 65535);
    }

    #[test]
    fn clamp_mcp_port_rejects_privileged_or_zero() {
        assert_eq!(Config::clamp_mcp_port(0), 17540);
        assert_eq!(Config::clamp_mcp_port(1023), 17540);
    }

    #[test]
    fn normalize_repairs_invalid_mcp_port() {
        let mut c = Config::default_with(2);
        c.mcp_enabled = false;
        c.mcp_port = 80;
        let n = c.normalize();
        assert!(!n.mcp_enabled);
        assert_eq!(n.mcp_port, 17540);
    }
}
