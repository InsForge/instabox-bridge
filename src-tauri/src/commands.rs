//! The commands the UI calls. Each one parses its arguments and hands off to `Core`.
//! Errors are `ErrorView` values the UI can branch on.
//!
//! An optional `ade` argument picks the ADE (`"t3"`, `"cmux"`, `"orca"`, `"paradev"`,
//! `"paseo"`, `"herdr"`). Without it a command uses the sandbox's own ADE (the first in its mark), else
//! the default (T3); `create_sandbox` without it creates a plain sandbox, with no ADE.

use std::sync::Arc;

use tauri::{AppHandle, State};

use crate::ade::Ade;
use crate::agents::Agent;
use crate::app::{AppState, Core, CreateArgs, Notice, parse_mcp_client, parse_provider};
use crate::errors::ErrorView;
use crate::login;
use crate::old_app;
use crate::updater::Updater;

type Shared<'a> = State<'a, Arc<Core>>;
type Res<T> = Result<T, ErrorView>;

#[tauri::command]
pub fn get_state(core: Shared<'_>) -> AppState {
    core.state()
}

#[tauri::command]
pub async fn sign_in(core: Shared<'_>, provider: String) -> Res<()> {
    let Some(p) = parse_provider(&provider) else {
        return core.settle(Err(ErrorView::other("Unknown sign-in provider")));
    };
    core.inner().clone().sign_in(p).await
}

#[tauri::command]
pub async fn sign_out(core: Shared<'_>) -> Res<()> {
    core.sign_out().await
}

#[tauri::command]
pub fn switch_org(core: Shared<'_>, org_id: String) -> Res<()> {
    core.switch_org(&org_id)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn create_sandbox(
    core: Shared<'_>,
    name: String,
    snapshot_id: Option<String>,
    vcpus: u32,
    memMiB: u32,
    auto_suspend_after_seconds: Option<i64>,
    ade: Option<Ade>,
) -> Res<Notice> {
    core.inner()
        .clone()
        .create_sandbox(CreateArgs {
            name,
            snapshot_id,
            vcpus,
            mem_mib: memMiB,
            auto_suspend_after_seconds,
            ade,
        })
        .await
}

#[tauri::command]
pub async fn connect_sandbox(core: Shared<'_>, id: String, ade: Option<Ade>) -> Res<Notice> {
    core.inner().clone().connect_sandbox(&id, ade).await
}

/// Closes a failed create or connect.
#[tauri::command]
pub fn dismiss_progress(core: Shared<'_>) {
    core.dismiss_progress();
}

/// Looks for the ADEs' desktop apps (T3 Code, cmux, Orca, Paradev, Paseo, and the OpenSSH
/// client for Herdr) on this computer again, now.
#[tauri::command]
pub async fn refresh_t3(core: Shared<'_>) -> Res<()> {
    core.inner().clone().refresh_ades().await
}

/// Opens the sandbox in the ADE's app: T3 Code (a connected sandbox is woken, synced and
/// paired again), cmux (its link, with an SSH token), Orca (its SSH host, then the app; the
/// answer says what to do next in Orca), Paradev (the sandbox is woken, then its host
/// opens), Paseo (like Orca, and its address is copied) or Herdr (the sandbox is woken, then
/// a terminal runs Herdr there over SSH). A sandbox without the ADE gets it first.
#[tauri::command]
pub async fn open_in_t3(core: Shared<'_>, id: String, ade: Option<Ade>) -> Res<Notice> {
    core.inner().clone().open_in_ade(&id, ade).await
}

/// A new pairing link for a connected sandbox, opened in the ADE's app.
#[tauri::command]
pub async fn pair_again(core: Shared<'_>, id: String, ade: Option<Ade>) -> Res<()> {
    core.inner().clone().pair_again(&id, ade).await
}

/// Copies a new pairing URL of the sandbox's T3 proxy, for T3 Code's Add route (the T3 switch
/// guide). It is cleared from the clipboard a minute later.
#[tauri::command]
pub async fn copy_t3_switch_url(core: Shared<'_>, id: String) -> Res<()> {
    core.inner().clone().copy_t3_switch_url(&id).await
}

/// Closes the T3 switch guide. `done`: finished or turned off, so it is not offered again for
/// the sandbox.
#[tauri::command]
pub async fn close_t3_switch(core: Shared<'_>, id: String, done: bool) -> Res<()> {
    core.inner().clone().close_t3_switch(&id, done).await;
    Ok(())
}

#[tauri::command]
pub async fn pause(core: Shared<'_>, id: String) -> Res<()> {
    core.inner().clone().pause(&id).await
}

#[tauri::command]
pub async fn resume(core: Shared<'_>, id: String) -> Res<()> {
    core.inner().clone().resume(&id).await
}

#[tauri::command]
pub async fn delete(core: Shared<'_>, id: String) -> Res<()> {
    core.inner().clone().delete(&id).await
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn resize(core: Shared<'_>, id: String, vcpus: u32, memMiB: u32) -> Res<()> {
    core.inner().clone().resize(&id, vcpus, memMiB).await
}

#[tauri::command]
pub async fn set_auto_sleep(core: Shared<'_>, id: String, seconds: i64) -> Res<()> {
    core.inner().clone().set_auto_sleep(&id, seconds).await
}

/// Sets the ADE (T3) up again on the sandbox. cmux installs nothing there to set up again.
#[tauri::command]
pub async fn reinstall_t3(core: Shared<'_>, id: String, ade: Option<Ade>) -> Res<()> {
    core.inner().clone().reinstall(&id, ade).await
}

#[tauri::command]
pub async fn agent_sign_in(core: Shared<'_>, id: String, agent: Agent) -> Res<()> {
    core.inner().clone().agent_sign_in(&id, agent).await
}

/// Whether the agent is signed in on the sandbox. False without any exec when the
/// sandbox is not running.
#[tauri::command]
pub async fn agent_status(core: Shared<'_>, id: String, agent: Agent) -> Res<bool> {
    core.agent_status(&id, agent).await
}

#[tauri::command]
pub async fn ssh_command(core: Shared<'_>, id: String) -> Res<String> {
    core.ssh_command(&id).await
}

#[tauri::command]
pub async fn open_ssh(core: Shared<'_>, id: String) -> Res<()> {
    core.open_ssh(&id).await
}

#[tauri::command]
pub async fn add_mcp(core: Shared<'_>, client: String) -> Res<()> {
    let Some(c) = parse_mcp_client(&client) else {
        return core.settle(Err(ErrorView::other("Unknown client")));
    };
    core.add_mcp(c).await
}

#[tauri::command]
pub fn set_start_at_login(app: AppHandle, core: Shared<'_>, on: bool) -> Res<()> {
    if on {
        match login::enable(&app) {
            login::TurnOn::On => {}
            // macOS 13 and later: the person declined the app's login item before. Only they
            // can allow it again, in System Settings.
            login::TurnOn::Approval => {
                login::open_login_items();
                core.set_start_at_login(false);
                return core.settle(Err(ErrorView::other(format!(
                    "Allow {} in System Settings, under Login Items.",
                    app.package_info().name
                ))));
            }
            login::TurnOn::Failed(e) => return core.settle(Err(ErrorView::other(e))),
        }
        #[cfg(windows)]
        login::quote_windows_run_entry(&app.package_info().name);
    } else if let Err(e) = login::disable(&app) {
        return core.settle(Err(ErrorView::other(e)));
    }
    core.set_start_at_login(on);
    core.settle(Ok(()))
}

/// Move to Trash on the notice about the old app. When the app starts at login, its login item
/// may still name the old copy. So it is turned off while that copy is there to be recognized
/// (a LaunchAgent counts as ours by its bundle), and on again after the move, for this app,
/// whatever the move came to.
#[tauri::command]
pub fn trash_old_app(app: AppHandle, core: Shared<'_>) -> Res<()> {
    let Some(old) = core.state().old_app else {
        return core.settle(Ok(()));
    };
    let on = login::is_enabled(&app);
    if on {
        // When this fails, turning it on again below still says how it ended.
        let _ = login::disable(&app);
    }
    let moved = old_app::trash_offered(&old, &app.config().identifier);
    let again = if on { login_again(&app) } else { Ok(()) };
    core.set_start_at_login(login::is_enabled(&app));
    if moved.is_ok() {
        core.set_old_app(None);
    }
    core.settle(moved.map_err(ErrorView::other).and(again))
}

/// Turns start at login on again after Move to Trash, so the login item names this app.
fn login_again(app: &AppHandle) -> Res<()> {
    let name = &app.package_info().name;
    match login::enable(app) {
        login::TurnOn::On => Ok(()),
        login::TurnOn::Approval => Err(ErrorView::other(format!(
            "To start {name} at login again, allow it in System Settings, under Login Items."
        ))),
        login::TurnOn::Failed(e) => Err(ErrorView::other(format!(
            "{name} could not start at login again: {e}. Turn it on in Settings."
        ))),
    }
}

/// Not now on the notice about the old app: it is not offered again until the next launch.
#[tauri::command]
pub fn dismiss_old_app(core: Shared<'_>) {
    core.set_old_app(None);
}

#[tauri::command]
pub fn open_url(core: Shared<'_>, url: String) -> Res<()> {
    core.open_url(&url)
}

/// Check for updates: reads the update manifest now and shows what it found in
/// `state.update`. Never downloads. Async, like `install_update`, so it never runs on the main
/// thread (see `Updater`).
#[tauri::command]
pub async fn check_for_updates(updater: State<'_, Arc<Updater>>) -> Res<()> {
    updater.check_now().await
}

/// Update: downloads the update on offer, checks its signature, installs it and restarts the
/// app. A failure shows in `state.update`, with Retry.
#[tauri::command]
pub async fn install_update(updater: State<'_, Arc<Updater>>) -> Res<()> {
    updater.inner().clone().install().await
}
