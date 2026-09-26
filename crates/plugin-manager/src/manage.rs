//! Installing, updating, rebuilding and removing plugins from their sources.

use std::path::Path;

use crate::error::{Code, Error, Result};
use crate::project::{Member, Project};
use crate::sources::{self, Origin, Source, Sources};
use crate::{Env, kit};

/// A source, cloned if it's a git repo, with the record of what's installed
/// from it.
pub struct Opened {
    pub sources: Sources,
    pub source: Source,
    pub project: Project,
}

impl Opened {
    /// The plugin crates it has.
    pub fn available(&self) -> Vec<&Member> {
        self.project.members.iter().filter(|m| m.plugin).collect()
    }

    pub fn installed(&self, member: &Member) -> bool {
        self.source.plugins.iter().any(|p| p == member.file_stem())
    }
}

pub fn open(env: &Env, origin: &Origin, interactive: bool) -> Result<Opened> {
    let dir = &env.info.plugin_dir;
    let sources = Sources::load(dir)?;
    let source = sources.find_or_new(origin);
    let root = source.root(dir);
    sources::fetch(&source, &root, interactive)?;
    let project = Project::open(&root)?;
    Ok(Opened { sources, source, project })
}

/// Which plugins of a source to install, by name or package.
pub enum Pick {
    All,
    /// These, next to what's installed from it already.
    Add(Vec<String>),
    /// Exactly these: the others installed from it are removed.
    Exactly(Vec<String>),
}

pub struct Installed {
    pub name: String,
    pub id: String,
    pub version: String,
    pub file: std::path::PathBuf,
}

pub struct InstallReport {
    pub installed: Vec<Installed>,
    pub removed: Vec<String>,
}

/// Builds and installs the plugins picked from an opened source, and
/// records them.
pub fn install(env: &Env, opened: Opened, pick: Pick) -> Result<InstallReport> {
    let Opened { mut sources, mut source, project } = opened;
    let available = project.plugins(&[])?;
    let installed = |m: &&Member| source.plugins.iter().any(|p| p == m.file_stem());
    let wanted: Vec<&Member> = match &pick {
        Pick::All => available.clone(),
        Pick::Add(names) => {
            let named = project.plugins(names)?;
            available.iter().copied().filter(|m| installed(m) || named.iter().any(|n| n.package == m.package)).collect()
        }
        Pick::Exactly(names) => project.plugins(names)?,
    };
    // `Add` builds only the ones named, unless the source moved to another rev.
    let moved = sources.sources.iter().any(|s| s.name == source.name && s.origin != source.origin);
    let to_build = match &pick {
        Pick::Add(names) if !moved => project.plugins(names)?,
        _ => wanted.clone(),
    };
    for member in &to_build {
        if let Some(other) = sources.owner(member.file_stem()).filter(|s| s.name != source.name) {
            let message = format!("{} is already installed from {}", member.file_stem(), other.origin.describe());
            return Err(Error::new(Code::Usage, message).hint(format!("remove it first: delight remove {}", member.file_stem())));
        }
    }

    let mut report = InstallReport { installed: Vec::new(), removed: Vec::new() };
    if !to_build.is_empty() {
        for built in kit::build(env, &project, &source.name, &to_build)? {
            let file = env.plugin_file(built.member.file_stem());
            kit::replace_file(&built.dylib, &file)?;
            let name = built.member.file_stem().to_string();
            report.installed.push(Installed { name, id: built.id, version: built.version, file });
        }
    }
    let kept: Vec<String> = wanted.iter().map(|m| m.file_stem().to_string()).collect();
    for plugin in source.plugins.iter().filter(|p| !kept.contains(p)) {
        let _ = std::fs::remove_file(env.plugin_file(plugin));
        report.removed.push(plugin.clone());
    }
    source.plugins = kept;
    forget_if_empty(&source, &env.info.plugin_dir);
    sources.put(source);
    sources.save(&env.info.plugin_dir)?;
    Ok(report)
}

/// What updating a source did.
pub enum UpdateStatus {
    UpToDate,
    /// New commits, not taken (`check`).
    Available,
    Updated,
    /// Pinned to a rev: never updated.
    Pinned,
    /// A folder, built again.
    Rebuilt,
    /// A folder (`check`: nothing to compare).
    Folder,
    /// Its new code didn't build: it's back where it was, and its plugins stay.
    Failed(Error),
}

impl UpdateStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            UpdateStatus::UpToDate => "up_to_date",
            UpdateStatus::Available => "available",
            UpdateStatus::Updated => "updated",
            UpdateStatus::Pinned => "pinned",
            UpdateStatus::Rebuilt => "rebuilt",
            UpdateStatus::Folder => "folder",
            UpdateStatus::Failed(_) => "failed",
        }
    }

    /// New plugin files were installed.
    pub fn changed(&self) -> bool {
        matches!(self, UpdateStatus::Updated | UpdateStatus::Rebuilt)
    }
}

pub struct SourceUpdate {
    pub name: String,
    pub status: UpdateStatus,
    /// New commits taken (or available, with `check`).
    pub commits: u32,
}

/// Updates the git sources that aren't pinned (all, or those of `plugins`),
/// and rebuilds folder sources; with `check`, only says what has updates.
pub fn update(env: &Env, plugins: &[String], check: bool, interactive: bool) -> Result<Vec<SourceUpdate>> {
    let sources = Sources::load(&env.info.plugin_dir)?;
    if let Some(plugin) = plugins.iter().find(|p| sources.owner(p).is_none()) {
        return Err(Error::new(Code::Usage, format!("{plugin} isn't installed from a source")).hint("see `delight list`"));
    }
    let chosen = sources.sources.iter().filter(|s| plugins.is_empty() || s.plugins.iter().any(|p| plugins.contains(p)));
    Ok(chosen
        .map(|source| {
            let (status, commits) = update_source(env, source, check, interactive).unwrap_or_else(|e| (UpdateStatus::Failed(e), 0));
            SourceUpdate { name: source.name.clone(), status, commits }
        })
        .collect())
}

fn update_source(env: &Env, source: &Source, check: bool, interactive: bool) -> Result<(UpdateStatus, u32)> {
    let root = source.root(&env.info.plugin_dir);
    match &source.origin {
        Origin::Git { rev: Some(_), .. } => Ok((UpdateStatus::Pinned, 0)),
        Origin::Git { rev: None, .. } => {
            let behind = sources::commits_behind(&root, interactive)?;
            if behind == 0 {
                return Ok((UpdateStatus::UpToDate, 0));
            }
            if check {
                return Ok((UpdateStatus::Available, behind));
            }
            let old = sources::head(&root)?;
            sources::fast_forward(&root)?;
            if let Err(e) = build_and_install(env, source, &root, &source.plugins) {
                sources::reset(&root, &old)?;
                return Err(e);
            }
            Ok((UpdateStatus::Updated, behind))
        }
        Origin::Folder { .. } if check => Ok((UpdateStatus::Folder, 0)),
        Origin::Folder { .. } => build_and_install(env, source, &root, &source.plugins).map(|()| (UpdateStatus::Rebuilt, 0)),
    }
}

pub struct RebuildFailure {
    pub source: String,
    pub plugins: Vec<String>,
    pub error: Error,
}

#[derive(Default)]
pub struct RebuildReport {
    pub rebuilt: Vec<String>,
    pub failed: Vec<RebuildFailure>,
    /// Recorded plugins whose file is gone (deleted in Settings): forgotten.
    pub forgotten: Vec<String>,
    /// Plugin files that don't load and weren't installed from a source.
    pub unmanaged: Vec<String>,
}

/// Rebuilds the installed plugins this Delight can't load (after an update
/// changed its SDK).
pub fn rebuild(env: &Env) -> Result<RebuildReport> {
    let dir = &env.info.plugin_dir;
    let mut sources = Sources::load(dir)?;
    let mut report = RebuildReport::default();
    for mut source in sources.sources.clone() {
        let (present, gone): (Vec<String>, Vec<String>) = source.plugins.iter().cloned().partition(|p| env.plugin_file(p).is_file());
        report.forgotten.extend(gone);
        source.plugins = present;
        let mut stale = Vec::new();
        for plugin in &source.plugins {
            if env.target.check(&env.plugin_file(plugin))?.is_err() {
                stale.push(plugin.clone());
            }
        }
        if !stale.is_empty() {
            env.progress(&format!("rebuilding {} from {}", stale.join(", "), source.origin.describe()));
            match build_and_install(env, &source, &source.root(dir), &stale) {
                Ok(()) => report.rebuilt.extend(stale),
                Err(error) => report.failed.push(RebuildFailure { source: source.name.clone(), plugins: stale, error }),
            }
        }
        forget_if_empty(&source, dir);
        sources.put(source);
    }
    sources.save(dir)?;
    for file in std::fs::read_dir(dir)?.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "dylib")) {
        let name = file.file_stem().unwrap_or_default().to_string_lossy().into_owned();
        if sources.owner(&name).is_none() && env.target.check(&file)?.is_err() {
            report.unmanaged.push(name);
        }
    }
    Ok(report)
}

/// Uninstalls plugins: their files, and their record (a clone with nothing
/// left installed goes too).
pub fn remove(env: &Env, plugins: &[String]) -> Result<()> {
    let dir = &env.info.plugin_dir;
    let mut sources = Sources::load(dir)?;
    if let Some(plugin) = plugins.iter().find(|p| sources.owner(p).is_none() && !env.plugin_file(p).is_file()) {
        return Err(Error::new(Code::Usage, format!("{plugin} isn't installed")).hint("see `delight list`"));
    }
    for plugin in plugins {
        let _ = std::fs::remove_file(env.plugin_file(plugin));
        if let Some(mut source) = sources.owner(plugin).cloned() {
            source.plugins.retain(|p| p != plugin);
            forget_if_empty(&source, dir);
            sources.put(source);
        }
    }
    sources.save(dir)
}

/// Builds `plugins` of a source and installs them.
fn build_and_install(env: &Env, source: &Source, root: &Path, plugins: &[String]) -> Result<()> {
    let project = Project::open(root)?;
    for built in kit::build(env, &project, &source.name, &project.plugins(plugins)?)? {
        kit::replace_file(&built.dylib, &env.plugin_file(built.member.file_stem()))?;
    }
    Ok(())
}

/// A git source with no plugins left: its clone goes too.
fn forget_if_empty(source: &Source, plugin_dir: &Path) {
    if source.plugins.is_empty() && matches!(source.origin, Origin::Git { .. }) {
        let _ = std::fs::remove_dir_all(source.root(plugin_dir));
    }
}
