// IPC 命令薄封装：调 api/cache/config/scheduler
use crate::api;
use crate::cache;
use crate::config::{self, Config};
use crate::scheduler::{self, Scheduler};
use crate::state::{self, AppState};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager, State};

/// Tauri managed state，持有 scheduler 句柄
pub struct SchedulerHandle(pub Scheduler);

#[tauri::command]
pub fn ping() -> &'static str {
    "pong"
}

#[tauri::command]
pub fn get_creds(app: AppHandle, state: State<AppState>) -> Option<state::Creds> {
    if let Some(p) = state::creds_path(&app) {
        if let Ok(s) = std::fs::read_to_string(&p) {
            if let Ok(c) = serde_json::from_str::<state::Creds>(&s) {
                *state.token.lock().unwrap() = Some(c.token.clone());
                *state.tenant_id.lock().unwrap() = c.tenant_id.clone();
                *state.user_name.lock().unwrap() = c.user_name.clone();
                return Some(c);
            }
        }
    }
    None
}

#[tauri::command]
pub fn save_creds(creds: state::Creds, app: AppHandle, state: State<AppState>) -> Result<(), String> {
    *state.token.lock().unwrap() = Some(creds.token.clone());
    *state.tenant_id.lock().unwrap() = creds.tenant_id.clone();
    *state.user_name.lock().unwrap() = creds.user_name.clone();
    if let Some(p) = state::creds_path(&app) {
        let s = serde_json::to_string_pretty(&creds).map_err(|e| e.to_string())?;
        std::fs::write(p, s).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// 登出：清凭证 + 清缓存 + 清配置（下个用户干净启动）
#[tauri::command]
pub fn clear_creds(app: AppHandle, state: State<AppState>) -> Result<(), String> {
    *state.token.lock().unwrap() = None;
    if let Some(p) = state::creds_path(&app) {
        let _ = std::fs::remove_file(p);
    }
    if let Some(d) = state::app_data_dir(&app) {
        let _ = std::fs::remove_file(d.join("config.json"));
        let _ = std::fs::remove_dir_all(d.join("cache"));
    }
    Ok(())
}

/// 保存账密（"记住密码"勾选时，明文存 app_data_dir/stored-cred.json，后续应加密）
#[tauri::command]
pub fn save_stored_cred(cred: state::StoredCred, app: AppHandle) -> Result<(), String> {
    if let Some(p) = state::stored_cred_path(&app) {
        let s = serde_json::to_string_pretty(&cred).map_err(|e| e.to_string())?;
        state::atomic_write(&p, &s)?;
    }
    Ok(())
}

#[tauri::command]
pub fn load_stored_cred(app: AppHandle) -> Option<state::StoredCred> {
    state::stored_cred_path(&app).and_then(|p| {
        std::fs::read_to_string(&p)
            .ok()
            .and_then(|s| serde_json::from_str::<state::StoredCred>(&s).ok())
    })
}

#[tauri::command]
pub fn clear_stored_cred(app: AppHandle) -> Result<(), String> {
    if let Some(p) = state::stored_cred_path(&app) {
        let _ = std::fs::remove_file(p);
    }
    Ok(())
}

#[tauri::command]
pub async fn list_views(state: State<'_, AppState>) -> Result<Value, String> {
    let token = state::get_token(&state)?;
    api::list_views(&state.client, &token).await
}

#[tauri::command]
pub async fn list_tickets(seach_type: i64, state: State<'_, AppState>) -> Result<Value, String> {
    let token = state::get_token(&state)?;
    api::list_tickets(&state.client, &token, seach_type, None).await
}

/// 读列表（分页）：缓存命中且页码/页大小匹配则返缓存；否则实时拉对应页并落盘。
/// search 非空（搜索态）：跳过读写缓存，直拉后端，结果标记 search=true。
#[tauri::command]
pub async fn list_tickets_cached(
    seach_type: i64,
    page_index: i64,
    page_size: i64,
    search: Option<api::SearchParams>,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<Value, String> {
    let has_search = search.as_ref().map_or(false, |s| !s.is_empty());
    if !has_search {
        if let Some(c) = cache::read_tickets(&app, seach_type, page_index, page_size) {
            return Ok(json!({
                "from_cache": true,
                "fetched_at": c.fetched_at,
                "count": c.count,
                "page_index": c.page_index,
                "page_size": c.page_size,
                "data": c.data,
                "search": false,
            }));
        }
    }
    let token = state::get_token(&state)?;
    let (data, count) = api::fetch_tickets_raw(&state.client, &token, seach_type, page_index, page_size, search.as_ref())
        .await
        .map_err(|e| format!("加载失败: {:?}", e))?;
    let now = state::now_unix();
    // 仅默认列表（非搜索态）落盘缓存
    if !has_search {
        cache::write_tickets(&app, seach_type, page_index, page_size, &data, count, now).ok();
    }
    Ok(json!({
        "from_cache": false,
        "fetched_at": now,
        "count": count,
        "page_index": page_index,
        "page_size": page_size,
        "data": data,
        "search": has_search,
    }))
}

/// 前端上报当前页码（scheduler 据此刷新用户正在看的页）
#[tauri::command]
pub fn set_current_page(seach_type: i64, page_index: i64, state: State<'_, AppState>) {
    state.current_pages.lock().unwrap().insert(seach_type, page_index);
}

#[tauri::command]
pub async fn get_detail(id: String, state: State<'_, AppState>) -> Result<Value, String> {
    let token = state::get_token(&state)?;
    api::get_detail(&state.client, &token, &id).await
}

#[tauri::command]
pub async fn list_replies(incident_id: String, state: State<'_, AppState>) -> Result<Value, String> {
    let token = state::get_token(&state)?;
    api::list_replies(&state.client, &token, &incident_id).await
}

#[tauri::command]
pub async fn claim(id: String, state: State<'_, AppState>) -> Result<Value, String> {
    let token = state::get_token(&state)?;
    api::claim(&state.client, &token, &id).await
}

#[tauri::command]
pub async fn reply(
    order_id: String,
    detail: String,
    file_ids: Vec<String>,
    is_private: bool,
    order_type: String,
    state: State<'_, AppState>,
) -> Result<Value, String> {
    let token = state::get_token(&state)?;
    api::reply(&state.client, &token, &order_id, &detail, &file_ids, is_private, &order_type).await
}

#[tauri::command]
pub async fn resolve(id: String, solution: String, state: State<'_, AppState>) -> Result<Value, String> {
    let token = state::get_token(&state)?;
    api::change_status(&state.client, &token, &id, "Resolved", &solution, true).await
}

#[tauri::command]
pub async fn suspend(id: String, reason: String, state: State<'_, AppState>) -> Result<Value, String> {
    let token = state::get_token(&state)?;
    api::change_status(&state.client, &token, &id, "Suspend", &reason, false).await
}

// ============ 补单 / 转派 / 取消 / 关闭 ============

#[tauri::command]
pub async fn list_service_tree(state: State<'_, AppState>) -> Result<Value, String> {
    let token = state::get_token(&state)?;
    api::list_service_tree(&state.client, &token).await
}

#[tauri::command]
pub async fn get_replenish_template(leaf_id: String, state: State<'_, AppState>) -> Result<Value, String> {
    let token = state::get_token(&state)?;
    api::get_replenish_template(&state.client, &token, &leaf_id).await
}

#[tauri::command]
pub async fn get_dict(dic_type: String, state: State<'_, AppState>) -> Result<Value, String> {
    let token = state::get_token(&state)?;
    let tenant_id = state.tenant_id.lock().unwrap().clone();
    api::get_dict(&state.client, &token, &tenant_id, &dic_type).await
}

#[tauri::command]
pub async fn list_support_groups(state: State<'_, AppState>) -> Result<Value, String> {
    let token = state::get_token(&state)?;
    api::list_support_groups(&state.client, &token).await
}

#[tauri::command]
pub async fn list_support_members(state: State<'_, AppState>) -> Result<Value, String> {
    let token = state::get_token(&state)?;
    api::list_support_members(&state.client, &token).await
}

#[tauri::command]
pub async fn search_customer_groups(keyword: String, state: State<'_, AppState>) -> Result<Value, String> {
    let token = state::get_token(&state)?;
    api::search_customer_groups(&state.client, &token, &keyword).await
}

#[tauri::command]
pub async fn search_base_persons(keyword: String, state: State<'_, AppState>) -> Result<Value, String> {
    let token = state::get_token(&state)?;
    api::search_base_persons(&state.client, &token, &keyword).await
}

#[tauri::command]
pub async fn save_replenish(params: Value, state: State<'_, AppState>) -> Result<Value, String> {
    let token = state::get_token(&state)?;
    api::save_replenish(&state.client, &token, params).await
}

#[tauri::command]
pub async fn reassign(params: Value, state: State<'_, AppState>) -> Result<Value, String> {
    let token = state::get_token(&state)?;
    api::reassign(&state.client, &token, params).await
}

#[tauri::command]
pub async fn cancel_incident(id: String, reason: String, state: State<'_, AppState>) -> Result<Value, String> {
    let token = state::get_token(&state)?;
    api::cancel_incident(&state.client, &token, &id, &reason).await
}

#[tauri::command]
pub async fn close_incident(id: String, state: State<'_, AppState>) -> Result<Value, String> {
    let token = state::get_token(&state)?;
    api::close_incident(&state.client, &token, &id).await
}

/// 手动刷新：Some(st) 单视图；None 全白名单（restart loop）
#[tauri::command]
pub async fn trigger_refresh(seach_type: Option<i64>, app: AppHandle) -> Result<(), String> {
    if let Some(st) = seach_type {
        scheduler::refresh_single(&app, st).await;
    } else {
        let cfg = config::load(&app, state::DEFAULT_SEACH_TYPE);
        let fallback = cfg.whitelist.first().copied().unwrap_or(state::DEFAULT_SEACH_TYPE);
        app.state::<SchedulerHandle>().0.restart(app.clone(), fallback);
    }
    Ok(())
}

/// 写操作成功后：立即刷新当前视图 + 删白名单缓存 + restart loop 刷其他视图
#[tauri::command]
pub async fn invalidate_after_write(seach_type: i64, app: AppHandle) -> Result<(), String> {
    // 先强制刷新用户操作的当前视图（无论是否在白名单），保证看到变化
    scheduler::refresh_single(&app, seach_type).await;
    // 再失效白名单缓存 + restart loop 让其他视图也新鲜
    let cfg = config::load(&app, state::DEFAULT_SEACH_TYPE);
    cache::invalidate_all_whitelist(&app, &cfg.whitelist);
    let fallback = cfg.whitelist.first().copied().unwrap_or(state::DEFAULT_SEACH_TYPE);
    app.state::<SchedulerHandle>().0.restart(app.clone(), fallback);
    Ok(())
}

#[tauri::command]
pub fn get_config(seach_type: i64, app: AppHandle) -> Result<Config, String> {
    Ok(config::load(&app, seach_type))
}

#[tauri::command]
pub fn save_config(mut config: Config, app: AppHandle) -> Result<(), String> {
    // interval > 0 时同步最近有效间隔，供托盘"恢复"取值
    if config.interval_sec > 0 {
        config.last_interval_sec = config.interval_sec;
    }
    let fallback = config.whitelist.first().copied().unwrap_or(state::DEFAULT_SEACH_TYPE);
    config::save(&app, &config)?;
    let _ = app.emit("config-changed", &config);
    app.state::<SchedulerHandle>().0.restart(app.clone(), fallback);
    Ok(())
}

/// 上传回复/解决/补单的图片附件：前端传 base64，解码后 multipart 上传 ITSM
#[tauri::command]
pub async fn upload_attachment(
    file_name: String,
    mime: String,
    file_base64: String,
    state: State<'_, AppState>,
) -> Result<api::UploadResult, String> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&file_base64)
        .map_err(|e| format!("base64 解码失败: {}", e))?;
    let token = state::get_token(&state)?;
    api::upload_attachment(&state.client, &token, bytes, &file_name, &mime).await
}

/// 持久化详情面板宽度百分比（仅写 config.json，不重启 scheduler）
#[tauri::command]
pub fn save_detail_width(pct: f64, app: AppHandle) -> Result<(), String> {
    let mut cfg = config::load(&app, state::DEFAULT_SEACH_TYPE);
    cfg.detail_width_pct = Some(pct);
    config::save(&app, &cfg)
}

/// 切换开机自启：plugin 改注册表 + 落 config + emit config-changed。
/// 公共函数，IPC 命令与托盘菜单共用。
pub fn apply_autostart(app: &AppHandle, enabled: bool) -> Result<(), String> {
    use tauri_plugin_autostart::ManagerExt;
    let mgr = app.autolaunch();
    if enabled {
        mgr.enable().map_err(|e| format!("启用自启失败: {}", e))?;
    } else {
        mgr.disable().map_err(|e| format!("禁用自启失败: {}", e))?;
    }
    let mut cfg = config::load(app, state::DEFAULT_SEACH_TYPE);
    cfg.autostart_enabled = enabled;
    config::save(app, &cfg)?;
    let _ = app.emit("config-changed", &cfg);
    Ok(())
}

/// IPC 包装
#[tauri::command]
pub fn set_autostart(enabled: bool, app: AppHandle) -> Result<(), String> {
    apply_autostart(&app, enabled)
}
