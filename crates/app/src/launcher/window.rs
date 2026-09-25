//! The launcher window: opening it, showing and hiding it, and the calls the
//! rest of the app makes into it.

use gpui::{
    App, AppContext, Bounds, Focusable, SharedString, WindowBackgroundAppearance, WindowBounds, WindowHandle,
    WindowKind, WindowOptions, point, px, size,
};

use super::{BAR_HEIGHT, BAR_RADIUS, BAR_WIDTH, CRASH_TOAST, Launcher};
use crate::platform;
use crate::state::{self, AppState};

pub fn open(cx: &mut App) -> anyhow::Result<WindowHandle<Launcher>> {
    let display = cx.primary_display();
    let screen = display.as_ref().map(|d| d.bounds()).unwrap_or(Bounds::new(point(px(0.), px(0.)), size(px(1440.), px(900.))));
    // Like Spotlight: centred, about a fifth of the way down the screen.
    let origin = point(
        screen.origin.x + (screen.size.width - px(BAR_WIDTH)) / 2.,
        screen.origin.y + screen.size.height * 0.22,
    );
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::new(origin, size(px(BAR_WIDTH), px(BAR_HEIGHT))))),
        titlebar: None,
        focus: true,
        show: true,
        kind: WindowKind::PopUp,
        is_movable: true,
        is_resizable: false,
        is_minimizable: false,
        display_id: display.map(|d| d.id()),
        // Transparent: the blur is our own backdrop (see `platform`), shaped to
        // our corners — GPUI's blurred background keeps macOS's own radius.
        window_background: WindowBackgroundAppearance::Transparent,
        ..Default::default()
    };
    let handle = cx.open_window(options, |window, cx| {
        platform::patch_gpui_focus();
        platform::style_floating_panel(window, BAR_RADIUS.into());
        cx.new(|cx| Launcher::new(window, cx))
    })?;
    handle.update(cx, |launcher, window, cx| {
        window.focus(&launcher.input.focus_handle(cx));
        launcher.sync_window_size(window, cx);
        // Why the previous run ended, if a crash ended it.
        if let Some(note) = delight_core::guard::take_crash_note() {
            launcher.flash_for(note, CRASH_TOAST, cx);
        }
    })?;
    Ok(handle)
}

fn handle(cx: &App) -> Option<WindowHandle<Launcher>> {
    cx.global::<AppState>().launcher
}

/// Shows the launcher, or hides it if it's already in front.
pub fn toggle(cx: &mut App) {
    let Some(handle) = handle(cx) else { return };
    let in_front = handle
        .update(cx, |_, window, _| platform::is_window_visible(window) && window.is_window_active())
        .unwrap_or(false);
    if in_front { hide(cx) } else { show(cx) }
}

pub fn show(cx: &mut App) {
    let Some(handle) = handle(cx) else { return };
    let native = handle.update(cx, |launcher, window, cx| {
        launcher.sync_window_size(window, cx);
        platform::native_window(window)
    });
    let Ok(Some(native)) = native else { return };
    // Present outside the GPUI update (AppKit calls back into GPUI), then
    // focus the input once the window is key — earlier focus doesn't stick.
    cx.spawn(async move |cx| {
        platform::present(native);
        let _ = handle.update(cx, |launcher, window, cx| {
            window.focus(&launcher.input.focus_handle(cx));
            if state::settings(cx).paste_clipboard_on_open {
                launcher.paste_clipboard(cx);
            }
            // Like Spotlight: the previous text stays, selected, so typing replaces it.
            launcher.input.update(cx, |input, cx| input.select_all_text(cx));
        });
    })
    .detach();
}

/// Hides the launcher, keeping the input to restore next launch (if the
/// input history is on).
pub fn hide(cx: &mut App) {
    let Some(handle) = handle(cx) else { return };
    let text = handle.update(cx, |launcher, window, cx| {
        platform::hide_window(window);
        launcher.text(cx)
    });
    let state = cx.global_mut::<AppState>();
    if let Ok(text) = text
        && state.settings.input_history
        && let Err(e) = state.input_history.set_input_to_restore(&text)
    {
        log::error!("saving the input: {e:#}");
    }
}

/// A brief message in the status bar.
pub fn toast(cx: &mut App, message: SharedString) {
    if let Some(handle) = handle(cx) {
        let _ = handle.update(cx, |launcher, _, cx| launcher.flash(message, cx));
    }
}

/// Asks the tools again (e.g. a plugin was turned on or off).
pub fn refresh(cx: &mut App) {
    if let Some(handle) = handle(cx) {
        let _ = handle.update(cx, |launcher, _, cx| launcher.classify(cx));
    }
}

/// The plugins were loaded again: drops the views of the old ones and asks
/// the new ones.
pub fn plugins_reloaded(cx: &mut App) {
    if let Some(handle) = handle(cx) {
        let _ = handle.update(cx, |launcher, _, cx| {
            launcher.views.clear();
            launcher.classify(cx);
        });
    }
}

/// Replaces the input (for plugins, through the host).
pub fn set_input(cx: &mut App, text: SharedString) {
    if let Some(handle) = handle(cx) {
        let _ = handle.update(cx, |launcher, _, cx| launcher.input.update(cx, |input, cx| input.set_text(text, cx)));
    }
}
