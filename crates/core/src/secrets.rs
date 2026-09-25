//! Plugin secrets in the macOS login Keychain: service `Delight`, account
//! `<plugin id>/<key>`, through Security.framework.
//!
//! An item's access list names the app that created it by its code
//! signature, so only Delight reads it without a prompt. An ad-hoc signed
//! build has a new signature every build: after a rebuild or an update,
//! macOS asks once before Delight can read its secrets again. A Developer ID
//! signature is stable across updates.

use anyhow::ensure;
use security_framework::base::Error;
use security_framework::item::{ItemClass, ItemSearchOptions, Limit};
use security_framework::passwords::{PasswordOptions, delete_generic_password, generic_password, set_generic_password};

use crate::plugin_store::valid_plugin_id;

const SERVICE: &str = "Delight";

/// `errSecItemNotFound`: no such item.
const NOT_FOUND: i32 = -25300;

fn account(plugin_id: &str, key: &str) -> anyhow::Result<String> {
    ensure!(valid_plugin_id(plugin_id), "invalid plugin id {plugin_id:?}");
    Ok(format!("{plugin_id}/{key}"))
}

fn not_found(e: &Error) -> bool {
    e.code() == NOT_FOUND
}

/// The secret; `None` if it isn't set.
pub fn get(plugin_id: &str, key: &str) -> anyhow::Result<Option<String>> {
    match generic_password(PasswordOptions::new_generic_password(SERVICE, &account(plugin_id, key)?)) {
        Ok(bytes) => Ok(Some(String::from_utf8(bytes)?)),
        Err(e) if not_found(&e) => Ok(None),
        Err(e) => Err(anyhow::anyhow!("Keychain: {e}")),
    }
}

/// Stores `value`, replacing any previous one; an empty value deletes it.
pub fn set(plugin_id: &str, key: &str, value: &str) -> anyhow::Result<()> {
    let account = account(plugin_id, key)?;
    let result = if value.is_empty() {
        delete_generic_password(SERVICE, &account)
    } else {
        set_generic_password(SERVICE, &account, value.as_bytes())
    };
    match result {
        Err(e) if !not_found(&e) => Err(anyhow::anyhow!("Keychain: {e}")),
        _ => Ok(()),
    }
}

/// Deletes every secret a plugin stored (the plugin was deleted).
pub fn forget_plugin(plugin_id: &str) -> anyhow::Result<()> {
    let prefix = account(plugin_id, "")?;
    let found = ItemSearchOptions::new()
        .class(ItemClass::generic_password())
        .service(SERVICE)
        .load_attributes(true)
        .limit(Limit::All)
        .search();
    let items = match found {
        Ok(items) => items,
        Err(e) if not_found(&e) => return Ok(()),
        Err(e) => return Err(anyhow::anyhow!("Keychain: {e}")),
    };
    let accounts = items.iter().filter_map(|item| item.simplify_dict()?.remove("acct"));
    for account in accounts.filter(|a| a.starts_with(&prefix)) {
        match delete_generic_password(SERVICE, &account) {
            Err(e) if !not_found(&e) => return Err(anyhow::anyhow!("Keychain: {e}")),
            _ => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Uses the real login Keychain, so it only runs when asked:
    /// `cargo test -p delight-core -- --ignored secrets`.
    #[test]
    #[ignore]
    fn set_get_replace_delete() {
        let (plugin, key) = ("delight.test", format!("secret-{}", std::process::id()));
        assert_eq!(get(plugin, &key).unwrap(), None);
        set(plugin, &key, "first").unwrap();
        set(plugin, &key, "sécond").unwrap();
        assert_eq!(get(plugin, &key).unwrap().as_deref(), Some("sécond"));
        set(plugin, &key, "").unwrap();
        assert_eq!(get(plugin, &key).unwrap(), None);
        set(plugin, &key, "").unwrap(); // deleting what isn't there is fine
        assert!(set("../evil", &key, "x").is_err());
    }

    /// Real Keychain, like the test above.
    #[test]
    #[ignore]
    fn forgets_every_secret_of_one_plugin() {
        let (gone, kept) = ("delight.test-gone", "delight.test-kept");
        set(gone, "a", "1").unwrap();
        set(gone, "b", "2").unwrap();
        set(kept, "a", "3").unwrap();
        forget_plugin(gone).unwrap();
        assert_eq!((get(gone, "a").unwrap(), get(gone, "b").unwrap()), (None, None));
        assert_eq!(get(kept, "a").unwrap().as_deref(), Some("3"));
        set(kept, "a", "").unwrap();
    }
}
