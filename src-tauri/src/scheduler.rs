// 后台周期刷新任务：按白名单拉列表 → 落盘 → emit
use crate::api::{self, RefreshError};
use crate::config::Config;
use crate::state::{self, AppState};
use serde_json::json;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager};
use tauri::async_runtime::JoinHandle;

pub fn should_alert(consec_fail: u32) -> bool {
    consec_fail >= 3
}

pub struct Scheduler {
    handle: Mutex<Option<JoinHandle<()>>>,
}

impl Scheduler {
    pub fn new() -> Self {
        Self { handle: Mutex::new(None) }
    }

    /// abort 旧任务 + spawn 新一轮 loop（立即跑一轮再 sleep）
    pub fn restart(&self, app: AppHandle, fallback_st: i64) {
        let mut guard = self.handle.lock().unwrap();
        if let Some(h) = guard.take() {
            h.abort();
        }
        let h = tauri::async_runtime::spawn(run_loop(app, fallback_st));
        *guard = Some(h);
    }
}

/// 刷新单个视图：拉 → 落盘 → emit tickets-updated。返回错误（若有）供 loop 计数
pub async fn refresh_single(app: &AppHandle, seach_type: i64) -> Option<RefreshError> {
    let token = match get_token_from_app(app) {
        Ok(t) => t,
        Err(_) => {
            let _ = app.emit("need-login", ());
            return Some(RefreshError::Auth);
        }
    };
    let st = app.state::<AppState>();
    let client = st.client.clone();
    let page_index = st.current_pages.lock().unwrap().get(&seach_type).copied().unwrap_or(1);
    drop(st);
    let cfg = crate::config::load(app, seach_type);
    let page_size = cfg.page_size_for(seach_type);
    match api::fetch_tickets_raw(&client, &token, seach_type, page_index, page_size, None).await {
        Ok((data, count)) => {
            let now = state::now_unix();
            crate::cache::write_tickets(app, seach_type, page_index, page_size, &data, count, now).ok();
            let _ = app.emit(
                "tickets-updated",
                json!({"seachType": seach_type, "data": data, "count": count, "fetched_at": now, "page_index": page_index, "page_size": page_size}),
            );
            None
        }
        Err(e) => {
            let r = api::map_fetch_error(&e);
            if r == RefreshError::Auth {
                let _ = app.emit("need-login", ());
            }
            Some(r)
        }
    }
}

async fn run_loop(app: AppHandle, fallback_st: i64) {
    let mut consec_fail: u32 = 0;
    loop {
        let cfg: Config = crate::config::load(&app, fallback_st);
        for st in cfg.whitelist.clone() {
            match refresh_single(&app, st).await {
                None => consec_fail = 0,
                Some(RefreshError::Auth) => return,
                Some(RefreshError::Network) => consec_fail = 0,
                Some(RefreshError::Server) => {
                    consec_fail += 1;
                    if should_alert(consec_fail) {
                        let _ = app.emit("refresh-failed", json!({"seachType": st}));
                        consec_fail = 0;
                    }
                }
            }
        }
        if cfg.interval_sec == 0 {
            // interval=0：退出 loop。靠 save_config/trigger_refresh 的 restart 唤醒下一轮
            return;
        }
        tokio::time::sleep(std::time::Duration::from_secs(cfg.interval_sec)).await;
    }
}

fn get_token_from_app(app: &AppHandle) -> Result<String, ()> {
    app.state::<AppState>().token.lock().unwrap().clone().ok_or(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_alert_below_threshold() {
        assert!(!should_alert(0));
        assert!(!should_alert(2));
    }

    #[test]
    fn should_alert_at_and_above_threshold() {
        assert!(should_alert(3));
        assert!(should_alert(5));
    }
}
