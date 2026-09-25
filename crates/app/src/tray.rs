//! The menu bar icon and its menu.

use anyhow::Context;
use futures::channel::mpsc::{self, UnboundedReceiver};
use resvg::{tiny_skia, usvg};
use gpui::Keystroke;
use tray_icon::menu::accelerator::Accelerator;
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

/// 18pt in the menu bar, @2x.
const ICON_PIXELS: u32 = 36;

/// A menu item the user clicked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayCommand {
    Open,
    Settings,
    Restart,
    Quit,
}

/// The menu bar icon, a GPUI global: it stays in the menu bar while this
/// value is alive.
pub struct Tray {
    _icon: TrayIcon,
    open: MenuItem,
}

impl gpui::Global for Tray {}

impl Tray {
    /// `shortcut`: the launcher's shortcut, shown next to "Open Delight".
    pub fn new(shortcut: Option<&Keystroke>) -> anyhow::Result<Self> {
        let open = MenuItem::with_id("open", "Open Delight", true, shortcut.and_then(accelerator));
        let menu = Menu::with_items(&[
            &open,
            &PredefinedMenuItem::separator(),
            &MenuItem::with_id("settings", "Settings…", true, None),
            &MenuItem::with_id("restart", "Restart Delight", true, None),
            &PredefinedMenuItem::separator(),
            &MenuItem::with_id("quit", "Quit Delight", true, None),
        ])?;
        let icon = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_icon(icon()?)
            .with_icon_as_template(true)
            .with_tooltip("Delight")
            .build()?;
        Ok(Self { _icon: icon, open })
    }

    /// Shows the launcher's new shortcut next to "Open Delight".
    pub fn set_shortcut(&self, shortcut: &Keystroke) {
        if let Err(e) = self.open.set_accelerator(accelerator(shortcut)) {
            log::warn!("menu bar shortcut: {e}");
        }
    }
}

/// The shortcut as the menu writes it (macOS draws it right-aligned).
fn accelerator(keystroke: &Keystroke) -> Option<Accelerator> {
    crate::hotkey::plus_separated(keystroke).parse().ok()
}

/// Menu clicks, as they happen: a stream to `.await` on, so nothing runs
/// between clicks.
pub fn clicks() -> UnboundedReceiver<TrayCommand> {
    let (sender, receiver) = mpsc::unbounded();
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        let command = match event.id.0.as_str() {
            "open" => TrayCommand::Open,
            "settings" => TrayCommand::Settings,
            "restart" => TrayCommand::Restart,
            "quit" => TrayCommand::Quit,
            _ => return,
        };
        let _ = sender.unbounded_send(command);
    }));
    receiver
}

fn icon() -> anyhow::Result<Icon> {
    let svg = usvg::Tree::from_data(delight_ui::LOGO_SVG, &usvg::Options::default())?;
    let mut pixmap = tiny_skia::Pixmap::new(ICON_PIXELS, ICON_PIXELS).context("empty icon")?;
    let scale = ICON_PIXELS as f32 / svg.size().width().max(svg.size().height());
    resvg::render(&svg, tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    Ok(Icon::from_rgba(pixmap.take(), ICON_PIXELS, ICON_PIXELS)?)
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_icon_renders() {
        super::icon().expect("the SVG renders to a 36×36 icon");
    }
}
