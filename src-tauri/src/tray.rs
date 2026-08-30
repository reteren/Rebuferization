//! Tray icon: Settings, and Enable/Disable. Left-click opens the popup.
//!
//! OWNER: worker W4.

use std::io::Cursor;

use once_cell::sync::Lazy;
use parking_lot::Mutex;
use tauri::{AppHandle, Emitter, Manager};

use crate::error::{AppError, AppResult};

const TRAY_ID: &str = "main";
const SETTINGS_ITEM_ID: &str = "settings";
const TOGGLE_ITEM_ID: &str = "toggle_capture";

/// Handle to the Enable/Disable item, kept so `set_capture_enabled` can swap
/// its label without rebuilding the menu.
static TOGGLE_ITEM: Lazy<Mutex<Option<tauri::menu::MenuItem<tauri::Wry>>>> =
    Lazy::new(|| Mutex::new(None));

/// Loads the bundled 32×32 icon and derives the muted variant by desaturating
/// it in memory — no second asset to keep in sync.
fn icons() -> AppResult<(tauri::image::Image<'static>, tauri::image::Image<'static>)> {
    let bytes: &'static [u8] = include_bytes!("../icons/32x32.png");

    let source = image::load_from_memory(bytes)?.to_rgba8();
    let mut muted = source.clone();
    for p in muted.pixels_mut() {
        // Rec. 601 luma — the standard "grey it out" pass.
        let luma = (0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32).round() as u8;
        p[0] = luma;
        p[1] = luma;
        p[2] = luma;
    }
    let mut png = Vec::new();
    image::DynamicImage::ImageRgba8(muted)
        .write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)?;

    let normal = tauri::image::Image::from_bytes(bytes).map_err(|e| AppError::Other(e.to_string()))?;
    let muted = tauri::image::Image::from_bytes(&png).map_err(|e| AppError::Other(e.to_string()))?;
    Ok((normal, muted))
}

fn tauri_err(e: tauri::Error) -> AppError {
    AppError::Other(e.to_string())
}

pub fn install(app: &AppHandle) -> AppResult<()> {
    let (normal, muted) = icons()?;

    // The store is not managed yet (lib.rs manages it after this call), so the
    // initial label and muted state come from a direct file peek.
    let behavior = crate::settings::peek_behavior();

    let settings_item = tauri::menu::MenuItem::with_id(
        app,
        SETTINGS_ITEM_ID,
        "Settings",
        true,
        None::<&str>,
    )
    .map_err(tauri_err)?;
    let toggle_item = tauri::menu::MenuItem::with_id(
        app,
        TOGGLE_ITEM_ID,
        if behavior.capture_enabled { "Disable" } else { "Enable" },
        true,
        None::<&str>,
    )
    .map_err(tauri_err)?;
    *TOGGLE_ITEM.lock() = Some(toggle_item.clone());

    let menu = tauri::menu::Menu::with_items(app, &[&settings_item, &toggle_item])
        .map_err(tauri_err)?;

    let tray = tauri::tray::TrayIconBuilder::with_id(TRAY_ID)
        .icon(normal)
        .tooltip("Rebuffer")
        .menu(&menu)
        // Left-click must open the popup, right-click the menu.
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            SETTINGS_ITEM_ID => {
                let _ = crate::window::show_settings(app);
            }
            TOGGLE_ITEM_ID => toggle_capture_from_tray(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let tauri::tray::TrayIconEvent::Click {
                button: tauri::tray::MouseButton::Left,
                button_state: tauri::tray::MouseButtonState::Up,
                ..
            } = event
            {
                let _ = crate::window::show_popup(tray.app_handle());
            }
        })
        .build(app)
        .map_err(tauri_err)?;

    if !behavior.capture_enabled {
        tray.set_icon(Some(muted)).map_err(tauri_err)?;
    }

    // Autostart: the plugin writes the HKCU Run key when `launchOnStartup`
    // flips; `silentStart` needs the Run value rewritten without `--silent`
    // because the plugin's args are fixed at init. The store has no
    // `AppHandle`, so the hook lives here and `patch` calls it.
    let hook_app = app.clone();
    let app_name = app.package_info().name.clone();
    crate::settings::set_autostart_hook(move |b: &crate::settings::BehaviorSettings| {
        use tauri_plugin_autostart::ManagerExt;
        let autolaunch = hook_app.autolaunch();
        if b.launch_on_startup {
            if let Err(e) = autolaunch.enable() {
                tracing::warn!("autostart enable failed: {e}");
                return;
            }
            if !b.silent_start {
                if let Err(e) = crate::settings::rewrite_run_value(&app_name, false) {
                    tracing::warn!("autostart silentStart rewrite failed: {e}");
                }
            }
        } else if let Err(e) = autolaunch.disable() {
            tracing::warn!("autostart disable failed: {e}");
        }
    });
    // One alignment pass at startup so a hand-edited settings.json is healed.
    crate::settings::apply_autostart(&behavior);

    Ok(())
}

/// Flips the capture toggle: settings, listener, and tray icon in one go —
/// the same state `commands::set_capture_enabled` keeps consistent.
fn toggle_capture_from_tray(app: &AppHandle) {
    let Some(state) = app.try_state::<crate::AppState>() else {
        tracing::warn!("tray toggle before AppState was managed");
        return;
    };
    let enabled = !state.settings.get().behavior.capture_enabled;
    if let Err(e) = state
        .settings
        .patch(serde_json::json!({ "behavior": { "captureEnabled": enabled } }))
    {
        tracing::warn!("failed to persist capture toggle: {e}");
        return;
    }
    state.clipboard.set_enabled(enabled);
    if let Err(e) = set_capture_enabled(app, enabled) {
        tracing::warn!("failed to update tray icon: {e}");
    }
    let _ = app.emit(crate::model::events::SETTINGS_CHANGED, state.settings.get());
}

/// Swaps to the muted icon variant while capture is disabled, and updates the
/// menu item label.
pub fn set_capture_enabled(app: &AppHandle, enabled: bool) -> AppResult<()> {
    let (normal, muted) = icons()?;
    let tray = app
        .tray_by_id(TRAY_ID)
        .ok_or_else(|| AppError::Other("tray icon not found".into()))?;
    tray.set_icon(Some(if enabled { normal } else { muted }))
        .map_err(tauri_err)?;
    if let Some(item) = TOGGLE_ITEM.lock().as_ref() {
        item.set_text(if enabled { "Disable" } else { "Enable" })
            .map_err(tauri_err)?;
    }
    Ok(())
}