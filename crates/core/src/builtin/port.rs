//! Type a port (`8080`, `:8080`, `localhost:8080`): what's listening on it
//! and what's connected, with Kill (SIGTERM) / Force kill (SIGKILL) actions.
//!
//! Uses `lsof` (machine-readable `-F` output) and `ps`; like `lsof` without
//! sudo it only sees the current user's processes. Only processes *listening*
//! on the port are killed — never clients merely connected to it, and never
//! Delight itself.

use std::collections::BTreeMap;
use std::process::Command;
use std::time::Duration;

use delight_sdk::gpui::{self, AnyView, App, AppContext, Context, Entity, Task, Window, div, prelude::*, px};
use delight_sdk::theme::{Theme, theme};
use delight_sdk::ui;
use delight_sdk::{Action, Detection, Input, Plugin, PluginManifest, ToolContext, ToolView};

use super::{manifest, op};

pub struct PortPlugin {
    manifest: PluginManifest,
}

impl PortPlugin {
    pub fn new() -> Self {
        Self {
            manifest: manifest(
                "delight.port",
                "Port",
                "See what's using a port, and kill it.",
                ":80",
                "#FF3B30",
                &["port", "lsof", "kill", "process", "network"],
                vec![op("inspect", "Port", "What's listening on a port; kill it", &["port", "lsof", "kill"], vec![])],
            ),
        }
    }
}

impl Default for PortPlugin {
    fn default() -> Self {
        Self::new()
    }
}

/// The port in `8080`, `:8080`, `localhost:8080`, `127.0.0.1:8080`, `port 8080`.
fn parse_port(input: &str) -> Option<u16> {
    let t = input.trim();
    // `localhost:65535` is the longest form: reject anything longer before copying.
    if t.len() > 24 {
        return None;
    }
    let t = t.to_ascii_lowercase();
    let t = t.strip_prefix("port").map_or(t.as_str(), str::trim_start);
    let digits = match t.rsplit_once(':') {
        Some(("" | "localhost" | "127.0.0.1" | "0.0.0.0" | "*" | "[::1]" | "[::]", p)) => p,
        Some(_) => return None,
        None => t,
    };
    (!digits.is_empty() && digits.len() <= 5 && digits.bytes().all(|b| b.is_ascii_digit()))
        .then(|| digits.parse::<u16>().ok())
        .flatten()
        .filter(|p| *p > 0)
}

impl Plugin for PortPlugin {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    fn detect(&self, input: &Input) -> Vec<Detection> {
        let Some(port) = parse_port(input.trimmed) else { return Vec::new() };
        // A bare number could be anything (an id, a count); `:8080` is a port.
        let explicit = input.trimmed.contains(':') || input.trimmed.to_ascii_lowercase().starts_with("port");
        let confidence = if explicit { 0.9 } else { 0.5 };
        vec![Detection::new("inspect", confidence).reason("port").preview(format!("What's on port {port}"))]
    }

    fn tool_view(&self, _operation_id: &str, cx: &mut App) -> Option<Box<dyn ToolView>> {
        let view = cx.new(|_| PortView { port: None, state: State::Idle, notice: None, _task: None });
        Some(Box::new(PortTool(view)))
    }
}

// ---------------------------------------------------------------------------
// Inspecting (`lsof`, `ps`)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, PartialEq)]
struct Process {
    pid: u32,
    name: String,
    user: String,
    command: String,
    uptime: String,
    /// `TCP 127.0.0.1:8080`, `UDP *:5353` …
    listening: Vec<String>,
    connections: usize,
}

/// Parses `lsof -nP -i :PORT -F pcLPnT` into processes, keeping for each its
/// listening sockets (on `port`) and a count of its other connections.
fn parse_lsof(text: &str, port: u16) -> Vec<Process> {
    let mut procs: BTreeMap<u32, Process> = BTreeMap::new();
    let mut pid = None;
    let (mut proto, mut name, mut state) = (String::new(), String::new(), String::new());
    let flush = |procs: &mut BTreeMap<u32, Process>, pid: Option<u32>, proto: &str, name: &str, state: &str| {
        let (Some(pid), false) = (pid, name.is_empty()) else { return };
        let Some(p) = procs.get_mut(&pid) else { return };
        let local = name.split("->").next().unwrap_or(name);
        let is_ours = local.rsplit_once(':').is_some_and(|(_, p)| p == port.to_string());
        // UDP has no LISTEN state: a bound socket on the port counts as listening.
        if is_ours && !name.contains("->") && (state == "LISTEN" || proto == "UDP") {
            p.listening.push(format!("{proto} {local}"));
        } else {
            p.connections += 1;
        }
    };
    for line in text.lines() {
        let (tag, value) = line.split_at(line.len().min(1));
        match tag {
            "p" => {
                flush(&mut procs, pid, &proto, &name, &state);
                (proto, name, state) = Default::default();
                pid = value.parse().ok();
                if let Some(pid) = pid {
                    procs.entry(pid).or_insert(Process { pid, ..Default::default() });
                }
            }
            "f" => {
                flush(&mut procs, pid, &proto, &name, &state);
                (proto, name, state) = Default::default();
            }
            "c" | "L" => {
                if let Some(p) = pid.and_then(|pid| procs.get_mut(&pid)) {
                    if tag == "c" { p.name = value.into() } else { p.user = value.into() }
                }
            }
            "P" => proto = value.into(),
            "n" => name = value.into(),
            "T" => {
                if let Some(s) = value.strip_prefix("ST=") {
                    state = s.into();
                }
            }
            _ => {}
        }
    }
    flush(&mut procs, pid, &proto, &name, &state);
    procs.into_values().collect()
}

#[derive(Debug, Clone, PartialEq)]
struct Report {
    port: u16,
    listeners: Vec<Process>,
    /// Other processes with connections to/from the port.
    clients: Vec<Process>,
}

fn inspect(port: u16) -> Result<Report, String> {
    let out = Command::new("/usr/sbin/lsof")
        .args(["-nP", &format!("-i:{port}"), "-F", "pcLPnT"])
        .output()
        .map_err(|e| format!("cannot run lsof: {e}"))?;
    // lsof exits 1 when nothing matches.
    let procs = parse_lsof(&String::from_utf8_lossy(&out.stdout), port);
    let (mut listeners, clients): (Vec<_>, Vec<_>) = procs.into_iter().partition(|p| !p.listening.is_empty());
    for p in &mut listeners {
        if let Ok(ps) = Command::new("/bin/ps").args(["-o", "etime=,command=", "-p", &p.pid.to_string()]).output() {
            let line = String::from_utf8_lossy(&ps.stdout).trim().to_string();
            if let Some((etime, command)) = line.split_once(char::is_whitespace) {
                (p.uptime, p.command) = (etime.trim().to_string(), command.trim().to_string());
            }
        }
    }
    Ok(Report { port, listeners, clients })
}

/// `01-02:03:04` / `02:03:04` / `03:04` (ps etime) → `1d 2h`, `2h 3m`, `3m 4s`.
fn human_etime(etime: &str) -> String {
    let (days, rest) = etime.split_once('-').map_or((0, etime), |(d, r)| (d.parse().unwrap_or(0), r));
    let parts: Vec<u64> = rest.split(':').filter_map(|p| p.parse().ok()).collect();
    let (h, m, s) = match parts.as_slice() {
        [h, m, s] => (*h, *m, *s),
        [m, s] => (0, *m, *s),
        _ => return etime.to_string(),
    };
    match (days, h, m) {
        (d, h, _) if d > 0 => format!("{d}d {h}h"),
        (_, h, m) if h > 0 => format!("{h}h {m}m"),
        (_, _, m) if m > 0 => format!("{m}m {s}s"),
        _ => format!("{s}s"),
    }
}

/// Signals every listener except Delight itself; returns the PIDs signalled.
fn kill(listeners: &[Process], force: bool) -> Result<Vec<u32>, String> {
    let me = std::process::id();
    let pids: Vec<String> = listeners.iter().filter(|p| p.pid != me && p.pid > 1).map(|p| p.pid.to_string()).collect();
    if pids.is_empty() {
        return Ok(Vec::new());
    }
    let out = Command::new("/bin/kill")
        .arg(if force { "-KILL" } else { "-TERM" })
        .args(&pids)
        .output()
        .map_err(|e| format!("cannot run kill: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(pids.iter().filter_map(|p| p.parse().ok()).collect())
}

// ---------------------------------------------------------------------------
// Tool page
// ---------------------------------------------------------------------------

enum State {
    Idle,
    Loading,
    Ready(Report),
    Failed(String),
}

struct PortView {
    port: Option<u16>,
    state: State,
    /// Result of the last kill.
    notice: Option<(bool, String)>,
    _task: Option<Task<()>>,
}

impl PortView {
    fn refresh(&mut self, delay: Duration, cx: &mut Context<Self>) {
        let Some(port) = self.port else { return };
        if !matches!(self.state, State::Ready(_)) {
            self.state = State::Loading;
        }
        self._task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(delay).await;
            let result = cx.background_executor().spawn(async move { inspect(port) }).await;
            let _ = this.update(cx, |this, cx| {
                if this.port == Some(port) {
                    this.state = match result {
                        Ok(r) => State::Ready(r),
                        Err(e) => State::Failed(e),
                    };
                    cx.notify();
                }
            });
        }));
    }

    fn kill(&mut self, force: bool, cx: &mut Context<Self>) {
        let State::Ready(report) = &self.state else { return };
        let (listeners, port) = (report.listeners.clone(), report.port);
        self._task = Some(cx.spawn(async move |this, cx| {
            let killed = cx.background_executor().spawn(async move { kill(&listeners, force) }).await;
            // Give the process a moment to exit, then look again.
            cx.background_executor().timer(Duration::from_millis(if force { 300 } else { 800 })).await;
            let after = cx.background_executor().spawn(async move { inspect(port) }).await;
            let _ = this.update(cx, |this, cx| {
                let still = after.as_ref().map(|r| !r.listeners.is_empty()).unwrap_or(false);
                this.notice = Some(match killed {
                    Err(e) => (false, format!("Kill failed: {e}")),
                    Ok(pids) if pids.is_empty() => (false, "Nothing to kill".into()),
                    Ok(pids) if still && !force => (false, format!("Sent SIGTERM to {} — still listening; try Force kill", join(&pids))),
                    Ok(pids) if still => (false, format!("Sent SIGKILL to {} — port {port} is still in use", join(&pids))),
                    Ok(pids) => (true, format!("Killed {} — port {port} is free", join(&pids))),
                });
                if let Ok(r) = after {
                    this.state = State::Ready(r);
                }
                cx.notify();
            });
        }));
    }
}

fn join(pids: &[u32]) -> String {
    pids.iter().map(|p| format!("PID {p}")).collect::<Vec<_>>().join(", ")
}

fn line(icon: &'static str, color: gpui::Hsla, text: String) -> impl IntoElement {
    div().flex().items_center().gap(px(8.)).text_size(px(12.5)).child(ui::icon(icon, 14., color)).child(text)
}

fn row(t: &Theme, key: &str, value: String) -> gpui::AnyElement {
    div()
        .flex()
        .gap(px(12.))
        .px(px(12.))
        .py(px(7.))
        .text_size(px(12.))
        .child(div().w(px(84.)).flex_shrink_0().text_color(t.secondary_label).child(key.to_string()))
        .child(div().flex_1().min_w(px(0.)).child(value))
        .into_any_element()
}

impl Render for PortView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(window, cx);
        let mut page = div().flex().flex_col().gap(px(12.));
        if let Some((ok, text)) = &self.notice {
            page = if *ok {
                page.child(line("icons/circle-check.svg", t.green, text.clone()))
            } else {
                page.child(line("icons/triangle-alert.svg", t.orange, text.clone()))
            };
        }
        match &self.state {
            State::Idle | State::Loading => page.child(div().text_size(px(12.)).text_color(t.tertiary_label).child("Looking…")),
            State::Failed(e) => page.child(line("icons/circle-x.svg", t.red, e.clone())),
            State::Ready(r) if r.listeners.is_empty() => {
                page = page.child(line("icons/info.svg", t.blue, format!("Nothing is listening on port {}", r.port)));
                if !r.clients.is_empty() {
                    page = page.child(
                        div()
                            .text_size(px(12.))
                            .text_color(t.secondary_label)
                            .child(format!("{} process(es) have connections to it: {}", r.clients.len(), names(&r.clients))),
                    );
                }
                page.child(
                    div()
                        .text_size(px(11.))
                        .text_color(t.tertiary_label)
                        .child("Only your own processes are visible (like lsof without sudo)."),
                )
            }
            State::Ready(r) => {
                page = page.child(line(
                    "icons/circle-check.svg",
                    t.green,
                    format!("Port {} is in use by {}", r.port, names(&r.listeners)),
                ));
                for p in &r.listeners {
                    let mut rows = vec![
                        row(&t, "Process", format!("{} (PID {})", p.name, p.pid)),
                        row(&t, "Listening", p.listening.join(", ")),
                        row(&t, "User", p.user.clone()),
                    ];
                    if !p.uptime.is_empty() {
                        rows.push(row(&t, "Running for", human_etime(&p.uptime)));
                    }
                    if p.connections > 0 {
                        rows.push(row(&t, "Connections", p.connections.to_string()));
                    }
                    page = page.child(ui::group(&t, rows));
                    if !p.command.is_empty() {
                        page = page.child(
                            div()
                                .rounded(px(8.))
                                .bg(t.fill)
                                .border_1()
                                .border_color(t.separator)
                                .px(px(12.))
                                .py(px(8.))
                                .font_family(t.mono_font.clone())
                                .text_size(px(11.5))
                                .child(p.command.clone()),
                        );
                    }
                }
                if !r.clients.is_empty() {
                    page = page.child(
                        div()
                            .text_size(px(11.5))
                            .text_color(t.tertiary_label)
                            .child(format!("Also connected (not killed): {}", names(&r.clients))),
                    );
                }
                page
            }
        }
    }
}

fn names(procs: &[Process]) -> String {
    procs.iter().map(|p| format!("{} ({})", p.name, p.pid)).collect::<Vec<_>>().join(", ")
}

struct PortTool(Entity<PortView>);

impl ToolView for PortTool {
    fn view(&self) -> AnyView {
        self.0.clone().into()
    }

    fn update(&self, context: &ToolContext, cx: &mut App) {
        self.0.update(cx, |view, cx| {
            let port = parse_port(&context.input);
            if view.port != port {
                view.port = port;
                view.notice = None;
                view.state = State::Idle;
                view.refresh(Duration::from_millis(200), cx);
                cx.notify();
            }
        });
    }

    fn actions(&self, cx: &App) -> Vec<Action> {
        let view = self.0.read(cx);
        let mut actions = Vec::new();
        if let State::Ready(r) = &view.state
            && !r.listeners.is_empty()
        {
            // Destructive: their own keys, never ↵ (which is Refresh).
            actions.push(Action::custom("kill", "Kill").shortcut("cmd-enter"));
            actions.push(Action::custom("force_kill", "Force kill").shortcut("cmd-shift-enter"));
        }
        if view.port.is_some() {
            actions.push(Action::custom("refresh", "Refresh").primary());
        }
        actions
    }

    fn perform(&self, action_id: &str, cx: &mut App) {
        self.0.update(cx, |view, cx| match action_id {
            "kill" => view.kill(false, cx),
            "force_kill" => view.kill(true, cx),
            "refresh" => {
                view.notice = None;
                view.refresh(Duration::ZERO, cx);
            }
            _ => {}
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_ports() {
        for (input, port) in [("8080", Some(8080)), (":3000", Some(3000)), ("localhost:5432", Some(5432)), ("port 22", Some(22))] {
            assert_eq!(parse_port(input), port, "{input}");
        }
        for input in ["70000", "0", "api.x.com:443", "12ab", "", "346632"] {
            assert_eq!(parse_port(input), None, "{input}");
        }
    }

    #[test]
    fn parses_lsof() {
        let out = "p100\ncnode\nLme\nf22\nPTCP\nn*:3000\nTST=LISTEN\nf23\nPTCP\nn127.0.0.1:3000->127.0.0.1:51000\nTST=ESTABLISHED\n\
                   p200\ncchrome\nLme\nf40\nPTCP\nn127.0.0.1:51000->127.0.0.1:3000\nTST=ESTABLISHED\n";
        let procs = parse_lsof(out, 3000);
        assert_eq!(procs.len(), 2);
        assert_eq!((procs[0].name.as_str(), procs[0].listening.clone(), procs[0].connections), ("node", vec!["TCP *:3000".to_string()], 1));
        assert!(procs[1].listening.is_empty(), "a client isn't a listener");
    }

    #[test]
    fn formats_uptime() {
        assert_eq!(human_etime("01-02:03:04"), "1d 2h");
        assert_eq!(human_etime("02:03:04"), "2h 3m");
        assert_eq!(human_etime("03:04"), "3m 4s");
        assert_eq!(human_etime("00:09"), "9s");
    }

    /// Kills the child process when dropped, even if the test panics.
    struct Child(std::process::Child);

    impl Drop for Child {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    /// Real processes: a listener (`nc -l`, a separate process so killing it
    /// can't kill the test) and a connected client — this test.
    #[test]
    fn inspects_and_kills_a_listener_only() {
        use std::net::{TcpListener, TcpStream};
        use std::process::Stdio;

        let probe = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = probe.local_addr().unwrap().port();
        drop(probe);
        let mut server = Child(
            Command::new("/usr/bin/nc")
                .args(["-l", "127.0.0.1", &port.to_string()])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        );
        let pid = server.0.id();
        let mut client = None;
        for _ in 0..50 {
            std::thread::sleep(Duration::from_millis(50));
            if let Ok(c) = TcpStream::connect(("127.0.0.1", port)) {
                client = Some(c);
                break;
            }
        }
        assert!(client.is_some(), "nc never listened");

        let report = inspect(port).unwrap();
        assert_eq!(report.listeners.iter().map(|p| p.pid).collect::<Vec<_>>(), [pid], "{report:?}");
        assert!(report.listeners[0].command.contains("nc"));
        // The test is only a client: never a kill target.
        assert!(report.listeners.iter().all(|p| p.pid != std::process::id()));

        assert_eq!(kill(&report.listeners, true).unwrap(), [pid]);
        let _ = server.0.wait();
        assert!(inspect(port).unwrap().listeners.is_empty());
    }
}
