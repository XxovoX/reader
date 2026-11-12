//! A separate transparent, non-activating window; the main library stays hidden.
use crate::app::hover_reader::{
    resized_bounds, should_reveal, Bounds, ResizeEdge, SavedHoverGeometry,
};
use serde::Serialize;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use tauri::{Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

const LABEL: &str = "hover-reader";
static GEOMETRY_WRITE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Default)]
struct HoverController {
    ready: AtomicBool,
    suspended: AtomicBool,
    visible: AtomicBool,
    stopped: AtomicBool,
    book_url: Mutex<Option<String>>,
    resizing: Mutex<Option<ResizeSession>>,
}

#[derive(Clone, Copy)]
struct ResizeSession {
    bounds: Bounds,
    pointer: (f64, f64),
    edge: ResizeEdge,
}

#[cfg(target_os = "macos")]
#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventSourceButtonState(state_id: i32, button: u32) -> bool;
}

#[cfg(target_os = "macos")]
fn left_mouse_down() -> bool {
    // Combined session state; querying the button state needs no input-monitoring permission.
    unsafe { CGEventSourceButtonState(0, 0) }
}

fn logical_cursor(window: &tauri::WebviewWindow) -> Result<(f64, f64), tauri::Error> {
    let cursor = window.cursor_position()?;
    // Tao macOS reports the global cursor using the PRIMARY display scale,
    // while window geometry uses the CURRENT display scale. Work in points.
    #[cfg(target_os = "macos")]
    let scale = window
        .primary_monitor()?
        .map(|m| m.scale_factor())
        .unwrap_or(1.);
    #[cfg(not(target_os = "macos"))]
    let scale = window.scale_factor()?;
    Ok((cursor.x / scale, cursor.y / scale))
}

fn geometry_path(app: &tauri::AppHandle) -> std::path::PathBuf {
    std::path::Path::new(&app.state::<crate::api::AppState>().config.storage_dir)
        .join("hover-window.json")
}
fn capture_geometry(window: &tauri::WebviewWindow) -> Option<SavedHoverGeometry> {
    let size = window.inner_size().ok()?;
    let position = window.outer_position().ok()?;
    let scale = window.scale_factor().ok()?;
    let saved = SavedHoverGeometry {
        width: size.width as f64 / scale,
        height: size.height as f64 / scale,
        x: Some(position.x as f64 / scale),
        y: Some(position.y as f64 / scale),
    };
    saved.is_valid().then_some(saved)
}

fn save_geometry(app: &tauri::AppHandle, window: &tauri::WebviewWindow, source: &HoverController) {
    write_geometry(app, window, Some(source));
}

fn write_geometry(
    app: &tauri::AppHandle,
    window: &tauri::WebviewWindow,
    source: Option<&HoverController>,
) {
    // Capture outside the write lock: native window APIs may wait for the UI
    // thread, which also flushes the file when the app exits.
    if let Some(saved) = capture_geometry(window) {
        let Ok(_write_guard) = GEOMETRY_WRITE_LOCK.lock() else {
            return;
        };
        // A poll may have captured an older frame before the UI thread stopped
        // it. Never let that pending write replace the final shutdown snapshot.
        if source.is_some_and(|state| state.stopped.load(Ordering::SeqCst)) {
            return;
        }
        let path = geometry_path(app);
        let temp = path.with_extension("json.tmp");
        let result = (|| -> std::io::Result<()> {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&temp, serde_json::to_vec(&saved)?)?;
            std::fs::rename(temp, path)
        })();
        if let Err(error) = result {
            tracing::warn!("Could not save hover geometry: {error}");
        }
    }
}

/// Flush before app shutdown, including changes still inside the debounce period.
pub fn persist_hover_reader_geometry(app: &tauri::AppHandle) {
    if let Ok(state) = controller(app) {
        state.stopped.store(true, Ordering::SeqCst);
    }
    if let Some(window) = app.get_webview_window(LABEL) {
        write_geometry(app, &window, None);
    }
}

/// macOS does not implement Tauri's startResizeDragging. Track a real mouse drag
/// natively so shrinking past the cursor never enables click-through mid-drag.
#[tauri::command]
pub fn resize_hover_reader(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    edge: ResizeEdge,
) -> Result<(), String> {
    if window.label() != LABEL {
        return Err("仅悬浮窗口可调用".into());
    }
    #[cfg(not(target_os = "macos"))]
    {
        let direction = match edge {
            ResizeEdge::North => tauri::window::ResizeDirection::North,
            ResizeEdge::South => tauri::window::ResizeDirection::South,
            ResizeEdge::East => tauri::window::ResizeDirection::East,
            ResizeEdge::West => tauri::window::ResizeDirection::West,
            ResizeEdge::NorthEast => tauri::window::ResizeDirection::NorthEast,
            ResizeEdge::NorthWest => tauri::window::ResizeDirection::NorthWest,
            ResizeEdge::SouthEast => tauri::window::ResizeDirection::SouthEast,
            ResizeEdge::SouthWest => tauri::window::ResizeDirection::SouthWest,
        };
        return window
            .start_resize_dragging(direction)
            .map_err(|e| e.to_string());
    }
    #[cfg(target_os = "macos")]
    {
        let state = controller(&app)?;
        if !state.visible.load(Ordering::SeqCst) || !left_mouse_down() {
            return Ok(());
        }
        let pos = window.outer_position().map_err(|e| e.to_string())?;
        let size = window.outer_size().map_err(|e| e.to_string())?;
        let cursor = logical_cursor(&window).map_err(|e| e.to_string())?;
        let scale = window.scale_factor().map_err(|e| e.to_string())?;
        *state.resizing.lock().map_err(|_| "窗口状态不可用")? = Some(ResizeSession {
            bounds: Bounds {
                x: pos.x as f64 / scale,
                y: pos.y as f64 / scale,
                width: size.width as f64 / scale,
                height: size.height as f64 / scale,
            },
            pointer: cursor,
            edge,
        });
        Ok(())
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HoverStatus {
    visible: bool,
    suspended: bool,
}

fn controller(app: &tauri::AppHandle) -> Result<Arc<HoverController>, String> {
    app.try_state::<Arc<HoverController>>()
        .map(|state| state.inner().clone())
        .ok_or_else(|| "悬浮窗口未初始化".into())
}

/// Create only from the management window; no arbitrary URL or remote webview.
#[tauri::command]
pub async fn open_hover_reader(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    book_url: String,
) -> Result<(), String> {
    if window.label() != "main" {
        return Err("请从书架打开悬浮阅读".into());
    }
    if app.get_webview_window(LABEL).is_some() {
        return Err("悬浮阅读已打开，请右键返回书架后再换书".into());
    }
    if let Some(previous) = app.unmanage::<Arc<HoverController>>() {
        previous.stopped.store(true, Ordering::SeqCst);
    }
    let state = Arc::new(HoverController::default());
    app.manage(state.clone());
    *state.book_url.lock().map_err(|_| "书籍状态不可用")? = Some(book_url);
    state.ready.store(false, Ordering::SeqCst);
    state.visible.store(false, Ordering::SeqCst);
    state.suspended.store(false, Ordering::SeqCst);
    state.stopped.store(false, Ordering::SeqCst);
    let saved = std::fs::read(geometry_path(&app))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<SavedHoverGeometry>(&bytes).ok())
        .filter(SavedHoverGeometry::is_valid)
        .unwrap_or_default();
    let (size, position) = saved.restore(&crate::app::window_state::monitor_rects(&window));
    let builder = WebviewWindowBuilder::new(
        &app,
        LABEL,
        WebviewUrl::App("index.html#/reader?mode=hover".into()),
    )
    .title("工作记录")
    .inner_size(size.width, size.height)
    .resizable(true)
    .min_inner_size(280., 90.)
    .decorations(false)
    .transparent(true)
    .background_color(tauri::window::Color(0, 0, 0, 0))
    .shadow(false)
    .always_on_top(true)
    .visible_on_all_workspaces(true)
    .skip_taskbar(true)
    .focused(false)
    .focusable(false)
    .accept_first_mouse(true)
    .visible(false)
    .on_navigation(|url| {
        matches!(url.scheme(), "tauri" | "about") || url.host_str() == Some("tauri.localhost")
    });
    let builder = if let Some(position) = position {
        builder.position(position.x, position.y)
    } else {
        builder.center()
    };
    let overlay = builder.build().map_err(|e| e.to_string())?;
    overlay
        .set_ignore_cursor_events(true)
        .map_err(|e| e.to_string())?;
    // Poll the global pointer, not DOM mouseenter. A fully transparent,
    // click-through window otherwise cannot receive the event that reveals it.
    let handle = app.clone();
    let watched = state.clone();
    tauri::async_runtime::spawn(async move {
        let mut poll_count = 0u32;
        let mut last_geometry = None;
        let mut geometry_changed_at: Option<std::time::Instant> = None;
        let mut timer = tokio::time::interval(std::time::Duration::from_millis(40));
        timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            timer.tick().await;
            if watched.stopped.load(Ordering::SeqCst) {
                break;
            }
            let Some(window) = handle.get_webview_window(LABEL) else {
                break;
            };
            poll_count = poll_count.wrapping_add(1);
            if let Some(geometry) = capture_geometry(&window) {
                if last_geometry != Some(geometry) {
                    last_geometry = Some(geometry);
                    geometry_changed_at = Some(std::time::Instant::now());
                } else if geometry_changed_at
                    .is_some_and(|at| at.elapsed() >= std::time::Duration::from_millis(400))
                {
                    save_geometry(&handle, &window, &watched);
                    geometry_changed_at = None;
                }
            }
            #[allow(unused_mut)]
            let mut resizing = false;
            #[cfg(target_os = "macos")]
            {
                // Never hold this mutex across a window API: those calls may
                // wait for the UI thread, which also handles resize IPC.
                let session = watched.resizing.lock().ok().and_then(|guard| *guard);
                if let Some(drag) = session {
                    if left_mouse_down() && !watched.suspended.load(Ordering::SeqCst) {
                        resizing = true;
                        if let Ok(cursor) = logical_cursor(&window) {
                            let b = resized_bounds(
                                drag.bounds,
                                drag.edge,
                                (cursor.0 - drag.pointer.0, cursor.1 - drag.pointer.1),
                                1.,
                            );
                            let _ = window.set_size(tauri::LogicalSize::new(b.width, b.height));
                            let _ = window.set_position(tauri::LogicalPosition::new(b.x, b.y));
                        }
                    } else {
                        if let Ok(mut guard) = watched.resizing.lock() {
                            *guard = None;
                        }
                        save_geometry(&handle, &window, &watched);
                    }
                }
            }
            let visible = resizing
                || match (
                    window.outer_position(),
                    window.outer_size(),
                    logical_cursor(&window),
                    window.scale_factor(),
                ) {
                    (Ok(pos), Ok(size), Ok(cursor), Ok(scale)) => should_reveal(
                        Bounds {
                            x: pos.x as f64 / scale,
                            y: pos.y as f64 / scale,
                            width: size.width as f64 / scale,
                            height: size.height as f64 / scale,
                        },
                        cursor,
                        watched.ready.load(Ordering::SeqCst),
                        watched.suspended.load(Ordering::SeqCst),
                    ),
                    _ => false,
                };
            let changed = visible != watched.visible.swap(visible, Ordering::SeqCst);
            if changed {
                let _ = window.set_ignore_cursor_events(!visible);
            }
            if changed || poll_count % 5 == 0 {
                let _ = window.emit(
                    "hover-reader-visibility",
                    HoverStatus {
                        visible,
                        suspended: watched.suspended.load(Ordering::SeqCst),
                    },
                );
            }
        }
    });
    Ok(())
}

#[tauri::command]
pub fn hover_reader_ready(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
) -> Result<HoverStatus, String> {
    if window.label() != LABEL {
        return Err("仅悬浮窗口可调用".into());
    }
    let state = controller(&app)?;
    state.ready.store(true, Ordering::SeqCst);
    window
        .set_ignore_cursor_events(true)
        .map_err(|e| e.to_string())?;
    window.show().map_err(|e| e.to_string())?;
    if let Some(main) = app.get_webview_window("main") {
        let _ = main.hide();
    }
    Ok(HoverStatus {
        visible: state.visible.load(Ordering::SeqCst),
        suspended: state.suspended.load(Ordering::SeqCst),
    })
}

#[tauri::command]
pub fn suspend_hover_reader(app: tauri::AppHandle, suspended: bool) -> Result<(), String> {
    let state = controller(&app)?;
    state.suspended.store(suspended, Ordering::SeqCst);
    if suspended {
        state.visible.store(false, Ordering::SeqCst);
        if let Some(window) = app.get_webview_window(LABEL) {
            let _ = window.set_ignore_cursor_events(true);
            let _ = window.emit(
                "hover-reader-visibility",
                HoverStatus {
                    visible: false,
                    suspended: true,
                },
            );
        }
    }
    Ok(())
}

#[tauri::command]
pub fn return_from_hover_reader(app: tauri::AppHandle) -> Result<(), String> {
    if let Ok(state) = controller(&app) {
        state.stopped.store(true, Ordering::SeqCst);
    }
    if let Some(window) = app.get_webview_window(LABEL) {
        write_geometry(&app, &window, None);
        window.destroy().map_err(|e| e.to_string())?;
    }
    if let Some(main) = app.get_webview_window("main") {
        let _ = main.show();
        let _ = main.set_focus();
        let _ = main.emit("hover-reader-return", ());
    }
    Ok(())
}

#[tauri::command]
pub async fn get_hover_reader_book(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    state: tauri::State<'_, crate::api::AppState>,
) -> Result<crate::model::book::Book, String> {
    if window.label() != LABEL {
        return Err("仅悬浮窗口可调用".into());
    }
    let control = controller(&app)?;
    let selected = control
        .book_url
        .lock()
        .map_err(|_| "书籍状态不可用")?
        .clone()
        .ok_or("尚未选择书籍")?;
    state
        .book_service
        .get_bookshelf("default")
        .await
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|book| book.book_url == selected)
        .ok_or_else(|| "书籍不在书架中".into())
}
