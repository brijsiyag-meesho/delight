# Delight

A Spotlight-style developer toolbox for macOS, written in Rust on [GPUI](https://www.gpui.rs/) (Zed's UI framework).

Press **⌘⇧Space**, paste anything, and Delight lists the tools that understand it, best match first:

| Input | Suggested tools |
|---|---|
| `{"a": 1}` | JSON (format · minify · escape · unescape) · JSON → .env · Sign as JWT |
| `eyJhbGciOi…` | Decode JWT (claims, expiry, HS256/384/512 verification) |
| `aGVsbG8=` | Decode Base64 (standard or URL-safe; text, JSON or hex dump) |
| `DB_HOST=x` | .env → JSON (types inferred) |
| `curl https://…` | Run curl — ↵ runs it; status, time, size, Body / Headers (JSON formatted) |
| `api.example.com`, URL, IP | DNS lookup — records + TTLs, CNAME chain, MX/NS/TXT/SOA, answering (VPN-scoped) resolver, what the system resolver returns, reverse PTR |
| `8080`, `:8080`, `localhost:8080` | Port — what's listening (process, PID, user, command, uptime, connections); **Kill** ⌘↵ (SIGTERM) / **Force kill** ⌘⇧↵ (SIGKILL) the listeners only; ↵ refreshes |
| YAML (`key: value`, `- item`) / JSON | YAML → JSON (formatted; several `---` documents become an array) and JSON → YAML |
| `uuid`, `guid`, `uuid7`, `uuid v7 5`, `ulid`, `nanoid`, `objectid`, `password 24`, `secret 40` | IDs — a tab per kind (UUID v4 · v7 · ULID · NanoID · ObjectId · Password · Secret); the keyword opens the matching tab; a number is the count (length for Password / Secret); **Regenerate** ⌥2. Paste a UUID / ULID / ObjectId to decode its version and embedded time |
| anything | Encode Base64 |

Plugins add more tools — written in Rust, with any GPUI UI they like.

## Install

```sh
curl -fsSL https://github.com/brijsiyag-meesho/delight/releases/latest/download/install.sh | bash
```

Installs `Delight.app` into `/Applications` and the `delight` plugin CLI into `/usr/local/bin` (or
`~/.local/bin`), then starts Delight; run it again to update. Apple silicon, macOS 12+. Or download
`Delight-arm64.dmg` from [Releases](https://github.com/brijsiyag-meesho/delight/releases) and drag Delight
to Applications — it isn't notarized yet, so open it the first time with right-click → Open.

Developing a plugin: `delight new my-tool && cd my-tool`, edit, then `delight install --restart` to build it,
install it and restart Delight with it loaded. See [Plugins](#plugins).

## Using it

- **Open at login**: Settings → General.

- **⌘⇧Space** opens or hides the launcher. It starts as a compact bar and expands once there's input.
- The input is global: it survives hiding the window and, with "Remember input" on, restarts too.
- **Auto-paste clipboard** (Settings → General, off by default): opening Delight puts the clipboard's text into
  the input, selected — only when the clipboard changed since the last auto-paste, so your edits aren't overwritten.
- **↑ / ↓** (cursor on the first or last line), **⌥↑ / ⌥↓** or **⌘1–9** switch tools.
- **← / →** (cursor at the start or end of the input) or **⌘⇧[ / ⌘⇧]** switch the tool's mode, e.g. JSON's
  Format / Minify / Escape / Unescape.
- **↵** runs the primary action (usually Copy), **⌥2–⌥4** the others, **⇧↵** inserts a newline.
- **Tab** moves between the input and a tool's fields. **⌘K** clears. **Esc** hides. **⌘,** opens Settings.
- A tool with settings shows **⚙** in its header; so does its row in Settings → Plugins.
- Menu bar icon: Open, Settings…, Restart Delight, Quit.

```sh
cargo run                                           # dev
./scripts/bundle.sh                                 # dist/Delight.app (menu-bar only)
./scripts/package-dmg.sh                            # dist/Delight-….dmg (see Releasing)
./scripts/install-plugin.sh <package> [--release]   # build + install a plugin
cargo test --workspace
DELIGHT_LOG=info cargo run                         # log plugin loading (debug: everything)
```

Settings has two tabs: **General** (appearance, behaviour, tool detection) and **Plugins** (turn tools on or
off, open a plugin's settings ⚙, **Install…** a plugin — a `.zip` from `delight package` or a `.dylib` —, or
delete an installed plugin: its file, settings and Keychain secrets).

The launcher's input uses Zed's editor font, **Lilex** (Zed's `.ZedMono`, v2.700, SIL OFL 1.1 —
`crates/sdk/assets/fonts/lilex/OFL.txt`, bundled in the SDK), at 13px. Code and tool output use SF Mono.

Icons are [Lucide](https://lucide.dev) v1.48.0 (ISC — `crates/sdk/assets/icons/LICENSE-lucide`), embedded in
the SDK; plugins can use them by name, e.g. `delight_sdk::ui::icon("icons/settings.svg", 14., color)`.

> **Shortcut conflict:** the Claude desktop app also binds ⌘⇧Space; change or disable it there.

## Layout

```
crates/sdk            delight-sdk (dylib): tool schema, host API, GPUI re-export, native UI kit, icons
crates/gpui-alias     delight-gpui: GPUI as re-exported by the SDK (import it as `gpui`)
crates/core           registry · classifier · built-in tools · dylib loader · settings · Keychain
crates/app            the GPUI app: launcher, settings window, menu bar, hotkey, AppKit shims
examples/plugins/hello  a plugin with a custom GPUI view — start new plugins from it
crates/test-plugins/panicky  test fixture: panics on purpose (exercises the panic guards)
plugins/*             your plugins: folders or symlinks to their repos (git-ignored)
```

## Plugins

Every tool — built-in or plugin — implements one trait from `delight-sdk`:

```rust
pub trait Plugin: Send + Sync {
    fn manifest(&self) -> &PluginManifest;                      // id, name, icon, operations, settings
    fn detect(&self, input: &Input) -> Vec<Detection>;          // default: the manifest's DetectRules
    fn run(&self, req: &RunRequest) -> Result<ToolOutput, PluginError>;       // declarative result
    fn tool_view(&self, op: &str, cx: &mut App) -> Option<Box<dyn ToolView>>; // or: any GPUI UI
    fn settings_view(&self, cx: &mut App) -> Option<Box<dyn SettingsView>>;   // custom settings page
}
```

Build manifests with the constructors and builders — every SDK struct and enum is `#[non_exhaustive]`, so SDK
releases can add fields and variants without breaking plugin sources:

```rust
PluginManifest::new("example.hello", "Hello")
    .icon("👋")
    .operations([OperationSpec::new("greet", "Say hello").detect([DetectRule::prefix("hello", 0.9)])])
    .settings([FormField::secret("token", "Token").help("Stored in the Keychain")])
```

- **Declarative results** — `run` returns `Block`s (code, text, key/value, notice, inline field) and `Action`s (copy, replace input, open URL, run operation, open settings). Delight renders them natively.
- **Custom UI** — `tool_view` returns any GPUI view for the result pane, updated with the input and settings.
  Its footer actions come from `ToolView::actions`; Delight draws them (and binds ↵ / ⌥N), and
  `Action::custom(id, label)` ones call back into `ToolView::perform(id)` — the plugin decides what they do
  (e.g. a **Fetch** button that runs a request).
- **Run again** — `Action::rerun(id, label)` re-runs a declarative tool on the same input (e.g. the ID tool's
  **Regenerate**).
- **Action keys** — `Action::shortcut("cmd-enter")` gives an action its own key (shown in the footer as `⌘↵`).
  ↵ only ever goes to an action *without* a shortcut, so destructive actions (the Port tool's **Kill** ⌘↵ /
  **Force kill** ⌘⇧↵) can't be triggered by a stray ↵. `delight_sdk::{theme, ui, editor}` are Delight's own widgets, so views can match it.
- **Detection** — `DetectRule`s (`regex`, `prefix`, `json`, `always`) or custom `detect`. Matching tools are
  listed under **Suggested**, by confidence. Under **Other Tools** go only operations with
  `show_unmatched: true` — tools that take any input, where the input can't tell whether they're wanted (e.g. a
  lookup by id). Tools that recognise their input format leave it `false` and stay hidden when it doesn't
  match.
- **Settings** — declare `settings` fields in the manifest: Delight renders the plugin's Settings page, stores values in `settings.json` — `secret` fields in the Keychain (service `Delight`) — and passes them in `RunRequest::settings` / `ToolContext::settings`. `delight_sdk::host(cx)` gives views settings, toasts and "open settings".
- **Network tools** — set `run_delay_ms` on the operation so `run` waits for typing to pause.
- **Modes** — set `OperationSpec::mode` to one of its `select` params (e.g. Format / Minify / Escape): it's shown
  as tabs without a label and switched with **← / →** (when the input's cursor is at its start / end) or
  **⌘⇧[ / ⌘⇧]**.
- **Suggested mode** — `Detection::mode(value)` opens the tab that fits the input (the ID tool: `uuid7` → UUID v7).
  The host applies it when the suggestion changes; a tab the user picks stays until then.
- **Conditional options** — `FormField::show_when(ShowWhen::new("mode", &["format"]))` shows a field only while
  another field has one of those values.

A plugin is a `dylib` crate ending in `delight_sdk::export_plugin!(MyPlugin::new);`. At startup Delight loads
the plugins bundled in `Delight.app/Contents/PlugIns` ("Included" in Settings), then every `*.dylib` in
`~/Library/Application Support/Delight/plugins` (`plugins-debug` for dev builds) — a plugin there replaces an
included one with the same id. Settings → Plugins → Install… adds one without a restart; a rebuilt or
replaced plugin needs one (loaded libraries are never unloaded). Downloaded plugins are quarantined and macOS
won't load them until Install… (or `xattr -d com.apple.quarantine`) clears that.

### Writing `detect`: fast, without cutting corners

`detect` runs for **every plugin on every input change** (on a background thread, so the UI never blocks — but
slow detectors delay the suggestions, and the cost grows with the number of tools and the input size). The
rules:

1. **Reject cheaply first.** Look at the input's shape in O(1) — first character, a prefix (`curl `), the
   length (a port is ≤ 24 characters) — and return before any full parse or copy.
2. **Scan with early exits.** Check an alphabet with `bytes().all(..)` (stops at the first bad byte) rather than
   collecting/filtering the whole input first.
3. **Share parsing.** Use `input.json()` / `input.json_error()`: the input is parsed as JSON at most once per
   classification (and only if it starts like JSON), for every plugin.
4. **Bound the work.** Decide from the first lines (the `.env` and YAML detectors look at 400) and only compute
   what the preview shows (Base64 encodes 60 bytes for an 80-character preview, not the whole input).
5. **Don't trade accuracy for it.** After the cheap checks pass, confirm properly — e.g. the YAML detector still
   parses inputs up to 256 KB and requires a mapping or list, so prose like `Note: …` isn't offered as YAML.

`RuleClassifier` logs a warning for any detector slower than 20 ms (`DELIGHT_LOG=info`), and
`detection_stays_cheap_on_large_inputs` runs every built-in against 1 MB of JSON, prose, YAML and logs.

### Building plugins

Plugins run in-process and share one copy of GPUI with the app through `libdelight_sdk.dylib`. Rust has
no stable ABI, so a plugin must be compiled against **the exact SDK build** the app loads. A plugin is still
its own project — a normal crate that depends on the SDK like any crates.io crate:

```toml
[lib]
crate-type = ["dylib"]

[dependencies]
delight-sdk = "=0.1.0"
gpui = { package = "delight-gpui", version = "=0.1.0" }   # for custom views
```

It's *built* inside the **SDK kit** — the workspace that built the app's SDK. `Delight.app` carries its kit
(`Contents/Resources/sdk-kit.tar.gz`) and the `delight` CLI that uses it:

```sh
ln -s /Applications/Delight.app/Contents/Resources/bin/delight /usr/local/bin/delight   # once

delight new word-count          # a plugin project: Cargo.toml, src/lib.rs, editor support
cd word-count
delight install --restart       # build it for the installed Delight, check it loads, install, restart
delight package                 # word-count-0.1.0-sdk0.1.0-aarch64-apple-darwin.zip, to share
delight ide                     # re-point editor support after updating Delight
delight sdk                     # which SDK the installed Delight carries
```

`delight build` unpacks the kit into `~/Library/Caches/Delight/sdk/<version>-<hash>/`, links the project into
its `plugins/`, and builds there with the kit's compiler. The kit's `[patch.crates-io]` points `delight-sdk`
at its own `crates/sdk`, and Cargo derives crate identities relative to the workspace root, so the plugin
gets the app's exact SDK. It then runs the app's loader checks on the result (`Delight --check-plugin`).
The first build per SDK compiles GPUI (a few minutes); plugins share it after that. `delight ide` writes a
`.cargo/config.toml` that points `cargo check` and rust-analyzer at the kit — `delight build` is still what
builds the shipping dylib.

In this repo, plugins under `plugins/` build directly (`scripts/install-plugin.sh`), as can
`delight-sdk.workspace = true`.

- **Same profile** as the app: `--release` for the bundled app, debug for `cargo run`.
- **Only add crates that don't change the SDK build**: a crate the SDK doesn't use is fine; one it uses, with
  extra features or a different version, rebuilds the SDK differently and the plugin won't load. Use the
  SDK's re-exports (`delight_sdk::gpui`, `delight_sdk::serde_json`).
- `Delight --sdk-build-id` prints the SDK build an app carries; `Delight --check-plugin <dylib>` runs the
  app's loader checks on a plugin.

Delight binds all symbols at load time and checks the SDK version, compiler, build identity and a hash of the
SDK's sources, so a mismatched plugin is listed under "Some plugins failed to load" in Settings (e.g. "built
for Delight SDK 0.1.0, this Delight has SDK 0.2.0 — it needs a rebuild") instead of crashing the app.

### SDK versions

`delight-sdk` has its own version (`crates/sdk/Cargo.toml`), shown in Settings → General. Plugins bind to one
SDK build, so:

- **App-only changes** (launcher, settings, built-in tools, `crates/core`, the app's version) keep every
  installed plugin working.
- **SDK changes** need a rebuild of every plugin: anything in `crates/sdk`, GPUI or any other dependency
  (`Cargo.lock`), `rust-toolchain.toml`, `[profile.release]`, `.cargo/config.toml`. Bump the SDK version with
  them. Edits to `crates/sdk/src` are caught even without a bump (the source hash), so a stale plugin is
  refused rather than misreading changed types.
- Keep SDK changes additive where possible: new fields and variants on the `#[non_exhaustive]` types, new trait
  methods only with a default body, deprecate instead of removing. Plugins then only need a rebuild, no code
  changes.

### Crashes

Every call into a plugin is guarded (`crates/core/src/guard.rs`), built-ins included:

- A panic in `detect` or `run` (background threads) is caught: the plugin is turned off until Delight
  restarts, the launcher says so, and everything else keeps working.
- A panic on the main thread — `tool_view`, `ToolView::update` / `perform`, `settings_view`, or while a
  plugin's view renders — can leave GPUI mid-update, so Delight turns the plugin off, relaunches, and explains
  why on the next launch. Turn it back on in Settings → Plugins.
- Not catchable in-process: segfaults, stack overflows, aborts, and panics in a plugin's own event listeners
  or spawned tasks. Delight still dies; the next launch shows the panic message.

Try it: `./scripts/install-plugin.sh delight-plugin-panicky`, restart, and type `panic run`, `panic detect`,
`panic render` or `panic update`.

## Releasing

- `scripts/release-app.sh` — an app release that keeps the plugin SDK: builds `dist/Delight.app` and proves a
  plugin built with the current SDK kit still loads. It fails if the release changed the SDK.
- `scripts/release-sdk.sh` — an SDK release (bump `crates/sdk` and `crates/gpui-alias` versions first):
  builds the app, packages the kit, and proves it with `scripts/check-kit.sh`.
- `scripts/package-dmg.sh` — the DMG. `bundle.sh` ships your `plugins/` inside the app
  (`DELIGHT_BUNDLE_PLUGINS=0` leaves them out) and signs it with the hardened runtime; library validation is off
  (`scripts/Delight.entitlements`) so it can load plugins signed by their authors. Ad-hoc signed by default;
  for other people's Macs set `DELIGHT_SIGN_IDENTITY="Developer ID Application: …"` and
  `DELIGHT_NOTARY_PROFILE=<notarytool profile>` to sign, notarize and staple it.
- `scripts/check-kit.sh <kit> [app]` — extracts a kit elsewhere, builds a standalone plugin in it and loads it
  with the app.

## Tool selection: deterministic now, model-ready

`Router` runs `Classifier` stages and merges their scores per (plugin, operation). `RuleClassifier` (the
default) calls each plugin's `detect`; `ModelClassifier` wraps a `ModelBackend` (`fn score(input, labels)
-> Vec<f32>`) whose labels come from the manifests, so a learned classifier such as **Jev from TypeSafe AI**
can rank new plugins without host changes. Add it in `crates/app/src/state.rs::build_router`:

```rust
Router::deterministic().with_stage(ModelClassifier::new(Arc::new(JevBackend::new(..))), 0.8)
```
