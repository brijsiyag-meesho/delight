//! Where installed plugins come from. Plugins are shared only as source: a
//! git repo (cloned, and updated with `delight update`) or a folder on this
//! Mac (used where it is). The built files in the plugins folder are just
//! this Delight's build of them.
//!
//! Kept next to the plugins: `<plugins folder>/sources.json`, and the
//! clones in `<plugins folder>/sources/<name>/`.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};

use crate::error::{Code, Error, Result};

#[derive(Default, Serialize, Deserialize)]
pub struct Sources {
    pub sources: Vec<Source>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Source {
    /// Unique: the clone's folder, and the source's folder in the kit.
    pub name: String,
    #[serde(flatten)]
    pub origin: Origin,
    /// The installed plugins, by file name (`<name>.dylib`).
    pub plugins: Vec<String>,
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Origin {
    /// Pinned to `rev` (a tag, branch or commit), or following the default branch.
    Git {
        url: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rev: Option<String>,
    },
    Folder { path: PathBuf },
}

impl Origin {
    /// What `delight install` was given: a git URL (`https://…`, `git@…`,
    /// `….git`), maybe with `@<rev>` at the end, or a folder.
    pub fn parse(arg: &str) -> Result<Self> {
        if arg.contains("://") || arg.starts_with("git@") || arg.ends_with(".git") {
            // The `@` of `git@host:` isn't a rev: only one after the last `/` or `:` is.
            let last = arg.rfind(['/', ':']).map_or(0, |i| i + 1);
            let (url, rev) = match arg[last..].rfind('@') {
                Some(at) => (&arg[..last + at], Some(arg[last + at + 1..].to_string())),
                None => (arg, None),
            };
            return Ok(Origin::Git { url: url.to_string(), rev });
        }
        let path = std::fs::canonicalize(arg).map_err(|e| Error::new(Code::Usage, format!("{arg}: {e}")))?;
        Ok(Origin::Folder { path })
    }

    /// The same repo or folder (a git source's rev aside).
    fn same_place(&self, other: &Origin) -> bool {
        match (self, other) {
            (Origin::Git { url: a, .. }, Origin::Git { url: b, .. }) => a.trim_end_matches(".git") == b.trim_end_matches(".git"),
            (a, b) => a == b,
        }
    }

    /// `delight-plugins` for `https://github.com/Meesho/delight-plugins.git` or `~/code/delight-plugins`.
    fn base_name(&self) -> String {
        let name = match self {
            Origin::Git { url, .. } => {
                url.trim_end_matches('/').rsplit(['/', ':']).next().unwrap_or_default().trim_end_matches(".git")
            }
            Origin::Folder { path } => path.file_name().and_then(|n| n.to_str()).unwrap_or_default(),
        };
        if name.is_empty() { "plugins".into() } else { name.to_string() }
    }

    pub fn describe(&self) -> String {
        match self {
            Origin::Git { url, rev: Some(rev) } => format!("{url}@{rev}"),
            Origin::Git { url, rev: None } => url.clone(),
            Origin::Folder { path } => path.display().to_string(),
        }
    }
}

impl Source {
    /// Where its code is: the clone, or the folder.
    pub fn root(&self, plugin_dir: &Path) -> PathBuf {
        match &self.origin {
            Origin::Git { .. } => plugin_dir.join("sources").join(&self.name),
            Origin::Folder { path } => path.clone(),
        }
    }
}

impl Sources {
    pub fn load(plugin_dir: &Path) -> Result<Self> {
        let path = plugin_dir.join("sources.json");
        match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|e| Error::new(Code::Failed, format!("{} isn't valid: {e}", path.display()))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e.into()),
        }
    }

    pub fn save(&self, plugin_dir: &Path) -> Result<()> {
        let json = serde_json::to_string_pretty(self).map_err(anyhow::Error::from)?;
        std::fs::write(plugin_dir.join("sources.json"), json + "\n")?;
        Ok(())
    }

    /// The recorded source at `origin`'s place, or a new one with a name no
    /// other source has.
    pub fn find_or_new(&self, origin: &Origin) -> Source {
        if let Some(source) = self.sources.iter().find(|s| s.origin.same_place(origin)) {
            return Source { origin: origin.clone(), ..source.clone() };
        }
        let base = origin.base_name();
        let name = (1..)
            .map(|n| if n == 1 { base.clone() } else { format!("{base}-{n}") })
            .find(|name| self.sources.iter().all(|s| &s.name != name))
            .unwrap_or(base);
        Source { name, origin: origin.clone(), plugins: Vec::new() }
    }

    /// Records `source` (replacing the one of that name), or drops it when
    /// it has no plugins left.
    pub fn put(&mut self, source: Source) {
        self.sources.retain(|s| s.name != source.name);
        if !source.plugins.is_empty() {
            self.sources.push(source);
        }
    }

    /// The source a plugin file (`<name>.dylib`) was installed from.
    pub fn owner(&self, plugin: &str) -> Option<&Source> {
        self.sources.iter().find(|s| s.plugins.iter().any(|p| p == plugin))
    }
}

/// Clones a git source if it isn't yet, and checks out its pinned rev.
pub fn fetch(source: &Source, root: &Path, interactive: bool) -> Result<()> {
    let Origin::Git { url, rev } = &source.origin else { return Ok(()) };
    // A folder left from another repo (a clone that was never installed) goes.
    if root.exists() && git(root, &["remote", "get-url", "origin"], false).ok().as_deref() != Some(url) {
        std::fs::remove_dir_all(root)?;
    }
    if !root.exists() {
        let parent = root.parent().unwrap_or(root);
        std::fs::create_dir_all(parent)?;
        git(parent, &["clone", "--quiet", url, &root.to_string_lossy()], interactive)?;
    } else if rev.is_some() {
        git(root, &["fetch", "--quiet", "--tags"], interactive)?;
    }
    if let Some(rev) = rev {
        git(root, &["checkout", "--quiet", rev], interactive)?;
    }
    Ok(())
}

/// How many commits the clone's branch is behind its remote, after fetching.
pub fn commits_behind(root: &Path, interactive: bool) -> Result<u32> {
    git(root, &["fetch", "--quiet"], interactive)?;
    Ok(git(root, &["rev-list", "--count", "HEAD..@{upstream}"], false)?.parse().unwrap_or(0))
}

pub fn head(root: &Path) -> Result<String> {
    git(root, &["rev-parse", "HEAD"], false)
}

/// Moves the clone to its remote branch.
pub fn fast_forward(root: &Path) -> Result<()> {
    git(root, &["merge", "--quiet", "--ff-only", "@{upstream}"], false).map(drop)
}

/// Puts the clone back at `commit` (an update that didn't build).
pub fn reset(root: &Path, commit: &str) -> Result<()> {
    git(root, &["reset", "--quiet", "--hard", commit], false).map(drop)
}

/// The system `git` (so SSH keys and credential helpers work); it may ask
/// for a password only on a terminal.
fn git(dir: &Path, args: &[&str], interactive: bool) -> Result<String> {
    let mut command = Command::new("git");
    command.args(args).current_dir(dir).stdin(if interactive { Stdio::inherit() } else { Stdio::null() });
    if !interactive {
        command.env("GIT_TERMINAL_PROMPT", "0");
    }
    let output = command.stderr(Stdio::piped()).output().map_err(|e| {
        Error::new(Code::NoGit, format!("git: {e}")).hint("install git (e.g. `xcode-select --install`)")
    })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(Error::new(Code::GitFailed, format!("git {}: {}", args[0], stderr.trim())));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git(url: &str, rev: Option<&str>) -> Origin {
        Origin::Git { url: url.into(), rev: rev.map(String::from) }
    }

    #[test]
    fn reads_git_urls_and_revs() {
        let parse = |arg| Origin::parse(arg).ok();
        assert!(parse("https://github.com/Meesho/delight-plugins") == Some(git("https://github.com/Meesho/delight-plugins", None)));
        assert!(parse("https://github.com/Meesho/delight-plugins@v1.2") == Some(git("https://github.com/Meesho/delight-plugins", Some("v1.2"))));
        assert!(parse("git@github.com:Meesho/delight-plugins.git") == Some(git("git@github.com:Meesho/delight-plugins.git", None)));
        assert!(parse("git@github.com:Meesho/p.git@abc123") == Some(git("git@github.com:Meesho/p.git", Some("abc123"))));
    }

    #[test]
    fn names_are_unique() {
        let mut sources = Sources::default();
        let first = sources.find_or_new(&git("https://github.com/a/plugins.git", None));
        assert_eq!(first.name, "plugins");
        sources.put(Source { plugins: vec!["x".into()], ..first });
        assert_eq!(sources.find_or_new(&git("https://github.com/a/plugins", Some("v2"))).name, "plugins");
        assert_eq!(sources.find_or_new(&git("https://github.com/b/plugins", None)).name, "plugins-2");
    }
}
