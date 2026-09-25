//! Menu bar (status item) icon and menu.

use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

pub struct Tray {
    _icon: TrayIcon,
    open: MenuId,
    settings: MenuId,
    restart: MenuId,
    quit: MenuId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayCommand {
    Open,
    Settings,
    Restart,
    Quit,
}

impl Tray {
    pub fn new() -> anyhow::Result<Self> {
        let open = MenuItem::new("Open Delight          ⌘⇧Space", true, None);
        let settings = MenuItem::new("Settings…", true, None);
        let restart = MenuItem::new("Restart Delight", true, None);
        let quit = MenuItem::new("Quit Delight", true, None);
        let menu = Menu::new();
        menu.append(&open)?;
        menu.append(&PredefinedMenuItem::separator())?;
        menu.append(&settings)?;
        menu.append(&restart)?;
        menu.append(&PredefinedMenuItem::separator())?;
        menu.append(&quit)?;

        let icon = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_icon(template_icon()?)
            .with_icon_as_template(true)
            .with_tooltip("Delight")
            .build()?;
        Ok(Self {
            _icon: icon,
            open: open.id().clone(),
            settings: settings.id().clone(),
            restart: restart.id().clone(),
            quit: quit.id().clone(),
        })
    }

    pub fn poll(&self) -> Vec<TrayCommand> {
        let mut out = Vec::new();
        while let Ok(ev) = MenuEvent::receiver().try_recv() {
            let cmd = if ev.id == self.open {
                TrayCommand::Open
            } else if ev.id == self.settings {
                TrayCommand::Settings
            } else if ev.id == self.restart {
                TrayCommand::Restart
            } else if ev.id == self.quit {
                TrayCommand::Quit
            } else {
                continue;
            };
            out.push(cmd);
        }
        out
    }
}

/// 36×36 (@2x) monochrome template: a rounded "command bar" with a bolt,
/// rasterised with 4×4 supersampling so macOS can tint it for light/dark bars.
fn template_icon() -> anyhow::Result<Icon> {
    const N: usize = 36;
    const SS: usize = 4;
    let bolt: [(f32, f32); 6] = [(20.5, 7.), (11., 20.), (17.5, 20.), (15.5, 29.), (25., 16.), (18.5, 16.)];
    let inside_bolt = |x: f32, y: f32| {
        let mut inside = false;
        let mut j = bolt.len() - 1;
        for i in 0..bolt.len() {
            let (xi, yi) = bolt[i];
            let (xj, yj) = bolt[j];
            if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
                inside = !inside;
            }
            j = i;
        }
        inside
    };
    // Rounded-rect ring.
    let ring = |x: f32, y: f32| {
        let (cx, cy, hw, hh, r) = (18., 18., 15., 15., 7.5);
        let dx = ((x - cx).abs() - (hw - r)).max(0.);
        let dy = ((y - cy).abs() - (hh - r)).max(0.);
        let d = (dx * dx + dy * dy).sqrt() - r;
        (-2.6..=0.).contains(&d)
    };
    let mut rgba = vec![0u8; N * N * 4];
    for py in 0..N {
        for px in 0..N {
            let mut hits = 0;
            for sy in 0..SS {
                for sx in 0..SS {
                    let x = px as f32 + (sx as f32 + 0.5) / SS as f32;
                    let y = py as f32 + (sy as f32 + 0.5) / SS as f32;
                    if ring(x, y) || inside_bolt(x, y) {
                        hits += 1;
                    }
                }
            }
            let a = (hits * 255 / (SS * SS)) as u8;
            let i = (py * N + px) * 4;
            rgba[i + 3] = a;
        }
    }
    Ok(Icon::from_rgba(rgba, N as u32, N as u32)?)
}
