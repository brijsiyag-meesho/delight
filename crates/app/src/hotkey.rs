//! The system-wide shortcut that shows and hides the launcher
//! ([`Settings::launcher_shortcut`](delight_core::Settings), ⌘⇧Space by
//! default).

use anyhow::{anyhow, ensure};
use delight_core::settings::DEFAULT_LAUNCHER_SHORTCUT;
use futures::channel::mpsc::{self, UnboundedReceiver};
use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};

/// A registered shortcut. It works while `manager` is alive.
pub struct Registered {
    pub manager: GlobalHotKeyManager,
    pub hotkey: HotKey,
    /// One item per press.
    pub presses: UnboundedReceiver<()>,
}

/// Parses a shortcut such as `cmd+shift+Space`. It must include ⌘, ⌥ or ⌃:
/// a system-wide shortcut on a plain key would stop that key typing anywhere.
pub fn parse(shortcut: &str) -> anyhow::Result<HotKey> {
    let hotkey: HotKey = shortcut.parse().map_err(|e| anyhow!("{shortcut:?} isn't a shortcut: {e}"))?;
    let needed = Modifiers::SUPER | Modifiers::ALT | Modifiers::CONTROL;
    ensure!(hotkey.mods.intersects(needed), "{shortcut:?} needs ⌘, ⌥ or ⌃");
    Ok(hotkey)
}

/// Registers `shortcut`; if it's invalid or another app (or macOS) has it,
/// logs why and registers ⌘⇧Space instead.
pub fn register(shortcut: &str) -> anyhow::Result<Registered> {
    let manager = GlobalHotKeyManager::new()?;
    let hotkey = match parse(shortcut).and_then(|hotkey| Ok(manager.register(hotkey).map(|()| hotkey)?)) {
        Ok(hotkey) => hotkey,
        Err(e) => {
            log::error!("launcher shortcut: {e:#} — using {DEFAULT_LAUNCHER_SHORTCUT}");
            let hotkey = parse(DEFAULT_LAUNCHER_SHORTCUT)?;
            manager.register(hotkey)?;
            hotkey
        }
    };
    let (sender, presses) = mpsc::unbounded();
    // Only Delight's one shortcut is registered: every press is it.
    GlobalHotKeyEvent::set_event_handler(Some(move |event: GlobalHotKeyEvent| {
        if event.state == HotKeyState::Pressed {
            let _ = sender.unbounded_send(());
        }
    }));
    Ok(Registered { manager, hotkey, presses })
}

/// How macOS writes the shortcut, e.g. `⌘⇧Space`, `⌥K`.
pub fn label(hotkey: HotKey) -> String {
    let mut label = String::new();
    for (modifier, symbol) in
        [(Modifiers::CONTROL, "⌃"), (Modifiers::ALT, "⌥"), (Modifiers::SHIFT, "⇧"), (Modifiers::SUPER, "⌘")]
    {
        if hotkey.mods.contains(modifier) {
            label.push_str(symbol);
        }
    }
    let key = hotkey.key.to_string();
    let key = match hotkey.key {
        Code::Enter => "↵",
        Code::Backspace => "⌫",
        Code::Tab => "⇥",
        Code::Escape => "⎋",
        _ => key.strip_prefix("Key").or_else(|| key.strip_prefix("Digit")).unwrap_or(&key),
    };
    label + key
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_labels_shortcuts() {
        assert_eq!(label(parse(DEFAULT_LAUNCHER_SHORTCUT).unwrap()), "⇧⌘Space");
        assert_eq!(label(parse("alt+KeyK").unwrap()), "⌥K");
        assert_eq!(label(parse("ctrl+alt+Digit1").unwrap()), "⌃⌥1");
    }

    #[test]
    fn rejects_invalid_and_modifier_less_shortcuts() {
        assert!(parse("cmd+NoSuchKey").is_err());
        assert!(parse("KeyK").is_err(), "a plain key");
        assert!(parse("shift+KeyK").is_err(), "shift alone types a capital");
    }
}
