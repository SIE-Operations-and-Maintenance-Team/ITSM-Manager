// ITSM 管理工具 - Rust 后端
mod api;
mod cache;
mod commands;
mod config;
mod scheduler;
mod state;
mod tray;

use state::{AppState, Creds};
use tauri::{Emitter, Listener, Manager};

/// 打开登录窗口（嵌入 ITSM 登录页，通过本地 HTTP server + image beacon 回传 token）
#[tauri::command]
async fn open_login(app: tauri::AppHandle) -> Result<(), String> {
    use tauri::webview::WebviewWindowBuilder;
    let url: tauri::Url = "https://help.chinasie.com/login?redirect=/maintenance"
        .parse()
        .map_err(|e| format!("URL 解析失败: {}", e))?;
    WebviewWindowBuilder::new(&app, "login", tauri::WebviewUrl::External(url))
        .title("ITSM 登录")
        .inner_size(1000.0, 720.0)
        .build()
        .map_err(|e| format!("打开登录窗口失败: {}", e))?;

    const PORT: u16 = 17539;
    let app_srv = app.clone();
    std::thread::spawn(move || {
        use std::io::{Read, Write};
        let listener = match std::net::TcpListener::bind(("127.0.0.1", PORT)) {
            Ok(l) => l,
            Err(e) => {
                println!("[login-srv] 绑定端口 {} 失败: {}", PORT, e);
                return;
            }
        };
        for stream in listener.incoming() {
            let mut s = match stream {
                Ok(s) => s,
                Err(_) => continue,
            };
            let mut buf = [0u8; 8192];
            let n = s.read(&mut buf).unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]);
            let first_line = req.lines().next().unwrap_or("");
            let resp = b"HTTP/1.1 200 OK\r\nAccess-Control-Allow-Origin:*\r\nContent-Type:image/gif\r\nContent-Length:43\r\nCache-Control:no-store\r\n\r\nGIF89a\x01\x00\x01\x00\x80\x00\x00\xff\xff\xff\x00\x00\x00!\xf9\x04\x01\x00\x00\x00\x00,\x00\x00\x00\x00\x01\x00\x01\x00\x00\x02\x02\x44\x01\x00\x3b";
            let _ = s.write_all(resp);
            let path = first_line.split_whitespace().nth(1).unwrap_or("");
            if path.starts_with("/cb") {
                let q = path.split_once('?').map(|(_, q)| q).unwrap_or("");
                let mut t = String::new();
                let mut u = String::new();
                let mut ti = String::new();
                for pair in q.split('&') {
                    if let Some((k, v)) = pair.split_once('=') {
                        let val = url_decode(v);
                        match k {
                            "t" => t = val,
                            "u" => u = val,
                            "ti" => ti = val,
                            _ => {}
                        }
                    }
                }
                let token = t.trim_matches('"').to_string();
                if token.len() > 20 {
                    let creds = Creds {
                        token,
                        tenant_id: if ti.is_empty() { state::DEFAULT_TENANT.into() } else { ti },
                        user_name: u,
                    };
                    save_creds_internal(&app_srv, creds.clone());
                    let _ = app_srv.emit("login-success", creds);
                    if let Some(w) = app_srv.get_webview_window("login") {
                        let _ = w.close();
                    }
                    break;
                }
            }
        }
    });

    let app_poll = app.clone();
    std::thread::spawn(move || {
        let beacon = "try{var t=localStorage.getItem('GuShen_Token')||'';var u=localStorage.getItem('userFullName')||'';var ti=localStorage.getItem('tenantId')||'';if(t.length>20&&!window.__tokenSent){window.__tokenSent=true;new Image().src='http://127.0.0.1:17539/cb?t='+encodeURIComponent(t)+'&u='+encodeURIComponent(u)+'&ti='+encodeURIComponent(ti);}}catch(e){}";
        loop {
            std::thread::sleep(std::time::Duration::from_secs(2));
            let win = match app_poll.get_webview_window("login") {
                Some(w) => w,
                None => break,
            };
            let path = win.url().map(|u| u.as_str().to_string()).unwrap_or_default();
            if path.contains("/maintenance")
                || path.contains("/portal")
                || path.contains("/SelfServiceCenter")
            {
                let _ = win.eval(beacon);
            }
        }
    });

    Ok(())
}

fn url_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(hex) = std::str::from_utf8(&b[i + 1..i + 3]) {
                if let Ok(byte) = u8::from_str_radix(hex, 16) {
                    out.push(byte);
                    i += 3;
                    continue;
                }
            }
        }
        if b[i] == b'+' {
            out.push(b' ');
        } else {
            out.push(b[i]);
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn save_creds_internal(app: &tauri::AppHandle, creds: Creds) {
    let st = app.state::<AppState>();
    *st.token.lock().unwrap() = Some(creds.token.clone());
    *st.tenant_id.lock().unwrap() = creds.tenant_id.clone();
    *st.user_name.lock().unwrap() = creds.user_name.clone();
    if let Some(p) = state::creds_path(app) {
        if let Ok(s) = serde_json::to_string_pretty(&creds) {
            let _ = std::fs::write(p, s);
        }
    }
    // 重登后重启 scheduler（若之前因 Auth 退出，这里唤醒）
    app.state::<commands::SchedulerHandle>()
        .0
        .restart(app.clone(), state::DEFAULT_SEACH_TYPE);
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--hidden"]),
        ))
        .plugin(tauri_plugin_notification::init())
        .manage(AppState {
            token: std::sync::Mutex::new(None),
            tenant_id: std::sync::Mutex::new(state::DEFAULT_TENANT.into()),
            user_name: std::sync::Mutex::new(String::new()),
            client: reqwest::Client::builder()
                .user_agent("itsm-manager/0.1")
                .build()
                .expect("reqwest client"),
            current_pages: std::sync::Mutex::new(std::collections::HashMap::new()),
        })
        .manage(commands::SchedulerHandle(scheduler::Scheduler::new()))
        .setup(|app| {
            let handle = app.handle().clone();
            app.state::<commands::SchedulerHandle>()
                .0
                .restart(handle, state::DEFAULT_SEACH_TYPE);

            tray::build(app.handle())?;

            // 启动隐藏判断：autostart 注册时带 --hidden；命中则不 show 主窗口
            let start_hidden = std::env::args().any(|a| a == "--hidden");
            if !start_hidden {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.show();
                }
            }

            // config 变更 → 刷新托盘勾选/label
            let h = app.handle().clone();
            app.handle().listen("config-changed", move |_| {
                tray::refresh_state(&h);
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::ping,
            commands::get_creds,
            commands::save_creds,
            commands::clear_creds,
            commands::list_views,
            commands::list_tickets,
            commands::list_tickets_cached,
            commands::set_current_page,
            commands::get_detail,
            commands::list_replies,
            commands::claim,
            commands::reply,
            commands::resolve,
            commands::suspend,
            commands::list_service_tree,
            commands::get_replenish_template,
            commands::get_dict,
            commands::list_support_groups,
            commands::list_support_members,
            commands::search_customer_groups,
            commands::search_base_persons,
            commands::save_replenish,
            commands::reassign,
            commands::cancel_incident,
            commands::close_incident,
            commands::trigger_refresh,
            commands::invalidate_after_write,
            commands::get_config,
            commands::save_config,
            commands::set_autostart,
            commands::upload_attachment,
            commands::save_detail_width,
            open_login,
        ])
        .on_window_event(|window, event| {
            if window.label() != "main" {
                return;
            }
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let app = window.app_handle();
                let cfg = config::load(app, state::DEFAULT_SEACH_TYPE);
                if cfg.minimize_to_tray {
                    api.prevent_close();
                    let _ = window.hide();
                    if !cfg.tray_hint_shown {
                        tray::show_hint_once(app);
                    }
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
