//! The menu-bar tray. A left click opens the popover window under the icon; a right click
//! shows a menu with one item per sandbox, then Open and Quit.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, LogicalPosition, Manager, Rect, WebviewWindow, Window};

use crate::ade::{Ade, Open};
use crate::app::{AppState, Core};
use crate::sandbox::view::{SandboxView, ViewState};

const TRAY_ID: &str = "main";
pub const MAIN_WINDOW: &str = "main";

/// One sandbox in the tray menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrayRow {
    pub id: String,
    pub label: String,
    pub connected: bool,
    /// The ADE its Open item opens.
    pub ade: Ade,
    pub action: Option<TrayAction>,
}

/// The Pause or Resume item of a row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayAction {
    Pause,
    Resume,
}

/// The row title: a filled dot for a sandbox that is up or changing, an empty one for a
/// paused or stopped sandbox.
pub fn row_label(name: &str, state: ViewState) -> String {
    let dot = match state {
        ViewState::Paused | ViewState::Stopped => '\u{25CB}',
        _ => '\u{25CF}',
    };
    format!("{dot} {name}")
}

/// The submenu item that opens the ADE's app. An unconnected sandbox is connected first.
pub fn open_label(ade: Ade, connected: bool) -> String {
    if connected {
        format!("Open in {}", ade.name())
    } else {
        format!("Connect to {}", ade.name())
    }
}

/// The ADE a row's Open item opens: the first the sandbox is marked with, else the default.
/// It is ready to open when the sandbox has it, and for a pairing ADE (T3) when this computer
/// connected it; else the item connects it first.
fn open_target(s: &SandboxView) -> (Ade, bool) {
    let ade = s.ades.first().copied().unwrap_or_default();
    let ready = s.ades.contains(&ade)
        && match ade.open() {
            Open::Pair(_) => s.connected,
            Open::Link(_) => true,
        };
    (ade, ready)
}

pub fn tray_rows(state: &AppState) -> Vec<TrayRow> {
    state
        .sandboxes
        .iter()
        .map(|s| TrayRow {
            id: s.id.clone(),
            label: row_label(&s.name, s.state),
            connected: open_target(s).1,
            ade: open_target(s).0,
            action: match s.state {
                ViewState::Running => Some(TrayAction::Pause),
                ViewState::Paused | ViewState::Stopped => Some(TrayAction::Resume),
                _ => None,
            },
        })
        .collect()
}

/// What the menu shows now, so an unchanged menu is not rebuilt on every refresh.
#[derive(Default)]
pub struct TrayMemo(Mutex<Option<Vec<TrayRow>>>);

/// When the popover last hid because it lost focus.
#[derive(Default)]
pub struct HiddenAt(Mutex<Option<Instant>>);

/// A rectangle in logical points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bounds {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// Space between the popover and the icon, and between the popover and the screen edge.
const GAP: f64 = 4.0;
const EDGE: f64 = 8.0;

/// The popover's top-left corner: centered on the icon, kept on the icon's screen. It opens
/// under an icon in the top half of the screen (the macOS menu bar) and above one in the
/// bottom half (a Windows taskbar).
pub fn popover_origin(icon: Bounds, window: (f64, f64), screen: Bounds) -> (f64, f64) {
    let min = screen.x + EDGE;
    let max = screen.x + screen.w - window.0 - EDGE;
    let x = (icon.x + icon.w / 2.0 - window.0 / 2.0)
        .max(min)
        .min(max.max(min));
    let y = if icon.y + icon.h / 2.0 < screen.y + screen.h / 2.0 {
        (icon.y + icon.h + GAP).min((screen.y + screen.h - window.1 - EDGE).max(screen.y + EDGE))
    } else {
        (icon.y - window.1 - GAP).max(screen.y + EDGE)
    };
    (x, y)
}

/// The popover's top-left corner when the icon's place is unknown: the bottom-right corner of
/// the work area, by the Windows notification area.
pub fn corner_origin(window: (f64, f64), work: Bounds) -> (f64, f64) {
    (
        (work.x + work.w - window.0 - EDGE).max(work.x),
        (work.y + work.h - window.1 - EDGE).max(work.y),
    )
}

/// A click on the icon also takes focus from an open popover, which hides it first. Without
/// this check the same click would open it again.
pub fn hid_just_now(last: Option<Instant>, now: Instant) -> bool {
    last.is_some_and(|t| now.duration_since(t) < Duration::from_millis(300))
}

fn logical_size(window: &WebviewWindow) -> tauri::Result<(f64, f64)> {
    let win = window
        .outer_size()?
        .to_logical::<f64>(window.scale_factor()?);
    Ok((win.width, win.height))
}

/// Places the popover by the icon. False when the icon has no place yet.
fn place(app: &AppHandle, window: &WebviewWindow, icon: Rect) -> tauri::Result<bool> {
    let p = icon.position.to_physical::<f64>(1.0);
    let mut screen = app.monitor_from_point(p.x, p.y)?;
    // On Windows an icon in the hidden icons flyout reports a place off every screen.
    if cfg!(target_os = "macos") {
        screen = screen.or(app.primary_monitor()?);
    }
    let Some(screen) = screen else {
        return Ok(false);
    };
    let scale = screen.scale_factor();
    let pos = icon.position.to_logical::<f64>(scale);
    let size = icon.size.to_logical::<f64>(scale);
    if size.width == 0.0 {
        return Ok(false);
    }
    let spos = screen.position().to_logical::<f64>(scale);
    let ssize = screen.size().to_logical::<f64>(scale);
    let (x, y) = popover_origin(
        Bounds {
            x: pos.x,
            y: pos.y,
            w: size.width,
            h: size.height,
        },
        logical_size(window)?,
        Bounds {
            x: spos.x,
            y: spos.y,
            w: ssize.width,
            h: ssize.height,
        },
    );
    window.set_position(LogicalPosition::new(x, y))?;
    Ok(true)
}

fn place_in_corner(app: &AppHandle, window: &WebviewWindow) -> tauri::Result<()> {
    let Some(screen) = app.primary_monitor()? else {
        return Ok(());
    };
    // The window's own scale: `set_position` converts the logical point back with it.
    let scale = window.scale_factor()?;
    let area = screen.work_area();
    let pos = area.position.to_logical::<f64>(scale);
    let size = area.size.to_logical::<f64>(scale);
    let (x, y) = corner_origin(
        logical_size(window)?,
        Bounds {
            x: pos.x,
            y: pos.y,
            w: size.width,
            h: size.height,
        },
    );
    window.set_position(LogicalPosition::new(x, y))
}

/// Shows the popover by the icon. `icon` is the clicked icon's place; None looks it up.
pub fn show_popover(app: &AppHandle, icon: Option<Rect>) {
    let Some(w) = app.get_webview_window(MAIN_WINDOW) else {
        return;
    };
    let icon = icon.or_else(|| {
        app.tray_by_id(TRAY_ID)
            .and_then(|t| t.rect().ok().flatten())
    });
    let placed = icon.is_some_and(|icon| place(app, &w, icon).unwrap_or(false));
    // Windows keeps a new tray icon in the hidden icons flyout, where it has no place. On
    // macOS an icon not laid out yet leaves the window in its last place.
    if !placed && cfg!(not(target_os = "macos")) {
        let _ = place_in_corner(app, &w);
    }
    let _ = w.show();
    let _ = w.set_focus();
}

fn toggle_popover(app: &AppHandle, icon: Rect) {
    let Some(w) = app.get_webview_window(MAIN_WINDOW) else {
        return;
    };
    if w.is_visible().unwrap_or(false) {
        let _ = w.hide();
        return;
    }
    let last = *app
        .state::<HiddenAt>()
        .0
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if !hid_just_now(last, Instant::now()) {
        show_popover(app, Some(icon));
    }
}

/// The popover closes when the user clicks anywhere else.
pub fn hide_on_blur(window: &Window) {
    if window.label() != MAIN_WINDOW {
        return;
    }
    let _ = window.hide();
    let app = window.app_handle();
    *app.state::<HiddenAt>()
        .0
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());
}

fn item(app: &AppHandle, id: String, text: &str) -> tauri::Result<MenuItem<tauri::Wry>> {
    MenuItem::with_id(app, id, text, true, None::<&str>)
}

fn build_menu(app: &AppHandle, rows: &[TrayRow]) -> tauri::Result<Menu<tauri::Wry>> {
    let menu = Menu::new(app)?;
    for r in rows {
        let sub = Submenu::with_id(app, format!("row:{}", r.id), &r.label, true)?;
        match r.action {
            Some(TrayAction::Pause) => {
                sub.append(&item(app, format!("pause:{}", r.id), "Pause")?)?
            }
            Some(TrayAction::Resume) => {
                sub.append(&item(app, format!("resume:{}", r.id), "Resume")?)?
            }
            None => {}
        }
        sub.append(&item(
            app,
            format!("open:{}", r.id),
            &open_label(r.ade, r.connected),
        )?)?;
        menu.append(&sub)?;
    }
    if !rows.is_empty() {
        menu.append(&PredefinedMenuItem::separator(app)?)?;
    }
    menu.append(&item(app, "show".into(), "Open InstaBox")?)?;
    menu.append(&item(app, "quit".into(), "Quit")?)?;
    Ok(menu)
}

pub fn build(app: &AppHandle, core: Arc<Core>) -> tauri::Result<()> {
    let menu = build_menu(app, &[])?;
    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .tooltip("InstaBox")
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                rect,
                ..
            } = event
            {
                toggle_popover(tray.app_handle(), rect);
            }
        })
        .on_menu_event(move |app, event| {
            let id = event.id().as_ref().to_string();
            match id.as_str() {
                "show" => show_popover(app, None),
                "quit" => app.exit(0),
                other => {
                    let Some((verb, sandbox)) = other.split_once(':') else {
                        return;
                    };
                    let (verb, sandbox, core) =
                        (verb.to_string(), sandbox.to_string(), core.clone());
                    tauri::async_runtime::spawn(async move {
                        // A failure shows in the app's error line.
                        let _ = match verb.as_str() {
                            "pause" => core.pause(&sandbox).await,
                            "resume" => core.resume(&sandbox).await,
                            "open" => core.open_in_ade(&sandbox, None).await,
                            _ => Ok(()),
                        };
                    });
                }
            }
        });
    // Template image: macOS tints it for light and dark menu bars.
    #[cfg(target_os = "macos")]
    let icon = tauri::image::Image::from_bytes(include_bytes!("../icons/tray.png"))?;
    // Elsewhere nothing tints it, and the black template would vanish on a dark taskbar.
    #[cfg(not(target_os = "macos"))]
    let icon = tauri::image::Image::from_bytes(include_bytes!("../icons/32x32.png"))?;
    builder = builder.icon(icon).icon_as_template(true);
    builder.build(app)?;
    Ok(())
}

/// Rebuilds the menu when the sandbox rows changed.
pub fn update(app: &AppHandle, state: &AppState) {
    let rows = tray_rows(state);
    {
        let memo = app.state::<TrayMemo>();
        let mut last = memo.0.lock().unwrap_or_else(|e| e.into_inner());
        if last.as_ref() == Some(&rows) {
            return;
        }
        *last = Some(rows.clone());
    }
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    if let Ok(menu) = build_menu(app, &rows) {
        let _ = tray.set_menu(Some(menu));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sandbox::view::{AutoSleep, SandboxView};

    fn row(id: &str, state: ViewState) -> SandboxView {
        SandboxView {
            id: id.into(),
            name: format!("box {id}"),
            state,
            size: "4 vCPU / 8 GB".into(),
            connected: true,
            ades: vec![Ade::T3],
            t3_version: None,
            free: false,
            auto_sleep: AutoSleep {
                seconds: None,
                label: "Default".into(),
            },
            run_left_minutes: None,
            cooldown_minutes: None,
            update_pending: false,
            desktop_behind: false,
        }
    }

    #[test]
    fn one_row_per_sandbox_with_pause_or_resume() {
        let state = AppState {
            sandboxes: vec![
                row("a", ViewState::Running),
                row("b", ViewState::Paused),
                row("c", ViewState::Waking),
            ],
            ..AppState::default()
        };
        let rows = tray_rows(&state);
        assert_eq!(rows[0].label, "\u{25CF} box a");
        assert_eq!(rows[1].label, "\u{25CB} box b");
        assert_eq!(rows[2].label, "\u{25CF} box c");
        assert_eq!(rows[0].action, Some(TrayAction::Pause));
        assert_eq!(rows[1].action, Some(TrayAction::Resume));
        assert_eq!(rows[2].action, None);
    }

    #[test]
    fn open_item_follows_connected() {
        assert_eq!(open_label(Ade::T3, true), "Open in T3 Code");
        assert_eq!(open_label(Ade::T3, false), "Connect to T3 Code");
        let mut r = row("a", ViewState::Running);
        r.connected = false;
        r.ades = vec![];
        let state = AppState {
            sandboxes: vec![r],
            ..AppState::default()
        };
        assert!(!tray_rows(&state)[0].connected);
        assert_eq!(tray_rows(&state)[0].ade, Ade::T3, "the default ADE");
        assert_eq!(row_label("x", ViewState::Stopped), "\u{25CB} x");
    }

    #[test]
    fn open_item_of_a_sandbox_with_several_ades() {
        let mut r = row("a", ViewState::Running);
        // A v0.1.x sandbox connected from here opens T3 as before.
        assert_eq!(open_target(&r), (Ade::T3, true));
        r.connected = false;
        assert_eq!(
            open_target(&r),
            (Ade::T3, false),
            "T3 pairs from here first"
        );
        // A cmux sandbox opens by its link from any computer.
        r.ades = vec![Ade::Cmux, Ade::T3];
        assert_eq!(open_target(&r), (Ade::Cmux, true));
        assert_eq!(open_label(Ade::Cmux, true), "Open in cmux");
    }

    const SCREEN: Bounds = Bounds {
        x: 0.0,
        y: 0.0,
        w: 1440.0,
        h: 900.0,
    };

    #[test]
    fn popover_is_centered_under_the_icon() {
        let icon = Bounds {
            x: 700.0,
            y: 0.0,
            w: 24.0,
            h: 24.0,
        };
        assert_eq!(popover_origin(icon, (360.0, 600.0), SCREEN), (532.0, 28.0));
    }

    #[test]
    fn popover_stays_on_the_icon_screen() {
        let right = Bounds {
            x: 1420.0,
            y: 0.0,
            w: 24.0,
            h: 24.0,
        };
        assert_eq!(popover_origin(right, (360.0, 600.0), SCREEN).0, 1072.0);
        let left = Bounds {
            x: 2.0,
            y: 0.0,
            w: 24.0,
            h: 24.0,
        };
        assert_eq!(popover_origin(left, (360.0, 600.0), SCREEN).0, 8.0);
        let second = Bounds {
            x: 1440.0,
            y: 0.0,
            w: 1920.0,
            h: 1080.0,
        };
        let icon = Bounds {
            x: 3340.0,
            y: 0.0,
            w: 24.0,
            h: 24.0,
        };
        assert_eq!(popover_origin(icon, (360.0, 600.0), second).0, 2992.0);
    }

    #[test]
    fn popover_opens_above_an_icon_in_a_bottom_taskbar() {
        let screen = Bounds {
            x: 0.0,
            y: 0.0,
            w: 1920.0,
            h: 1080.0,
        };
        let icon = Bounds {
            x: 1700.0,
            y: 1040.0,
            w: 24.0,
            h: 40.0,
        };
        assert_eq!(
            popover_origin(icon, (360.0, 600.0), screen),
            (1532.0, 436.0)
        );
        let corner = Bounds { x: 1900.0, ..icon };
        assert_eq!(popover_origin(corner, (360.0, 600.0), screen).0, 1552.0);
        // A second screen below the first: the icon's own screen decides.
        let lower = Bounds {
            y: 1080.0,
            ..screen
        };
        let icon = Bounds { y: 2120.0, ..icon };
        assert_eq!(popover_origin(icon, (360.0, 600.0), lower).1, 1516.0);
    }

    #[test]
    fn without_an_icon_place_the_popover_sits_in_the_work_area_corner() {
        let work = Bounds {
            x: 0.0,
            y: 0.0,
            w: 1280.0,
            h: 752.0,
        };
        assert_eq!(corner_origin((376.0, 609.0), work), (896.0, 135.0));
        let tiny = Bounds { h: 500.0, ..work };
        assert_eq!(corner_origin((376.0, 609.0), tiny).1, 0.0);
    }

    #[test]
    fn a_short_screen_keeps_the_popover_top_on_screen() {
        let screen = Bounds {
            x: 0.0,
            y: 0.0,
            w: 1093.0,
            h: 614.0,
        };
        let icon = Bounds {
            x: 900.0,
            y: 578.0,
            w: 24.0,
            h: 36.0,
        };
        assert_eq!(popover_origin(icon, (360.0, 600.0), screen).1, 8.0);
    }

    #[test]
    fn a_short_screen_keeps_the_popover_bottom_on_screen() {
        // A taskbar at the top of a short screen: the popover opens below the icon.
        let screen = Bounds {
            x: 0.0,
            y: 0.0,
            w: 1093.0,
            h: 614.0,
        };
        let icon = Bounds {
            x: 900.0,
            y: 0.0,
            w: 24.0,
            h: 36.0,
        };
        assert_eq!(popover_origin(icon, (360.0, 600.0), screen).1, 8.0);
    }

    #[test]
    fn a_click_right_after_a_blur_does_not_reopen() {
        let now = Instant::now();
        assert!(!hid_just_now(None, now));
        assert!(hid_just_now(Some(now), now + Duration::from_millis(100)));
        assert!(!hid_just_now(Some(now), now + Duration::from_millis(400)));
    }
}
