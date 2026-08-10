// IPC 命令薄封装：调 api/cache/config/scheduler
use crate::api;
use crate::cache;
use crate::config::{self, Config};
use crate::scheduler::{self, Scheduler};
use crate::state::{self, AppState};
use keyring::Entry;
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
                state.token.set(Some(c.token.clone()));
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
    state.token.set(Some(creds.token.clone()));
    *state.tenant_id.lock().unwrap() = creds.tenant_id.clone();
    *state.user_name.lock().unwrap() = creds.user_name.clone();
    if let Some(p) = state::creds_path(&app) {
        let s = serde_json::to_string_pretty(&creds).map_err(|e| e.to_string())?;
        std::fs::write(p, s).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// 登出：清凭证 + 清缓存（保留 config.json，配置与登录态解绑）
#[tauri::command]
pub fn clear_creds(app: AppHandle, state: State<AppState>) -> Result<(), String> {
    state.token.set(None);
    if let Some(p) = state::creds_path(&app) {
        let _ = std::fs::remove_file(p);
    }
    if let Some(d) = state::app_data_dir(&app) {
        // 不删 config.json：用户偏好（白名单/间隔/补单默认值/分页/托盘设置）跨登录保留，
        // 因 token 会过期、登出重登是高频操作。仅清账号相关的工单缓存。
        let _ = std::fs::remove_dir_all(d.join("cache"));
    }
    Ok(())
}

const KEYCHAIN_SERVICE: &str = "itsm-manager";
const KEYCHAIN_USER: &str = "default";

#[derive(Debug, PartialEq)]
enum MigrationAction {
    None,
    Migrate,
    DeletePlaintext,
}

/// 迁移决策纯函数（输入：明文文件存在?、keychain 存在? → 动作）
fn decide_migration(plaintext_exists: bool, keychain_exists: bool) -> MigrationAction {
    match (plaintext_exists, keychain_exists) {
        (false, _) => MigrationAction::None,
        (true, true) => MigrationAction::DeletePlaintext,
        (true, false) => MigrationAction::Migrate,
    }
}

/// 保存账密到 keychain（"记住密码"/"自动登录"勾选时）。签名不变，前端无感。
#[tauri::command]
pub fn save_stored_cred(cred: state::StoredCred, app: AppHandle) -> Result<(), String> {
    let entry = Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_USER)
        .map_err(|e| format!("安全存储不可用: {}", e))?;
    let json = serde_json::to_string(&cred).map_err(|e| e.to_string())?;
    entry.set_password(&json).map_err(|e| format!("安全存储写入失败: {}", e))?;
    // 迁移成功后删旧明文（若存在）
    if let Some(p) = state::stored_cred_path(&app) {
        let _ = std::fs::remove_file(&p);
    }
    Ok(())
}

/// 读账密（惰性迁移：keychain 无 + 明文有 → 迁移）。签名不变。
#[tauri::command]
pub fn load_stored_cred(app: AppHandle) -> Option<state::StoredCred> {
    let plaintext_exists = state::stored_cred_path(&app)
        .map(|p| p.exists())
        .unwrap_or(false);
    let keychain_cred = match Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_USER) {
        Ok(e) => e.get_password().ok().and_then(|s| serde_json::from_str(&s).ok()),
        Err(_) => None,
    };
    let keychain_exists = keychain_cred.is_some();

    match decide_migration(plaintext_exists, keychain_exists) {
        MigrationAction::None => keychain_cred,
        MigrationAction::DeletePlaintext => {
            // keychain 已有，明文是残留 → 删
            if let Some(p) = state::stored_cred_path(&app) {
                let _ = std::fs::remove_file(&p);
            }
            keychain_cred
        }
        MigrationAction::Migrate => {
            // 读明文 → 写 keychain → 删明文；写失败则降级返明文凭据（保留文件）
            let cred = state::stored_cred_path(&app).and_then(|p| {
                std::fs::read_to_string(&p)
                    .ok()
                    .and_then(|s| serde_json::from_str::<state::StoredCred>(&s).ok())
            });
            if let Some(c) = cred {
                let written = Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_USER)
                    .ok()
                    .and_then(|e| serde_json::to_string(&c).ok().and_then(|j| e.set_password(&j).ok()))
                    .is_some();
                if written {
                    if let Some(p) = state::stored_cred_path(&app) {
                        let _ = std::fs::remove_file(&p);
                    }
                }
                return Some(c);
            }
            None
        }
    }
}

/// 清账密（登出/取消记住密码）。签名不变。
#[tauri::command]
pub fn clear_stored_cred(app: AppHandle) -> Result<(), String> {
    if let Ok(entry) = Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_USER) {
        let _ = entry.delete_credential();
    }
    if let Some(p) = state::stored_cred_path(&app) {
        let _ = std::fs::remove_file(&p);
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
    api::suspend_or_unhang(&state.client, &token, &id, "suspend", &reason).await
}

#[tauri::command]
pub async fn unhang(id: String, state: State<'_, AppState>) -> Result<Value, String> {
    let token = state::get_token(&state)?;
    api::suspend_or_unhang(&state.client, &token, &id, "unhang", "").await
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
    } else if mgr.is_enabled().unwrap_or(false) {
        // 幂等：仅在当前已启用时才 disable，避免 Run 值不存在时
        // delete_value 报 "系统找不到指定的文件。(os error 2)" 阻塞保存
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

// ============================ 自动登录辅助 ============================

/// 探活当前 token：Ok(true)=有效，Ok(false)=失效(Auth)，Err=暂时性(网络/服务器)
#[tauri::command]
pub async fn verify_token(state: State<'_, AppState>) -> Result<bool, String> {
    let token = state::get_token(&state)?;
    match api::probe_token(&state.client, &token).await {
        Ok(valid) => Ok(valid),
        Err(api::RefreshError::Network) => Err("网络错误".into()),
        Err(api::RefreshError::Server) => Err("服务器错误".into()),
        Err(api::RefreshError::Auth) => Ok(false), // 防御：probe_token 内部已转 Ok(false)
    }
}

/// 是否以 --hidden 启动（开机自启）；复用 setup 同款判断表达式
#[tauri::command]
pub fn is_start_hidden() -> bool {
    std::env::args().any(|a| a == "--hidden")
}

/// 发系统通知（静默模式自动登录失败/未存账密/验证码 fallback 用）
#[tauri::command]
pub fn send_system_notification(title: String, body: String, app: AppHandle) -> Result<(), String> {
    use tauri_plugin_notification::NotificationExt;
    app.notification()
        .builder()
        .title(&title)
        .body(&body)
        .show()
        .map_err(|e| format!("通知失败: {}", e))?;
    Ok(())
}

// ============================ 关于 ============================

/// 当前应用版本号（编译期注入，与 Cargo.toml / package.json / tauri.conf.json 一致）
#[tauri::command]
pub fn get_app_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// 打开外部 http/https 链接（系统默认浏览器）。
/// 项目 bundle 仅 nsis（Windows），用 cmd start；非 Windows 返回错误。
#[tauri::command]
pub fn open_external_url(url: String) -> Result<(), String> {
    if !url.starts_with("https://") && !url.starts_with("http://") {
        return Err("仅支持 http/https 链接".into());
    }
    #[cfg(target_os = "windows")]
    {
        // start 第一段空串占位窗口标题，避免带引号的 URL 被当作标题
        std::process::Command::new("cmd")
            .args(["/C", "start", "", &url])
            .spawn()
            .map_err(|e| format!("打开链接失败：{}", e))?;
        Ok(())
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = url;
        Err("当前平台不支持打开外链".into())
    }
}

// ============================ 版本更新 ============================

/// 检查更新结果（前端据此决定是否弹更新提示）
#[derive(serde::Serialize)]
pub struct UpdateInfo {
    pub available: bool,
    pub current_version: String,
    pub version: Option<String>,
    pub notes: Option<String>,
    pub pub_date: Option<String>,
}

/// 检查更新（只检查，不下载）。endpoint 无法访问或已是最新版时 available=false。
#[tauri::command]
pub async fn check_update(app: AppHandle) -> Result<UpdateInfo, String> {
    use tauri_plugin_updater::UpdaterExt;
    let current = env!("CARGO_PKG_VERSION").to_string();
    let updater = app.updater().map_err(|e| format!("初始化 updater 失败: {e}"))?;
    match updater.check().await.map_err(|e| format!("检查更新失败: {e}"))? {
        Some(u) => Ok(UpdateInfo {
            available: true,
            current_version: current,
            version: Some(u.version.clone()),
            notes: u.body.clone(),
            pub_date: u.date.map(|d| d.to_string()),
        }),
        None => Ok(UpdateInfo {
            available: false,
            current_version: current,
            version: None,
            notes: None,
            pub_date: None,
        }),
    }
}

/// 下载并安装更新，进度通过 emit("update-progress", {downloaded, total}) 上报。
/// Windows：download_and_install 内部已退出应用执行 NSIS passive 安装，重启由安装器接管；
/// 非 Windows 路径到此处主动重启（Windows 上此行因进程已退出不会执行）。
#[tauri::command]
pub async fn download_and_install_update(app: AppHandle) -> Result<(), String> {
    use std::sync::atomic::{AtomicU64, Ordering};
    use tauri_plugin_updater::UpdaterExt;
    let updater = app.updater().map_err(|e| format!("初始化 updater 失败: {e}"))?;
    let update = updater
        .check()
        .await
        .map_err(|e| format!("检查更新失败: {e}"))?
        .ok_or_else(|| "没有可用更新".to_string())?;
    let app_for_progress = app.clone();
    let total = AtomicU64::new(0);
    let downloaded = AtomicU64::new(0);
    update
        .download_and_install(
            move |chunk_len, content_len| {
                if let Some(cl) = content_len {
                    total.store(cl, Ordering::Relaxed);
                }
                let d = downloaded.fetch_add(chunk_len as u64, Ordering::Relaxed)
                    + chunk_len as u64;
                let t = total.load(Ordering::Relaxed);
                let _ = app_for_progress
                    .emit("update-progress", json!({ "downloaded": d, "total": t }));
            },
            || {},
        )
        .await
        .map_err(|e| format!("下载/安装失败: {e}"))?;
    app.restart()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decide_migration_no_plaintext_no_keychain() {
        assert_eq!(decide_migration(false, false), MigrationAction::None);
    }

    #[test]
    fn decide_migration_no_plaintext_has_keychain() {
        assert_eq!(decide_migration(false, true), MigrationAction::None);
    }

    #[test]
    fn decide_migration_plaintext_no_keychain_migrate() {
        assert_eq!(decide_migration(true, false), MigrationAction::Migrate);
    }

    #[test]
    fn decide_migration_both_delete_plaintext() {
        assert_eq!(decide_migration(true, true), MigrationAction::DeletePlaintext);
    }
}
