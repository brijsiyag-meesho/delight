//! `delight`: installs Delight plugins from source and keeps them built for
//! this Delight. Plugins are shared only as source, a git repo or a folder;
//! the built files are this Delight's build of them, rebuilt when an update
//! changes its SDK.
//!
//! Made for people and agents alike: it asks which plugins to install only
//! on a terminal; `--json` prints one JSON object on stdout (progress goes to
//! stderr) and never asks; failures carry a stable code and a hint; exit
//! codes: 0 ok, 1 failed, 2 usage, 3 environment.
//!
//! The work (sources, building, install, update, rebuild) is
//! `delight-plugin-manager`, shared with the app; this is the command line:
//!
//! * `template` — new repos and plugin crates.
//! * `report` — output, error codes, exit codes.

mod report;
mod template;

use std::io::IsTerminal as _;
use std::path::{Path, PathBuf};

use clap::{Args, Parser, Subcommand};
use serde_json::{Value, json};

use delight_plugin_manager::project::{Member, Project};
use delight_plugin_manager::sources::{Origin, Sources};
use delight_plugin_manager::{Env, Pick, Target, UpdateStatus, kit, target};

use crate::report::{Code, Error, Outcome, Result};

const ABOUT: &str = "Install Delight plugins from source, and keep them built for this Delight.

A plugin repo is a Cargo workspace: each member is a plugin crate
([lib] crate-type = [\"dylib\"]) or a library the plugins share.";

const AFTER_HELP: &str = "Examples:
  delight install https://github.com/Meesho/delight-plugins    pick plugins from a repo
  delight list https://github.com/Meesho/delight-plugins --json   what the repo has
  delight install <repo> -p lucide -p process --restart         install two, restart Delight
  delight install                                               this folder's plugins
  delight update                                                update every git source
  delight new my-tools && cd my-tools && delight install --all  write a plugin

Output: --json prints one JSON object on stdout: {\"ok\": true, …} or
{\"ok\": false, \"error\": {\"code\", \"message\", \"hint\"}}. Progress goes to stderr.
Exit codes: 0 ok, 1 failed, 2 usage, 3 environment (no Delight, Rust or git).
Environment: DELIGHT_APP (a Delight.app or checkout), DELIGHT_CACHE.";

#[derive(Parser)]
#[command(name = "delight", version, about = ABOUT, after_help = AFTER_HELP)]
struct Cli {
    /// Print the result as one JSON object on stdout; never ask.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Command,
}

/// Which Delight to build for.
#[derive(Args)]
struct TargetArgs {
    /// A Delight.app, its binary, or a Delight checkout [default: the Delight
    /// this CLI came with]; or set DELIGHT_APP.
    #[arg(long, value_name = "PATH")]
    app: Option<PathBuf>,
}

impl TargetArgs {
    fn resolve(self) -> Result<Env> {
        Env::new(Target::resolve(self.app)?, report::progress)
    }
}

/// Plugins to act on, by name (`lucide`) or package (`delight-plugin-lucide`).
#[derive(Args)]
struct Select {
    /// Only this plugin; repeatable.
    #[arg(short = 'p', long = "package", value_name = "NAME")]
    packages: Vec<String>,
}

#[derive(Subcommand)]
enum Command {
    /// Install plugins from a git repo (URL[@rev]) or a folder [default: this
    /// repo]. Asks which on a terminal; else pass -p or --all.
    Install {
        source: Option<String>,
        #[command(flatten)]
        select: Select,
        /// Every plugin in the source.
        #[arg(long, conflicts_with = "packages")]
        all: bool,
        /// Restart a running Delight afterwards: it loads plugins only when
        /// it starts.
        #[arg(long)]
        restart: bool,
        #[command(flatten)]
        target: TargetArgs,
    },
    /// The installed plugins and their sources; with a source, the plugins it has.
    List {
        source: Option<String>,
        #[command(flatten)]
        target: TargetArgs,
    },
    /// Update plugins from their git repos (all, or the ones named), and
    /// rebuild folder sources.
    Update {
        plugins: Vec<String>,
        /// Only say what has updates.
        #[arg(long)]
        check: bool,
        #[arg(long)]
        restart: bool,
        #[command(flatten)]
        target: TargetArgs,
    },
    /// Rebuild the installed plugins this Delight can't load (after an update
    /// changed its SDK). Delight runs it itself when it starts.
    Rebuild {
        #[arg(long)]
        restart: bool,
        #[command(flatten)]
        target: TargetArgs,
    },
    /// Uninstall plugins (their settings and data stay; Settings → Plugins
    /// deletes those too).
    Remove {
        #[arg(required = true)]
        plugins: Vec<String>,
        #[command(flatten)]
        target: TargetArgs,
    },
    /// Build this repo's plugins and check Delight loads them, without installing.
    Build {
        #[command(flatten)]
        select: Select,
        #[command(flatten)]
        target: TargetArgs,
    },
    /// Create a plugin repo with one plugin crate.
    New {
        /// The repo's folder (created).
        repo: PathBuf,
        /// The first plugin's name [default: the folder's name].
        #[arg(long)]
        plugin: Option<String>,
        /// The plugin's id [default: local.<plugin>].
        #[arg(long)]
        id: Option<String>,
        #[command(flatten)]
        target: TargetArgs,
    },
    /// Add a plugin crate to this repo.
    Add {
        name: String,
        /// The plugin's id [default: local.<name>].
        #[arg(long)]
        id: Option<String>,
        #[command(flatten)]
        target: TargetArgs,
    },
    /// Which Delight, SDK, profile, plugins folder and Rust apply.
    Info {
        #[command(flatten)]
        target: TargetArgs,
    },
    /// Remove the SDK kit and build cache.
    Clean,
}

fn main() {
    let json = std::env::args().any(|a| a == "--json");
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(e) if !e.use_stderr() => e.exit(), // --help, --version
        Err(e) if json => {
            let message = e.to_string().lines().next().unwrap_or_default().trim_start_matches("error: ").to_string();
            let error = Error::new(Code::Usage, message).hint("see `delight --help`");
            std::process::exit(report::finish(Err(error), true));
        }
        Err(e) => e.exit(),
    };
    // Asking (which plugins, a git password) needs a person at a terminal.
    let interactive = !cli.json && std::io::stdin().is_terminal() && std::io::stderr().is_terminal();
    std::process::exit(report::finish(run(cli.command, interactive), cli.json));
}

fn run(command: Command, interactive: bool) -> Result<Outcome> {
    let cwd = std::env::current_dir()?;
    match command {
        Command::Install { source, select, all, restart, target } => {
            install(&target.resolve()?, &cwd, source, &select.packages, all, restart, interactive)
        }
        Command::List { source: None, target } => list_installed(&target.resolve()?),
        Command::List { source: Some(source), target } => list_source(&target.resolve()?, &source, interactive),
        Command::Update { plugins, check, restart, target } => update(&target.resolve()?, &plugins, check, restart, interactive),
        Command::Rebuild { restart, target } => rebuild(&target.resolve()?, restart),
        Command::Remove { plugins, target } => remove(&target.resolve()?, &plugins),
        Command::Build { select, target } => {
            let env = target.resolve()?;
            let project = Project::find(&cwd)?;
            let name = project.root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let built = kit::build(&env, &project, &name, &project.plugins(&select.packages)?)?;
            let mut plugins = Vec::new();
            let mut summary = Vec::new();
            for b in &built {
                let dylib = project.root.join("target/delight").join(format!("{}.dylib", b.member.file_stem()));
                kit::replace_file(&b.dylib, &dylib)?;
                summary.push(format!("built {} → {} ({} {})", b.member.package, dylib.display(), b.id, b.version));
                plugins.push(json!({ "package": b.member.package, "id": b.id, "version": b.version, "dylib": dylib }));
            }
            Ok(Outcome { data: json!({ "plugins": plugins }), summary })
        }
        Command::New { repo, plugin, id, target } => {
            let env = target.resolve()?;
            let folder = repo.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let plugin = plugin.unwrap_or(folder);
            template::check_name(&plugin)?;
            let id = id.unwrap_or_else(|| format!("local.{plugin}"));
            let sdk = env.info.sdk_version().to_string();
            template::new_repo(&repo, &plugin, &id, &sdk)?;
            let data = json!({ "repo": absolute(&repo), "plugin": plugin, "id": id, "sdk_version": sdk });
            Ok(Outcome {
                data,
                summary: vec![
                    format!("created {} with plugin {plugin} (id {id})", repo.display()),
                    format!("next: cd {} && delight install --all", repo.display()),
                ],
            })
        }
        Command::Add { name, id, target } => {
            let env = target.resolve()?;
            template::check_name(&name)?;
            let project = Project::find(&cwd)?;
            let id = id.unwrap_or_else(|| format!("local.{name}"));
            template::add_plugin(&project.root, &name, &id, env.info.sdk_version())?;
            let path = project.root.join(&name);
            Ok(Outcome {
                data: json!({ "plugin": name, "id": id, "path": path }),
                summary: vec![format!("added {} (id {id})", path.display())],
            })
        }
        Command::Info { target } => {
            let Env { target, info, .. } = target.resolve()?;
            // The compiler plugins build with: the kit's toolchain.
            let kit = kit::kit_dir();
            target.write_sdk_kit(&kit)?;
            let rust = target::rust_version(&kit)?;
            let summary = vec![
                format!("Delight: {}", target.describe()),
                format!("SDK:     {}", info.sdk_build_id),
                format!("Profile: {}", info.profile),
                format!("Plugins: {}", info.plugin_dir.display()),
                format!("Rust:    {rust}"),
                format!("Kit:     {}", kit.display()),
            ];
            let data = json!({
                "delight": target.describe(),
                "sdk_build_id": info.sdk_build_id,
                "sdk_version": info.sdk_version(),
                "profile": info.profile,
                "plugin_dir": info.plugin_dir,
                "rust": rust,
                "kit": kit,
            });
            Ok(Outcome { data, summary })
        }
        Command::Clean => {
            let root = kit::cache_root();
            let mut removed = Vec::new();
            for dir in ["kit", "target"].map(|d| root.join(d)).into_iter().filter(|d| d.is_dir()) {
                std::fs::remove_dir_all(&dir)?;
                removed.push(dir);
            }
            let summary = if removed.is_empty() { vec!["nothing to clean".into()] } else { removed.iter().map(|d| format!("removed {}", d.display())).collect() };
            Ok(Outcome { data: json!({ "removed": removed }), summary })
        }
    }
}

/// Installs plugins from a source: those asked for with `-p` (added to what's
/// installed from it), every one (`--all`), or those picked on a terminal
/// (what's picked is what stays installed from it).
fn install(
    env: &Env,
    cwd: &Path,
    source: Option<String>,
    packages: &[String],
    all: bool,
    restart: bool,
    interactive: bool,
) -> Result<Outcome> {
    let origin = match source {
        Some(arg) => Origin::parse(&arg)?,
        None => Origin::Folder { path: Project::find(cwd)?.root },
    };
    let opened = delight_plugin_manager::open(env, &origin, interactive)?;
    let pick = if all {
        Pick::All
    } else if !packages.is_empty() {
        Pick::Add(packages.to_vec())
    } else if interactive {
        match choose(&opened.available(), |m| opened.installed(m))? {
            Some(picked) => Pick::Exactly(picked),
            None => return Ok(Outcome { data: json!({ "plugins": [] }), summary: vec!["nothing changed".into()] }),
        }
    } else {
        let names: Vec<&str> = opened.available().iter().map(|m| m.file_stem()).collect();
        return Err(Error::new(Code::Usage, "choose the plugins to install")
            .hint(format!("pass -p <name> (repeatable) or --all; plugins: {}", names.join(", "))));
    };
    let report = delight_plugin_manager::install(env, opened, pick)?;
    let mut summary: Vec<String> =
        report.installed.iter().map(|p| format!("installed {} {} → {}", p.id, p.version, p.file.display())).collect();
    summary.extend(report.removed.iter().map(|p| format!("removed {p}")));
    let plugins: Vec<Value> = report
        .installed
        .iter()
        .map(|p| json!({ "name": p.name, "id": p.id, "version": p.version, "installed": p.file }))
        .collect();
    let changed = !report.installed.is_empty() || !report.removed.is_empty();
    let mut data = json!({ "plugins": plugins, "removed": report.removed });
    after_install(env, restart, changed, &mut data, &mut summary)?;
    Ok(Outcome { data, summary })
}

/// Asks which plugins to install; installed ones start ticked. Their names,
/// or `None` when cancelled.
fn choose(available: &[&Member], installed: impl Fn(&Member) -> bool) -> Result<Option<Vec<String>>> {
    let items = available.iter().map(|m| match m.description.as_str() {
        "" => m.file_stem().to_string(),
        description => format!("{} — {description}", m.file_stem()),
    });
    let ticked: Vec<bool> = available.iter().map(|m| installed(m)).collect();
    let picked = dialoguer::MultiSelect::new()
        .with_prompt("Plugins to install (space to select, enter to confirm)")
        .items(items)
        .defaults(&ticked)
        .interact_opt()
        .map_err(|e| Error::new(Code::Failed, e.to_string()))?;
    Ok(picked.map(|picked| picked.into_iter().map(|i| available[i].file_stem().to_string()).collect()))
}

fn list_installed(env: &Env) -> Result<Outcome> {
    let sources = Sources::load(&env.info.plugin_dir)?;
    let summary = if sources.sources.is_empty() {
        vec!["no plugins installed from source".into()]
    } else {
        sources.sources.iter().map(|s| format!("{} ({}): {}", s.name, s.origin.describe(), s.plugins.join(", "))).collect()
    };
    let listed = serde_json::to_value(&sources.sources).map_err(anyhow::Error::from)?;
    let data = json!({ "plugin_dir": env.info.plugin_dir, "sources": listed });
    Ok(Outcome { data, summary })
}

fn list_source(env: &Env, arg: &str, interactive: bool) -> Result<Outcome> {
    let opened = delight_plugin_manager::open(env, &Origin::parse(arg)?, interactive)?;
    let mut summary = Vec::new();
    let mut plugins = Vec::new();
    for member in opened.available() {
        let installed = opened.installed(member);
        summary.push(format!("{}{}  {}", member.file_stem(), if installed { " (installed)" } else { "" }, member.description));
        let (name, package, description) = (member.file_stem(), &member.package, &member.description);
        plugins.push(json!({ "name": name, "package": package, "description": description, "installed": installed }));
    }
    Ok(Outcome { data: json!({ "source": opened.source.origin.describe(), "plugins": plugins }), summary })
}

fn update(env: &Env, plugins: &[String], check: bool, restart: bool, interactive: bool) -> Result<Outcome> {
    let updates = delight_plugin_manager::update(env, plugins, check, interactive)?;
    let mut summary = Vec::new();
    let mut results = Vec::new();
    for update in &updates {
        let status = update.status.as_str().replace('_', " ");
        summary.push(match (&update.status, update.commits) {
            (UpdateStatus::Failed(error), _) => format!("{}: failed: {}", update.name, error.message),
            (_, 0) => format!("{}: {status}", update.name),
            (_, 1) => format!("{}: {status} (1 new commit)", update.name),
            (_, n) => format!("{}: {status} ({n} new commits)", update.name),
        });
        let error = match &update.status {
            UpdateStatus::Failed(error) => Some(error.to_json()),
            _ => None,
        };
        results.push(json!({ "name": update.name, "status": update.status.as_str(), "commits": update.commits, "error": error }));
    }
    if updates.is_empty() {
        summary.push("no plugins installed from source".into());
    }
    let changed = updates.iter().any(|u| u.status.changed());
    let mut data = json!({ "sources": results });
    after_install(env, restart, changed, &mut data, &mut summary)?;
    Ok(Outcome { data, summary })
}

fn rebuild(env: &Env, restart: bool) -> Result<Outcome> {
    let report = delight_plugin_manager::rebuild(env)?;
    let mut summary: Vec<String> = report.rebuilt.iter().map(|p| format!("rebuilt {p}")).collect();
    summary.extend(report.failed.iter().map(|f| format!("couldn't rebuild {}: {}", f.plugins.join(", "), f.error.message)));
    summary.extend(report.forgotten.iter().map(|p| format!("forgot {p} (its file is gone)")));
    let unmanaged = |p: &String| format!("can't rebuild {p}: not installed from a source (install it again with `delight install`)");
    summary.extend(report.unmanaged.iter().map(unmanaged));
    if summary.is_empty() {
        summary.push("every plugin is built for this Delight".into());
    }
    let failed: Vec<Value> =
        report.failed.iter().map(|f| json!({ "source": f.source, "plugins": f.plugins, "error": f.error.to_json() })).collect();
    let mut data = json!({ "rebuilt": report.rebuilt, "failed": failed, "forgotten": report.forgotten, "unmanaged": report.unmanaged });
    after_install(env, restart, !report.rebuilt.is_empty(), &mut data, &mut summary)?;
    Ok(Outcome { data, summary })
}

fn remove(env: &Env, plugins: &[String]) -> Result<Outcome> {
    delight_plugin_manager::remove(env, plugins)?;
    let mut summary: Vec<String> = plugins.iter().map(|p| format!("removed {p}")).collect();
    summary.push("restart Delight to unload it".into());
    Ok(Outcome { data: json!({ "removed": plugins }), summary })
}

/// A running Delight loads plugins only when it starts: restarts it, or
/// says to.
fn after_install(env: &Env, restart: bool, changed: bool, data: &mut Value, summary: &mut Vec<String>) -> Result<()> {
    let running = if changed { env.target.info()?.running } else { None };
    let restarted = match running {
        Some(pid) if restart => {
            env.target.restart(pid)?;
            summary.push("restarted Delight".into());
            true
        }
        Some(_) => {
            summary.push("restart Delight to use it (or pass --restart)".into());
            false
        }
        None => false,
    };
    data["running"] = json!(running.is_some());
    data["restarted"] = json!(restarted);
    Ok(())
}

fn absolute(path: &Path) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
}
