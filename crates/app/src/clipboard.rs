//! Copying a file to the clipboard, as Finder does
//! ([`Host::copy_file`](delight_sdk::Host::copy_file)): GPUI's clipboard
//! holds text and images, not files.

use std::path::Path;

use anyhow::Context as _;

use crate::platform;

/// Writes `bytes` to a file called `name` in Delight's clipboard folder
/// (replacing the file copied before), then puts that file on the clipboard.
pub fn copy_file(name: &str, bytes: &[u8]) -> anyhow::Result<()> {
    // Only a file name: a plugin can't write outside the folder.
    let name = Path::new(name).file_name().context("the file has no name")?;
    let dir = dirs::cache_dir().context("no caches folder")?.join("Delight").join("Clipboard");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(name);
    std::fs::write(&path, bytes).with_context(|| format!("writing {}", path.display()))?;
    platform::put_file_on_clipboard(&path, bytes)
}
