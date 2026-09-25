//! Plugin secrets in the macOS login Keychain (service `Delight`, account
//! `<plugin id>/<key>`), via `/usr/bin/security`. Items saved under the app's
//! former name (service `DevLight`) move over when first read.
//!
//! Values are written through `security -i` on stdin, hex-encoded, so they
//! never appear in a process's arguments. Going through the system tool also
//! keeps the item's access list stable across app rebuilds (no prompts).

use std::io::Write;
use std::process::{Command, Stdio};

const SERVICE: &str = "Delight";

/// The service under the app's former name; items are moved on first read.
const LEGACY_SERVICE: &str = "DevLight";

fn account(plugin_id: &str, key: &str) -> String {
    format!("{plugin_id}/{key}")
}

fn find(service: &str, account: &str) -> Option<String> {
    let out = Command::new("/usr/bin/security")
        .args(["find-generic-password", "-s", service, "-a", account, "-w"])
        .stderr(Stdio::null())
        .output()
        .ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim_end_matches('\n').to_string())
}

fn delete(service: &str, account: &str) {
    let _ = Command::new("/usr/bin/security")
        .args(["delete-generic-password", "-s", service, "-a", account])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

pub fn get(plugin_id: &str, key: &str) -> Option<String> {
    let account = account(plugin_id, key);
    if let Some(value) = find(SERVICE, &account) {
        return Some(value);
    }
    // Saved before the rename: move it to the new service.
    let value = find(LEGACY_SERVICE, &account)?;
    match set(plugin_id, key, &value) {
        Ok(()) => delete(LEGACY_SERVICE, &account),
        Err(e) => log::error!("moving secret {account} to the {SERVICE} Keychain service: {e:#}"),
    }
    Some(value)
}

/// Stores `value`; an empty value deletes the item.
pub fn set(plugin_id: &str, key: &str, value: &str) -> anyhow::Result<()> {
    let account = account(plugin_id, key);
    if value.is_empty() {
        delete(SERVICE, &account);
        delete(LEGACY_SERVICE, &account);
        return Ok(());
    }
    anyhow::ensure!(!account.contains(['"', '\n']), "invalid secret name {account:?}");
    let hex: String = value.bytes().map(|b| format!("{b:02x}")).collect();
    let mut child = Command::new("/usr/bin/security")
        .arg("-i")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;
    let command = format!("add-generic-password -U -s {SERVICE} -a \"{account}\" -X {hex}\n");
    child.stdin.take().expect("piped").write_all(command.as_bytes())?;
    let out = child.wait_with_output()?;
    let err = String::from_utf8_lossy(&out.stderr);
    anyhow::ensure!(out.status.success() && err.trim().is_empty(), "Keychain: {}", err.trim());
    Ok(())
}
