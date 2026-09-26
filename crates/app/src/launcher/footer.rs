//! The footer's actions and the keys that run them: each action's own
//! shortcut, in the tool's order. A shortcut the keymap already binds where
//! the focus is (the keymap wins), or an earlier action's, is dropped: that
//! action is only clicked.

use delight_sdk::Action;
use gpui::Keystroke;

/// The actions, in order, each with the key that runs it (`None`: clicked).
pub fn keyed(actions: Vec<Action>, is_bound: impl Fn(&Keystroke) -> bool) -> Vec<(Action, Option<Keystroke>)> {
    let mut taken: Vec<Keystroke> = Vec::new();
    actions
        .into_iter()
        .map(|action| {
            let key = action
                .shortcut
                .as_deref()
                .and_then(|s| Keystroke::parse(s).ok())
                .filter(|k| !is_bound(k) && !taken.iter().any(|t| matches(t, k)));
            taken.extend(key.clone());
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

    fn keys(actions: Vec<Action>, bound: &[&str]) -> Vec<(String, Option<Keystroke>)> {
        let bound: Vec<Keystroke> = bound.iter().map(|k| Keystroke::parse(k).unwrap()).collect();
        let is_bound = |k: &Keystroke| bound.iter().any(|b| matches(b, k));
        keyed(actions, is_bound).into_iter().map(|(a, k)| (a.id, k)).collect()
    }

    fn key(keystroke: &str) -> Option<Keystroke> {
        Some(Keystroke::parse(keystroke).unwrap())
    }

    #[test]
    fn keeps_the_order_and_own_keys() {
        let actions = vec![
            Action::new("copy", "Copy").shortcut("enter"),
            Action::new("open", "Open"),
            Action::new("delete", "Delete").shortcut("cmd-shift-backspace"),
        ];
        let expected = [
            ("copy".to_string(), key("enter")),
            ("open".to_string(), None),
            ("delete".to_string(), key("cmd-shift-backspace")),
        ];
        assert_eq!(keys(actions, &[]), expected);
    }

    #[test]
    fn the_keymap_and_earlier_actions_win() {
        let actions = vec![
            Action::new("a", "A").shortcut("cmd-k"),
            Action::new("b", "B").shortcut("enter"),
            Action::new("c", "C").shortcut("enter"),
        ];
        let expected = [("a".to_string(), None), ("b".to_string(), key("enter")), ("c".to_string(), None)];
        assert_eq!(keys(actions, &["cmd-k"]), expected);
    }

    #[test]
    fn own_shortcuts_match_exactly() {
        let own = Keystroke::parse("cmd-shift-enter").unwrap();
        assert!(matches(&own, &Keystroke::parse("cmd-shift-enter").unwrap()));
        assert!(!matches(&own, &Keystroke::parse("enter").unwrap()), "↵ alone isn't ⌘⇧↵");
    }
}
