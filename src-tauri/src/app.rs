//! Process wiring: window that hides instead of closing, tray, single instance, and the two background loops.
use crate::{
    api::SystemClock,
    capture::SystemCapturer,
    commands::{self, lock, Shared},
    config::Paths,
    core::{Core, Deps},
    elevation,
    layers::{FirewallLayer, HostsLayer, Layer, SysinfoKiller},
    hello::Verifier,
    session,
    telegram::{self, Bot, HttpBot, Hub},
    watchdog::{Restarter, TaskRestarter},
};
use std::{
    sync::{atomic::Ordering, Arc, Mutex},
    thread,
    time::Duration,
};
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager, RunEvent, WindowEvent,
};

const TICK: Duration = Duration::from_secs(5);
const SESSION_POLL: Duration = Duration::from_secs(1);

fn verifier() -> Arc<dyn Verifier> {
    #[cfg(windows)]
    return Arc::new(crate::hello::WindowsHello);
    #[cfg(not(windows))]
    Arc::new(crate::hello::NoHello)
}

struct Booted {
    core: Core,
    hub: Hub,
    bot: Arc<dyn Bot>,
}

fn build_core(hello_available: bool) -> Result<Booted, String> {
    let paths = Paths::resolve();
    // The Telegram settings sit next to the config, so the directory must exist before the core prepares it.
    std::fs::create_dir_all(&paths.data).map_err(|e| e.to_string())?;
    let hub = Hub::load(paths.telegram()).unwrap_or_else(|why| {
        eprintln!("porteiro: telegram.json: {why}");
        Hub::detached(paths.telegram(), &why)
    });
    let bot: Arc<dyn Bot> = Arc::new(HttpBot::new(telegram::api_base(|k| std::env::var_os(k), cfg!(debug_assertions))));
    let outbox = telegram::spawn_outbox(bot.clone(), hub.clone(), paths.shots(), paths.outbox());
    let mut layers: Vec<Box<dyn Layer>> = Vec::new();
    // A sandboxed run (PORTEIRO_DATA_DIR / PORTEIRO_HOSTS_FILE) only ever touches its redirected files:
    // the firewall and the registry are real system state and are left out.
    if !paths.sandboxed {
        layers.push(Box::new(FirewallLayer::new(paths.ips())));
    }
    layers.push(Box::new(HostsLayer { path: paths.hosts.clone(), sandboxed: paths.hosts_overridden }));
    #[cfg(windows)]
    if !paths.sandboxed {
        layers.push(Box::new(crate::layers::PolicyLayer(winreg::RegKey::predef(winreg::enums::HKEY_LOCAL_MACHINE))));
    }
    let manage_watchdog = !cfg!(debug_assertions) && !paths.sandboxed;
    let core = Core::new(Deps {
        capturer: Box::new(SystemCapturer),
        killer: Box::new(SysinfoKiller),
        clock: Box::new(SystemClock::new()),
        elevated: elevation::is_elevated(),
        hello_available,
        restarter: manage_watchdog.then(|| Box::new(TaskRestarter) as Box<dyn Restarter>),
        layers,
        paths,
        telegram: hub.clone(),
        notifier: Some(telegram::notifier(hub.clone(), outbox)),
    })?;
    Ok(Booted { core, hub, bot })
}

fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

fn spawn_loops(core: Shared) {
    let enforcing = core.clone();
    thread::spawn(move || loop {
        thread::sleep(TICK);
        lock(&enforcing).tick();
    });
    thread::spawn(move || {
        let mut previous = None;
        loop {
            thread::sleep(SESSION_POLL);
            // An unreadable session is skipped, never read as "unlocked".
            if let Some(now) = session::poll() {
                if let Some(t) = session::transition(previous, now) {
                    lock(&core).session_event(t);
                }
                previous = Some(now);
            }
        }
    });
}

pub fn run() {
    let from_watchdog = std::env::args().any(|a| a == "--watchdog");
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            // The restart task launches us every minute; only a human launch should raise the window.
            if !args.iter().any(|a| a == "--watchdog") {
                show_main(app);
            }
        }))
        .setup(move |app| {
            let hello = verifier();
            let Booted { core, hub, bot } = build_core(hello.available())?;
            let core: Shared = Arc::new(Mutex::new(core));
            app.manage(hello);
            app.manage(bot.clone());
            app.manage(core.clone());
            telegram::spawn_poller(hub, bot, Arc::new(core.clone()));
            spawn_loops(core);

            let open = MenuItem::with_id(app, "open", "Abrir Porteiro", true, None::<&str>)?;
            TrayIconBuilder::new()
                .icon(app.default_window_icon().expect("bundled icon").clone())
                .tooltip("Porteiro")
                .menu(&Menu::with_items(app, &[&open])?)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| {
                    if event.id() == "open" {
                        show_main(app);
                    }
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                        show_main(tray.app_handle());
                    }
                })
                .build(app)?;
            if !from_watchdog {
                show_main(app.handle());
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            // The close button hides the window; there is no way to end the app without the password.
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_status,
            commands::setup_password,
            commands::set_hello,
            commands::unlock,
            commands::relock,
            commands::list_events,
            commands::get_shot,
            commands::add_domain,
            commands::remove_domain,
            commands::set_service,
            commands::set_unlock_minutes,
            commands::change_password,
            commands::disable_and_quit,
            commands::telegram_status,
            commands::telegram_connect,
            commands::telegram_pair,
            commands::telegram_remove_chat,
            commands::telegram_set_alerts,
            commands::telegram_disconnect,
        ])
        .build(tauri::generate_context!())
        .expect("failed to build the Porteiro app");

    app.run(|_app, event| {
        if let RunEvent::ExitRequested { api, .. } = event {
            if !crate::QUIT_AUTHORIZED.load(Ordering::SeqCst) {
                api.prevent_exit();
            }
        }
    });
}
