//! The footer's actions and the keys that run them. An action's own
//! shortcut runs it, unless the keymap already binds that key where the
//! focus is (the keymap wins). The other actions are numbered, the primary
//! one first, and `launcher::RunAction(n)` runs the nth: ↵ for 1 and ⌥2… for
//! the rest in the default keymap.

use delight_sdk::Action;
use gpui::Keystroke;

/// The key that runs a footer action.
#[derive(Debug, Clone, PartialEq)]
pub enum ActionKey {
    /// Run by `launcher::RunAction(n)`, from 1.
    Numbered(usize),
    /// The action's own keystroke, e.g. `cmd-enter`.
    Own(Keystroke),
}

/// The actions with their keys, the primary one first. An own shortcut that
/// doesn't parse, or that `is_bound` in the keymap, gets a number instead.
pub fn keyed(mut actions: Vec<Action>, is_bound: impl Fn(&Keystroke) -> bool) -> Vec<(Action, ActionKey)> {
    if let Some(i) = actions.iter().position(|a| a.primary) {
        let primary = actions.remove(i);
        actions.insert(0, primary);
    }
    let mut next = 1;
    actions
        .into_iter()
        .map(|action| {
            let own = action.shortcut.as_deref().and_then(|s| Keystroke::parse(s).ok()).filter(|k| !is_bound(k));
            let key = own.map(ActionKey::Own).unwrap_or_else(|| {
                next += 1;
                ActionKey::Numbered(next - 1)
            });
            (action, key)
        })
        .collect()
}

/// Whether `pressed` is `own` (same key, same modifiers).
pub fn matches(own: &Keystroke, pressed: &Keystroke) -> bool {
    own.modifiers == pressed.modifiers && own.key.eq_ignore_ascii_case(&pressed.key)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(actions: Vec<Action>, bound: &[&str]) -> Vec<(String, ActionKey)> {
        let bound: Vec<Keystroke> = bound.iter().map(|k| Keystroke::parse(k).unwrap()).collect();
        let is_bound = |k: &Keystroke| bound.iter().any(|b| matches(b, k));
        keyed(actions, is_bound).into_iter().map(|(a, k)| (a.id, k)).collect()
    }

    fn own(keystroke: &str) -> ActionKey {
        ActionKey::Own(Keystroke::parse(keystroke).unwrap())
    }

    #[test]
    fn primary_first_numbered_own_shortcuts_stay() {
        let actions = vec![
            Action::custom("a", "A"),
            Action::custom("delete", "Delete").shortcut("cmd-shift-backspace"),
            Action::custom("b", "B").primary(),
            Action::custom("c", "C"),
        ];
        let expected = [
            ("b".to_string(), ActionKey::Numbered(1)),
            ("a".to_string(), ActionKey::Numbered(2)),
            ("delete".to_string(), own("cmd-shift-backspace")),
            ("c".to_string(), ActionKey::Numbered(3)),
        ];
        assert_eq!(keys(actions, &[]), expected);
    }

    #[test]
    fn keymap_wins_over_an_own_shortcut() {
        let actions = vec![Action::custom("a", "A").shortcut("cmd-k"), Action::custom("b", "B").shortcut("cmd-enter")];
        let expected = [("a".to_string(), ActionKey::Numbered(1)), ("b".to_string(), own("cmd-enter"))];
        assert_eq!(keys(actions, &["cmd-k"]), expected);
    }

    #[test]
    fn own_shortcuts_match_exactly() {
        let own = Keystroke::parse("cmd-shift-enter").unwrap();
        assert!(matches(&own, &Keystroke::parse("cmd-shift-enter").unwrap()));
        assert!(!matches(&own, &Keystroke::parse("enter").unwrap()), "↵ alone isn't ⌘⇧↵");
    }
}
