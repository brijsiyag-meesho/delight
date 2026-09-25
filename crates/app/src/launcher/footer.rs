//! The footer's actions and the keys that run them: an action's own
//! shortcut, else ↵ for the first (the primary one first), ⌥2… for the rest.

use delight_sdk::Action;
use gpui::{Keystroke, Modifiers};

/// The key that runs a footer action.
#[derive(Debug, Clone, PartialEq)]
pub enum ActionKey {
    Enter,
    /// ⌥ and a digit, from 2.
    Alt(usize),
    /// The action's own keystroke, e.g. `cmd-enter`.
    Own(Keystroke),
}

impl ActionKey {
    /// What the footer shows, e.g. `↵`, `⌥2`, `⌘⇧↵`.
    pub fn label(&self) -> String {
        match self {
            ActionKey::Enter => "↵".into(),
            ActionKey::Alt(n) => format!("⌥{n}"),
            ActionKey::Own(keystroke) => keystroke_label(keystroke),
        }
    }
}

/// The actions with their keys, the primary one first. An action whose own
/// shortcut doesn't parse gets a key assigned instead.
pub fn keyed(mut actions: Vec<Action>) -> Vec<(Action, ActionKey)> {
    if let Some(i) = actions.iter().position(|a| a.primary) {
        let primary = actions.remove(i);
        actions.insert(0, primary);
    }
    let mut enter_taken = false;
    let mut next_alt = 2;
    actions
        .into_iter()
        .map(|action| {
            let own = action.shortcut.as_deref().and_then(|s| Keystroke::parse(s).ok());
            let key = match own {
                Some(keystroke) => ActionKey::Own(keystroke),
                None if !enter_taken => {
                    enter_taken = true;
                    ActionKey::Enter
                }
                None => {
                    next_alt += 1;
                    ActionKey::Alt(next_alt - 1)
                }
            };
            (action, key)
        })
        .collect()
}

/// Whether `pressed` is `own` (same key, same modifiers).
pub fn matches(own: &Keystroke, pressed: &Keystroke) -> bool {
    own.modifiers == pressed.modifiers && own.key.eq_ignore_ascii_case(&pressed.key)
}

/// `cmd-shift-enter` → `⌘⇧↵`.
fn keystroke_label(keystroke: &Keystroke) -> String {
    let Modifiers { control, alt, shift, platform, .. } = keystroke.modifiers;
    let mut label = String::new();
    for (on, symbol) in [(control, "⌃"), (alt, "⌥"), (shift, "⇧"), (platform, "⌘")] {
        if on {
            label.push_str(symbol);
        }
    }
    let key = match keystroke.key.as_str() {
        "enter" => "↵".to_string(),
        "backspace" => "⌫".to_string(),
        "delete" => "⌦".to_string(),
        "escape" => "⎋".to_string(),
        "tab" => "⇥".to_string(),
        "space" => "Space".to_string(),
        other => other.to_uppercase(),
    };
    label + &key
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(actions: Vec<Action>) -> Vec<(String, String)> {
        keyed(actions).into_iter().map(|(a, k)| (a.id, k.label())).collect()
    }

    #[test]
    fn primary_gets_enter_the_rest_alt_digits_own_shortcuts_stay() {
        let actions = vec![
            Action::custom("a", "A"),
            Action::custom("delete", "Delete").shortcut("cmd-shift-backspace"),
            Action::custom("b", "B").primary(),
            Action::custom("c", "C"),
        ];
        let expected = [("b", "↵"), ("a", "⌥2"), ("delete", "⇧⌘⌫"), ("c", "⌥3")];
        assert_eq!(keys(actions), expected.map(|(a, k)| (a.to_string(), k.to_string())));
    }

    #[test]
    fn own_shortcuts_match_exactly() {
        let own = Keystroke::parse("cmd-shift-enter").unwrap();
        assert!(matches(&own, &Keystroke::parse("cmd-shift-enter").unwrap()));
        assert!(!matches(&own, &Keystroke::parse("enter").unwrap()), "↵ alone isn't ⌘⇧↵");
    }
}
