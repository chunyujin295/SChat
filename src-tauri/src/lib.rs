mod commands;
mod config;
mod core;
mod crypto;
mod discovery;
mod identity;
mod store;
mod transfer;
mod transport;

pub use crypto::{b64, un_b64};

use core::SharedCore;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, RunEvent, WindowEvent};
use tauri_plugin_global_shortcut::GlobalShortcutExt;

fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

fn toggle_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let visible = w.is_visible().unwrap_or(false);
        let focused = w.is_focused().unwrap_or(false);
        if visible && focused {
            let _ = w.hide();
        } else {
            show_main(app);
        }
    } else {
        show_main(app);
    }
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "显示主窗口", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;

    let icon_path = app
        .path()
        .resource_dir()
        .ok()
        .and_then(|p| {
            let p = p.join("icons/icon.png");
            if p.exists() { Some(p) } else { None }
        })
        .or_else(|| {
            app.path()
                .app_data_dir()
                .ok()
                .map(|p| p.join("icon.png"))
        });

    let icon = icon_path
        .and_then(|p| {
            let bytes = std::fs::read(p).ok()?;
            tauri::image::Image::from_bytes(&bytes).ok()
        })
        .or_else(|| app.default_window_icon().cloned())
        .expect("no icon available");

    TrayIconBuilder::with_id("main")
        .icon(icon)
        .tooltip("SChat")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, ev| match ev.id.as_ref() {
            "show" => show_main(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, ev| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = ev
            {
                toggle_main(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

async fn keepalive(core: SharedCore) {
    let mut tick = tokio::time::interval(Duration::from_secs(10));
    loop {
        tick.tick().await;
        let now = crypto::now_ms();
        let dead: Vec<[u8; 32]> = {
            let mut map = core.sessions.lock().unwrap();
            let mut d = Vec::new();
            map.retain(|fp, s| {
                let stale = now.saturating_sub(s.last_rx_ms.load(Ordering::Relaxed)) > 25_000;
                if !s.is_alive() || stale {
                    d.push(*fp);
                    false
                } else {
                    true
                }
            });
            d
        };
        for fp in &dead {
            core.emit(
                "session",
                &serde_json::json!({"fp": crypto::hex(fp), "state": "closed"}),
            );
        }
        let sessions = core.sessions.lock().unwrap();
        for s in sessions.values() {
            s.send(transport::F_PING, b"ping".to_vec());
        }
    }
}

fn shutdown_notice(app: &AppHandle) {
    if let Some(c) = app.try_state::<SharedCore>() {
        discovery::goodbye_blocking(&c);
        {
            let sessions = c.sessions.lock().unwrap();
            for s in sessions.values() {
                s.close();
            }
        }
        std::thread::sleep(Duration::from_millis(150));
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .try_init();

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_main(app);
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, _event| {
                    toggle_main(app);
                })
                .build(),
        )
        .setup(|app| {
            let handle = app.handle().clone();
            let default_dir = handle.path().app_data_dir()?;
            std::fs::create_dir_all(&default_dir)?;

            // Load config first to check for custom data_dir
            let cfg_check = config::load_or_create(&default_dir)?;
            let dir = if let Some(ref custom) = cfg_check.read().unwrap().data_dir {
                let p = std::path::PathBuf::from(custom);
                std::fs::create_dir_all(&p).unwrap_or_else(|_| {
                    eprintln!("[SChat] Failed to create custom data_dir: {}", p.display());
                });
                p
            } else {
                default_dir
            };

            std::fs::create_dir_all(dir.join("avatars"))?;
            std::fs::create_dir_all(dir.join("files"))?;
            std::fs::create_dir_all(dir.join("outgoing"))?;

            let identity = identity::load_or_create(&dir)?;
            let key = identity::load_or_create_db_key(&dir)?;
            let db = store::Db::open(&dir.join("schat.db"), key)?;
            let cfg = config::load_or_create(&dir)?;
            let instance_id = uuid::Uuid::new_v4().to_string();

            let core_app: SharedCore = std::sync::Arc::new(core::AppCore {
                app: handle.clone(),
                dir,
                identity,
                cfg,
                db,
                peers: Default::default(),
                sessions: Default::default(),
                transfers: transfer::Transfers::new(),
                tcp_port: Default::default(),
                ava_pending: Default::default(),
                instance_id,
            });
            app.manage(core_app.clone());

            build_tray(&handle)?;

            if let Some(w) = handle.get_webview_window("main") {
                let c = core_app.clone();
                let win = w.clone();
                w.on_window_event(move |e| {
                    if let WindowEvent::CloseRequested { api, .. } = e {
                        let to_tray =
                            c.cfg.read().map(|g| g.close_to_tray).unwrap_or(true);
                        if to_tray {
                            api.prevent_close();
                            let _ = win.hide();
                        }
                    }
                });
            }

            let hk = core_app
                .cfg
                .read()
                .map(|g| g.hotkey.clone())
                .unwrap_or_default();
            if let Ok(sc) = hk.to_lowercase().parse::<tauri_plugin_global_shortcut::Shortcut>()
            {
                let _ = handle.global_shortcut().register(sc);
            }

            tauri::async_runtime::spawn(discovery::run(core_app.clone()));
            tauri::async_runtime::spawn(transport::listen(core_app.clone()));
            tauri::async_runtime::spawn(keepalive(core_app.clone()));

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_bootstrap,
            commands::list_messages,
            commands::open_session,
            commands::confirm_peer,
            commands::set_blocked,
            commands::forget_peer,
            commands::send_text,
            commands::typing,
            commands::mark_read,
            commands::send_files,
            commands::send_media,
            commands::cancel_transfer,
            commands::get_avatar,
            commands::set_profile,
            commands::set_settings,
            commands::clear_history,
            commands::reveal_path,
            commands::open_path,
            commands::quit_app,
            commands::get_data_dir,
        ])
        .build(tauri::generate_context!())
        .expect("error while building schat")
        .run(|app, event| {
            if let RunEvent::Exit = event {
                shutdown_notice(app);
            }
        });
}
