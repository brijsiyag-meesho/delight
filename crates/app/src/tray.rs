//! The menu bar icon and its menu.

use anyhow::Context;
use futures::channel::mpsc::{self, UnboundedReceiver};
use resvg::{tiny_skia, usvg};
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

/// 18pt in the menu bar, @2x.
const ICON_PIXELS: u32 = 36;

/// A menu item the user clicked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayCommand {
    Restart,
    Quit,
}

/// Creates the menu bar icon. It stays in the menu bar while the returned
/// value is alive.
pub fn create() -> anyhow::Result<TrayIcon> {
    let menu = Menu::with_items(&[
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
    Ok(icon)
}

/// Menu clicks, as they happen: a stream to `.await` on, so nothing runs
/// between clicks.
pub fn clicks() -> UnboundedReceiver<TrayCommand> {
    let (sender, receiver) = mpsc::unbounded();
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        let command = match event.id.0.as_str() {
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
