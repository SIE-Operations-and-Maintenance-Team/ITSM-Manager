// 系统托盘 + 菜单：构建、事件路由、状态刷新、一次性气泡
use crate::commands::{self, SchedulerHandle};
use crate::config;
use crate::state;
use tauri::{
    AppHandle, Emitter, Manager,
    menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};

const TRAY_ID: &str = "main-tray";

/// 持有需动态刷新的菜单项引用（Tauri 2 TrayIcon 不暴露 menu getter，用 managed state）
pub struct TrayState {
    pub autostart: CheckMenuItem<tauri::Wry>,
    pub min_tray: CheckMenuItem<tauri::Wry>,
    pub pause: MenuItem<tauri::Wry>,
}

pub fn build(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let cfg = config::load(app, state::DEFAULT_SEACH_TYPE);

    let show = MenuItem::with_id(app, "show", "显示窗口", true, None::<&str>)?;
    let refresh = MenuItem::with_id(app, "refresh", "立即刷新", true, None::<&str>)?;
    let pause_label = if cfg.interval_sec == 0 { "恢复自动刷新" } else { "暂停自动刷新" };
    let pause = MenuItem::with_id(app, "pause", pause_label, true, None::<&str>)?;
    let autostart = CheckMenuItem::with_id(app, "autostart", "开机自启", true, cfg.autostart_enabled, None::<&str>)?;
    let min_tray = CheckMenuItem::with_id(app, "min_tray", "关闭进托盘", true, cfg.minimize_to_tray, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;

    let menu = Menu::with_items(app, &[
        &show, &refresh, &pause, &autostart, &min_tray, &sep, &quit,
    ])?;

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(app.default_window_icon().cloned().expect("默认窗口图标"))
        .tooltip("ITSM 管理工具")
        .menu(&menu)
        .on_menu_event(handle_menu_event)
        .on_tray_icon_event(handle_tray_event)
        .build(app)?;

    // 存菜单项引用，供 refresh_state 直接 set_checked/set_text
    app.manage(TrayState { autostart, min_tray, pause });

    Ok(())
}

fn handle_menu_event(app: &AppHandle, event: tauri::menu::MenuEvent) {
    match event.id().as_ref() {
        "show" => show_main(app),
        "refresh" => trigger_refresh_all(app),
        "pause" => toggle_pause(app),
        "autostart" => toggle_autostart(app),
        "min_tray" => toggle_min_tray(app),
        "quit" => app.exit(0),
        _ => {}
    }
}

fn handle_tray_event(tray: &tauri::tray::TrayIcon, event: TrayIconEvent) {
    if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
        let app = tray.app_handle().clone();
        show_main(&app);
    }
}

fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.set_focus();
    }
}

/// 立即刷新：restart scheduler（立即跑一轮白名单）
fn trigger_refresh_all(app: &AppHandle) {
    let cfg = config::load(app, state::DEFAULT_SEACH_TYPE);
    let fallback = cfg.whitelist.first().copied().unwrap_or(state::DEFAULT_SEACH_TYPE);
    app.state::<SchedulerHandle>().0.restart(app.clone(), fallback);
}

/// 暂停/恢复：纯函数决策 + 落盘 + emit + restart scheduler。
/// interval=0 时 scheduler loop 会退出，必须 restart 唤醒。
fn toggle_pause(app: &AppHandle) {
    let cfg = config::load(app, state::DEFAULT_SEACH_TYPE).toggled_pause();
    let fallback = cfg.whitelist.first().copied().unwrap_or(state::DEFAULT_SEACH_TYPE);
    let _ = config::save(app, &cfg);
    let _ = app.emit("config-changed", &cfg);
    app.state::<SchedulerHandle>().0.restart(app.clone(), fallback);
    refresh_state(app);
}

/// 自启勾选切换：复用 commands::apply_autostart（内部已 emit config-changed）
fn toggle_autostart(app: &AppHandle) {
    let cfg = config::load(app, state::DEFAULT_SEACH_TYPE);
    let _ = commands::apply_autostart(app, !cfg.autostart_enabled);
}

/// 关闭进托盘勾选切换：直接改 config 字段
fn toggle_min_tray(app: &AppHandle) {
    let mut cfg = config::load(app, state::DEFAULT_SEACH_TYPE);
    cfg.minimize_to_tray = !cfg.minimize_to_tray;
    let _ = config::save(app, &cfg);
    let _ = app.emit("config-changed", &cfg);
    refresh_state(app);
}

/// config 变更时刷新菜单勾选/label
pub fn refresh_state(app: &AppHandle) {
    let cfg = config::load(app, state::DEFAULT_SEACH_TYPE);
    let st = app.state::<TrayState>();
    let _ = st.autostart.set_checked(cfg.autostart_enabled);
    let _ = st.min_tray.set_checked(cfg.minimize_to_tray);
    let text = if cfg.interval_sec == 0 { "恢复自动刷新" } else { "暂停自动刷新" };
    let _ = st.pause.set_text(text);
}

/// 首次关闭进托盘气泡 + 标记已弹
pub fn show_hint_once(app: &AppHandle) {
    use tauri_plugin_notification::NotificationExt;
    let _ = app.notification().builder()
        .title("ITSM 管理工具")
        .body("已最小化到托盘，点击托盘图标可恢复窗口。")
        .show();
    let mut cfg = config::load(app, state::DEFAULT_SEACH_TYPE);
    cfg.tray_hint_shown = true;
    let _ = config::save(app, &cfg);
}
