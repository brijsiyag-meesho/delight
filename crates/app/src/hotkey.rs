//! The system-wide shortcut that shows and hides the launcher
//! ([`Settings::launcher_shortcut`](delight_core::Settings), ⌘⇧Space by
//! default). It's a GPUI keystroke (`cmd-shift-space`) like every other key
//! in Delight; macOS registers it through `global-hotkey`, which reads the
//! same key names joined with `+`. It can be changed while Delight runs.

use anyhow::{anyhow, ensure};
use delight_core::settings::DEFAULT_LAUNCHER_SHORTCUT;
use delight_ui::keystroke_label;
use futures::channel::mpsc::{self, UnboundedReceiver};
use global_hotkey::hotkey::HotKey;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use gpui::{Global, Keystroke};

/// The registered launcher shortcut, a GPUI global. It works while this
/// value is alive.
pub struct LauncherShortcut {
    manager: GlobalHotKeyManager,
    keystroke: Keystroke,
    hotkey: HotKey,
}

impl Global for LauncherShortcut {}

impl LauncherShortcut {
    /// Registers `shortcut`; if it's invalid or refused, logs why and
    /// registers ⌘⇧Space instead. Also returns a stream with one item per
    /// press.
    pub fn register(shortcut: &str) -> anyhow::Result<(Self, UnboundedReceiver<()>)> {
        let manager = GlobalHotKeyManager::new()?;
        let register = |shortcut: &str| -> anyhow::Result<(Keystroke, HotKey)> {
            let (keystroke, hotkey) = parse(shortcut)?;
            manager.register(hotkey)?;
            Ok((keystroke, hotkey))
        };
        let (keystroke, hotkey) = match register(shortcut) {
            Ok(registered) => registered,
            Err(e) => {
                log::error!("launcher shortcut {shortcut:?}: {e:#} — using {DEFAULT_LAUNCHER_SHORTCUT}");
                register(DEFAULT_LAUNCHER_SHORTCUT)?
            }
        };
        let (sender, presses) = mpsc::unbounded();
        // Only Delight's one shortcut is registered: every press is it.
        GlobalHotKeyEvent::set_event_handler(Some(move |event: GlobalHotKeyEvent| {
            if event.state == HotKeyState::Pressed {
                let _ = sender.unbounded_send(());
            }
        }));
        Ok((Self { manager, keystroke, hotkey }, presses))
    }

    pub fn keystroke(&self) -> &Keystroke {
        &self.keystroke
    }

    /// Switches to `keystroke`. The new one is registered before the old one
    /// is released, so a refused shortcut leaves the old one working.
    pub fn change(&mut self, keystroke: &Keystroke) -> anyhow::Result<()> {
        let hotkey = to_hotkey(keystroke)?;
        if hotkey != self.hotkey {
            self.manager
                .register(hotkey)
                .map_err(|e| anyhow!("macOS refused {}: {e}", keystroke_label(keystroke)))?;
            let _ = self.manager.unregister(self.hotkey);
            self.hotkey = hotkey;
        }
        self.keystroke = keystroke.clone();
        Ok(())
    }
}

/// Reads a shortcut as settings store it (`cmd-shift-space`).
pub fn parse(shortcut: &str) -> anyhow::Result<(Keystroke, HotKey)> {
    let keystroke = Keystroke::parse(shortcut)?;
    // `shift+super+Space` (an older format) parses as one odd key.
    ensure!(!keystroke.key.contains('+'), "not written like {DEFAULT_LAUNCHER_SHORTCUT}");
    let hotkey = to_hotkey(&keystroke)?;
    Ok((keystroke, hotkey))
}

/// `cmd-shift-space` → `cmd+shift+space`: how `global-hotkey` (and the menu
/// bar menu, through `muda`) write a shortcut.
pub fn plus_separated(keystroke: &Keystroke) -> String {
    let m = &keystroke.modifiers;
    let modifiers = [(m.platform, "cmd"), (m.alt, "alt"), (m.control, "ctrl"), (m.shift, "shift")];
    let mut parts: Vec<&str> = modifiers.iter().filter(|(on, _)| *on).map(|(_, name)| *name).collect();
    parts.push(&keystroke.key);
    parts.join("+")
}

/// The keystroke as a system-wide hotkey. It must include ⌘, ⌥ or ⌃: a
/// system-wide shortcut on a plain key would stop that key typing anywhere.
fn to_hotkey(keystroke: &Keystroke) -> anyhow::Result<HotKey> {
    let m = &keystroke.modifiers;
    ensure!(m.platform || m.alt || m.control, "a shortcut needs ⌘, ⌥ or ⌃");
    plus_separated(keystroke).parse().map_err(|_| anyhow!("{} can't be a shortcut", keystroke_label(keystroke)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hotkey(shortcut: &str) -> anyhow::Result<HotKey> {
        to_hotkey(&Keystroke::parse(shortcut)?)
    }

    #[test]
    fn keystrokes_become_hotkeys() {
        assert_eq!(hotkey(DEFAULT_LAUNCHER_SHORTCUT).unwrap().into_string(), "shift+super+Space");
        assert_eq!(hotkey("alt-k").unwrap().into_string(), "alt+KeyK");
        assert!(hotkey("ctrl-alt-1").is_ok());
        assert!(hotkey("cmd-/").is_ok(), "punctuation");
        assert!(hotkey("cmd-f5").is_ok());
    }

    #[test]
    fn rejects_modifier_less_and_unknown_keys() {
        assert!(hotkey("k").is_err(), "a plain key");
        assert!(hotkey("shift-k").is_err(), "shift alone types a capital");
        assert!(hotkey("cmd-nosuchkey").is_err(), "not a key");
        assert!(parse("shift+super+Space").is_err(), "the older format");
        assert!(parse(DEFAULT_LAUNCHER_SHORTCUT).is_ok());
    }
}
