//! Icons: [Lucide](https://lucide.dev) v1.48.0 (ISC, see
//! `assets/icons/LICENSE-lucide`), embedded at compile time and tinted at
//! render time. Referenced as `icons/<lucide name>.svg`.

use std::borrow::Cow;

use gpui::{AssetSource, Result, SharedString};

pub struct Assets;

macro_rules! icons {
    ($($name:literal),* $(,)?) => {
        const ICONS: &[(&str, &[u8])] = &[
            $((concat!("icons/", $name, ".svg"), include_bytes!(concat!("../assets/icons/", $name, ".svg")))),*
        ];
    };
}

icons![
    "check",
    "circle-check",
    "circle-x",
    "copy",
    "folder",
    "info",
    "puzzle",
    "refresh-cw",
    "settings",
    "sparkles",
    "trash-2",
    "triangle-alert",
    "zap",
];

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        Ok(ICONS.iter().find(|(p, _)| *p == path).map(|(_, bytes)| Cow::Borrowed(*bytes)))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        Ok(ICONS.iter().map(|(p, _)| *p).filter(|p| p.starts_with(path)).map(SharedString::from).collect())
    }
}
