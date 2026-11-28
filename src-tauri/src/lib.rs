pub mod ade;
pub mod agents;
pub mod api;
pub mod app;
pub mod auth;
pub mod clipboard;
pub mod commands;
pub mod connected;
pub mod errors;
pub mod gate;
pub mod login;
pub mod mcp;
pub mod old_app;
pub mod process;
pub mod sandbox;
pub mod ssh;
pub mod ssh_config;
pub mod t3proxy;
pub mod tray;
pub mod updater;

use std::sync::{Arc, OnceLock};

use tauri::{Emitter, Manager, WindowEvent};
#[cfg(target_os = "macos")]
use tauri_plugin_autostart::MacosLauncher;
use tauri_plugin_opener::OpenerExt;

use crate::app::{AppState, Core, Opener, api_base};
use crate::connected::FileConnectedStore;
use crate::mcp::KeyringKeyStore;
use crate::ssh::KeyringSshStore;
use crate::ssh_config::SyncedStore;
use crate::updater::{PluginSource, Updater};

/// InstaBox's bundle identifier. Instabox Preview has its own.
pub(crate) const APP_IDENTIFIER: &str = "sh.instabox.bridge";
/// The keychain service before startup sets one: Instabox's identifier.
const DEFAULT_KEYCHAIN_SERVICE: &str = APP_IDENTIFIER;
static KEYCHAIN_SERVICE: OnceLock<String> = OnceLock::new();

/// The keychain service that holds the app's secrets: the identifier the app was bundled
/// with, set at startup. InstaADE Preview has its own identifier, so it never shares a
/// sign-in with InstaADE.
pub(crate) fn keychain_service() -> &'static str {
    service_or_default(KEYCHAIN_SERVICE.get())
}

fn service_or_default(set: Option<&'static String>) -> &'static str {
    set.map_or(DEFAULT_KEYCHAIN_SERVICE, String::as_str)
}

pub fn run() {
    let context = tauri::generate_context!();
    // Before the single-instance plugin, which hands this launch to a running copy and exits:
    // a running old copy (Instabox.app) is asked to quit first, so this app takes over.
    old_app::quit_running(&context.config().identifier, !cfg!(debug_assertions));
    let app = tauri::Builder::default()
        // Single instance first: a second launch opens the running popover.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            tray::show_popover(app, None);
        }))
        .plugin(tauri_plugin_opener::init())
        // Only Rust uses it (`updater`): the popover gets no updater permissions.
        .plugin(tauri_plugin_updater::Builder::new().build())
        // The login item on Windows and before macOS 13. `login` uses SMAppService instead on
        // macOS 13 and later.
        .plugin(autostart(&context.config().identifier))
        .manage(tray::TrayMemo::default())
        .manage(tray::HiddenAt::default())
        .setup(|app| {
            // First, while the Apple event that launched the app is still current.
            let at_login = login::launched_at_login();
            // Before anything reads the keychain.
            let _ = KEYCHAIN_SERVICE.set(app.config().identifier.clone());
            let handle = app.handle().clone();
            // A menu-bar app: no Dock icon.
            #[cfg(target_os = "macos")]
            let _ = handle.set_activation_policy(tauri::ActivationPolicy::Accessory);
            // With an app, the opener runs `open -a <app> -- <url>` on macOS: only that app
            // gets the URL, whatever else handles its scheme.
            let opener: Opener = {
                let h = handle.clone();
                Arc::new(move |url: &str, app: Option<&std::path::Path>| {
                    h.opener()
                        .open_url(url, app.map(|p| p.to_string_lossy().into_owned()))
                        .map_err(|e| e.to_string())
                })
            };
            let emit = {
                let h = handle.clone();
                Arc::new(move |state: &AppState| {
                    let _ = h.emit("state", state);
                    tray::update(&h, state);
                })
            };
            let core = Arc::new(Core::new(
                api_base(std::env::var("INSTABOX_API_URL").ok()),
                Arc::new(KeyringKeyStore),
                // A new token also rewrites the sandbox's SSH host (Orca, Paseo), and a sign-out
                // removes every host.
                Arc::new(SyncedStore::new(
                    KeyringSshStore,
                    dirs::home_dir().unwrap_or_default(),
                )),
                Arc::new(FileConnectedStore::new(
                    handle
                        .path()
                        .app_config_dir()
                        .unwrap_or_default()
                        .join("connected.json"),
                )),
                opener,
                emit,
                dirs::home_dir().unwrap_or_default(),
            ));
            let release = !cfg!(debug_assertions);
            // On macOS 13 and later, a LaunchAgent an older build wrote becomes the app's own
            // login item.
            login::migrate(&handle, release);
            // macOS: a copy of the app from before it was InstaADE is offered for the Trash.
            core.set_old_app(old_app::look(&app.config().identifier, release));
            // Without a config dir there is nowhere to save the marker, so no default.
            let marker = handle
                .path()
                .app_config_dir()
                .ok()
                .map(|d| d.join(login::DEFAULT_MARKER));
            let marked = marker.as_ref().is_none_or(|m| m.exists());
            let enabled = login::is_enabled(&handle);
            match login::startup(release, marked, enabled) {
                login::Startup::Keep => {}
                login::Startup::Rewrite => {
                    if let Err(e) = login::rewrite(&handle) {
                        eprintln!("start at login: could not update the login item: {e}");
                    }
                }
                login::Startup::TurnOn => {
                    // Marked only once the item is on, so a failed attempt is tried again next
                    // launch. A person who declined the app's login item before (macOS 13 and
                    // later) keeps that choice: marked too, and System Settings stays closed.
                    // A default that cannot be marked is undone, so it never turns the item
                    // back on after the person turned it off.
                    if let Some(m) = marker.as_deref() {
                        match login::enable(&handle) {
                            login::TurnOn::On | login::TurnOn::Approval => {
                                if let Err(e) = login::mark(m) {
                                    eprintln!("start at login: could not save the default: {e}");
                                    let _ = login::disable(&handle);
                                }
                            }
                            login::TurnOn::Failed(e) => {
                                eprintln!("start at login: could not turn it on: {e}")
                            }
                        }
                    }
                }
            }
            let starts_at_login = login::is_enabled(&handle);
            #[cfg(windows)]
            if starts_at_login {
                login::quote_windows_run_entry(&handle.package_info().name);
            }
            core.set_start_at_login(starts_at_login);
            // Prototype, off unless INSTAADE_T3_PROXY is set: the T3 local proxy.
            let gate = t3proxy::Gate::from_env();
            if gate.on() {
                let source: t3proxy::hub::ClientSource = {
                    let core = Arc::downgrade(&core);
                    Arc::new(move || core.upgrade().and_then(|c| c.current_client()))
                };
                let hub = t3proxy::Hub::new(
                    handle
                        .path()
                        .app_config_dir()
                        .unwrap_or_default()
                        .join("t3-proxy"),
                    Arc::new(t3proxy::hub::Instabox::new(source)),
                    Arc::new(t3proxy::hub::KeyringTokenStore),
                    t3proxy::runtime::Tuning::default(),
                    // A T3 without DPoP gets a bearer session only with INSTAADE_T3_PROXY=any.
                    gate == t3proxy::Gate::Any,
                );
                core.set_t3_proxy(hub.clone(), gate);
                tauri::async_runtime::spawn(async move { hub.start_saved().await });
            }
            app.manage(core.clone());
            let updates = Arc::new(Updater::new(
                handle.package_info().version.clone(),
                updater::enabled(&app.config().identifier, release),
                Arc::new(PluginSource::new(handle.clone(), {
                    let c = core.clone();
                    Arc::new(move || c.on_quit())
                })),
                {
                    let c = core.clone();
                    Arc::new(move |u| c.set_update(Some(u)))
                },
                // Through the app's exit, so `on_quit` runs before the new version starts.
                {
                    let h = handle.clone();
                    Arc::new(move || h.request_restart())
                },
            ));
            updates.publish();
            app.manage(updates.clone());
            tauri::async_runtime::spawn(updates.run());
            tray::build(&handle, core.clone())?;
            // Opened by hand: show where the app lives. Started at login: stay in the menu bar.
            if !at_login {
                let h = handle.clone();
                tauri::async_runtime::spawn(async move {
                    // Give the menu bar time to lay out the icon.
                    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                    tray::show_popover(&h, None);
                });
            }

            let starter = core.clone();
            tauri::async_runtime::spawn(async move { starter.start().await });
            let h = handle.clone();
            tauri::async_runtime::spawn(core.run_loop(move || {
                h.get_webview_window(tray::MAIN_WINDOW)
                    .and_then(|w| w.is_visible().ok())
                    .unwrap_or(false)
            }));
            Ok(())
        })
        .on_window_event(|window, event| match event {
            // Closing the popover keeps the app in the menu bar.
            WindowEvent::CloseRequested { api, .. } => {
                api.prevent_close();
                let _ = window.hide();
            }
            WindowEvent::Focused(false) => tray::hide_on_blur(window),
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_state,
            commands::sign_in,
            commands::sign_out,
            commands::switch_org,
            commands::create_sandbox,
            commands::connect_sandbox,
            commands::dismiss_progress,
            commands::refresh_t3,
            commands::open_in_t3,
            commands::pair_again,
            commands::copy_t3_switch_url,
            commands::close_t3_switch,
            commands::pause,
            commands::resume,
            commands::delete,
            commands::resize,
            commands::set_auto_sleep,
            commands::reinstall_t3,
            commands::agent_sign_in,
            commands::agent_status,
            commands::ssh_command,
            commands::open_ssh,
            commands::add_mcp,
            commands::set_start_at_login,
            commands::trash_old_app,
            commands::dismiss_old_app,
            commands::open_url,
            commands::check_for_updates,
            commands::install_update,
        ])
        .build(context)
        .expect("error while building InstaADE");

    app.run(|app, event| {
        #[cfg(target_os = "macos")]
        if let tauri::RunEvent::Reopen { .. } = event {
            tray::show_popover(app, None);
        }
        // A copied SSH address whose minute is not up yet does not outlive the app, nor an
        // update's restart.
        if let tauri::RunEvent::Exit = event
            && let Some(core) = app.try_state::<Arc<Core>>()
        {
            core.on_quit();
        }
    });
}

/// The autostart plugin: the login item on Windows and before macOS 13. On macOS its
/// LaunchAgent keeps the label older releases gave it (see `login::agent_label`).
fn autostart<R: tauri::Runtime>(identifier: &str) -> tauri::plugin::TauriPlugin<R> {
    let builder = tauri_plugin_autostart::Builder::new().args([login::AT_LOGIN]);
    #[cfg(target_os = "macos")]
    let builder = builder
        .macos_launcher(MacosLauncher::LaunchAgent)
        .app_name(login::agent_label(identifier));
    #[cfg(not(target_os = "macos"))]
    let _ = identifier;
    builder.build()
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_KEYCHAIN_SERVICE, service_or_default};

    fn identifier(conf: &str) -> String {
        let v: serde_json::Value = serde_json::from_str(conf).unwrap();
        v["identifier"].as_str().unwrap().to_string()
    }

    #[test]
    fn the_keychain_service_is_the_bundle_identifier() {
        let app = identifier(include_str!("../tauri.conf.json"));
        let preview = identifier(include_str!("../tauri.preview.conf.json"));
        assert_ne!(app, preview, "a preview installs next to InstaADE");
        // Before startup sets it, the service is InstaADE's identifier.
        assert_eq!(service_or_default(None), app);
        assert_eq!(DEFAULT_KEYCHAIN_SERVICE, app);
        // InstaADE Preview sets its own identifier, so its secrets live apart.
        let set: &'static String = Box::leak(Box::new(preview.clone()));
        assert_eq!(service_or_default(Some(set)), preview);
    }
}
