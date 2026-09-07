//! Tray icon: Settings, and Enable/Disable. Left-click opens the popup.
//!
//! OWNER: worker W4.

use std::io::Cursor;

use tauri::AppHandle;

use crate::error::{AppError, AppResult};

const TRAY_ID: &str = "main";

/// Loads the bundled 32×32 icon and derives the muted variant by desaturating
/// it in memory — no second asset to keep in sync.
fn icons() -> AppResult<(tauri::image::Image<'static>, tauri::image::Image<'static>)> {
    let bytes: &'static [u8] = include_bytes!("../icons/32x32.png");

    let source = image::load_from_memory(bytes)?.to_rgba8();
    let mut muted = source.clone();
    for p in muted.pixels_mut() {
        // Rec. 601 luma — the standard "grey it out" pass.
        let luma =
            (0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32).round() as u8;
        p[0] = luma;
        p[1] = luma;
        p[2] = luma;
    }
    let mut png = Vec::new();
    image::DynamicImage::ImageRgba8(muted)
        .write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)?;

    let normal =
        tauri::image::Image::from_bytes(bytes).map_err(|e| AppError::Other(e.to_string()))?;
    let muted =
        tauri::image::Image::from_bytes(&png).map_err(|e| AppError::Other(e.to_string()))?;
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

    // No native menu is attached. A tray HMENU is painted by Windows and no
    // amount of CSS reaches it, so it could never follow the app's themes;
    // right-click opens our own window instead, which can.
    let tray = tauri::tray::TrayIconBuilder::with_id(TRAY_ID)
        .icon(normal)
        .tooltip("Rebuffer")
        .on_tray_icon_event(|tray, event| {
            if let tauri::tray::TrayIconEvent::Click {
                button,
                button_state: tauri::tray::MouseButtonState::Up,
                ..
            } = event
            {
                let app = tray.app_handle();
                match button {
                    tauri::tray::MouseButton::Left => {
                        tracing::info!("tray icon: left click");
                        if let Err(e) = crate::window::show_popup(app) {
                            tracing::error!("could not show the popup from the tray: {e}");
                        }
                    }
                    tauri::tray::MouseButton::Right => {
                        tracing::info!("tray icon: right click");
                        if let Err(e) = crate::window::show_tray_menu(app) {
                            tracing::error!("could not show the tray menu: {e}");
                        }
                    }
                    _ => {}
                }
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

/// Swaps to the muted icon variant while capture is disabled, and updates the
/// the tray icon between its normal and muted variants.
pub fn set_capture_enabled(app: &AppHandle, enabled: bool) -> AppResult<()> {
    let (normal, muted) = icons()?;
    let tray = app
        .tray_by_id(TRAY_ID)
        .ok_or_else(|| AppError::Other("tray icon not found".into()))?;
    tray.set_icon(Some(if enabled { normal } else { muted }))
        .map_err(tauri_err)?;
    // The menu is a webview window now and reads the state itself when it
    // opens, so there is no label here to keep in sync.
    Ok(())
}
