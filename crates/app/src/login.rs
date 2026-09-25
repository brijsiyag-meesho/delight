//! "Open at login": a per-user LaunchAgent (`~/Library/LaunchAgents`) that
//! starts this Delight when the user logs in. It works whatever the app's
//! signature (unlike `SMAppService`), and launchd reads it only at login, so
//! writing it doesn't start a second instance now.

use std::path::PathBuf;

const LABEL: &str = "dev.delight.app";

fn plist_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join("Library/LaunchAgents").join(format!("{LABEL}.plist")))
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// Writes or removes the LaunchAgent. Called on every launch too, so the
/// agent follows the app when it's moved (or updated in place).
pub fn apply(enabled: bool) {
    let Some(path) = plist_path() else { return };
    if !enabled {
        if path.exists()
            && let Err(e) = std::fs::remove_file(&path)
        {
            log::error!("removing {}: {e}", path.display());
        }
        return;
    }
    let Ok(exe) = std::env::current_exe() else { return };
    let plist = plist_for(&exe.to_string_lossy());
    if std::fs::read_to_string(&path).is_ok_and(|current| current == plist) {
        return;
    }
    let result = path.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|()| std::fs::write(&path, plist));
    if let Err(e) = result {
        log::error!("writing {}: {e}", path.display());
    }
}

fn plist_for(exe: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key><string>{LABEL}</string>
    <key>ProgramArguments</key><array><string>{}</string></array>
    <key>RunAtLoad</key><true/>
    <key>ProcessType</key><string>Interactive</string>
    <key>LimitLoadToSessionType</key><string>Aqua</string>
</dict>
</plist>
"#,
        xml_escape(exe)
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn plist_is_valid_even_with_odd_paths() {
        let path = std::env::temp_dir().join(format!("delight-login-{}.plist", std::process::id()));
        std::fs::write(&path, super::plist_for("/Applications/R&D <tools>/Delight.app/Contents/MacOS/Delight")).unwrap();
        let lint = std::process::Command::new("plutil").arg("-lint").arg(&path).output().unwrap();
        let exe = std::process::Command::new("plutil")
            .args(["-extract", "ProgramArguments.0", "raw", "-o", "-"])
            .arg(&path)
            .output()
            .unwrap();
        let _ = std::fs::remove_file(&path);
        assert!(lint.status.success(), "{}", String::from_utf8_lossy(&lint.stdout));
        assert_eq!(String::from_utf8_lossy(&exe.stdout).trim(), "/Applications/R&D <tools>/Delight.app/Contents/MacOS/Delight");
    }
}
