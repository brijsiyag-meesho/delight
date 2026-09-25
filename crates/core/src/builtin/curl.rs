//! Paste a `curl …` command, press ↵ (the Run action), see the response.
//!
//! The command is split into arguments by [`shlex`] (POSIX shell rules) and
//! run as `/usr/bin/curl` with them — never through a shell; curl itself
//! interprets its options. Only the flags that redirect or reshape curl's
//! output are dropped so the response can be captured, and a pasted
//! `Accept-Encoding` is replaced by `--compressed` (only encodings this curl
//! can decode). A timeout is added unless one is given.

use std::process::Command;
use std::sync::Arc;

use delight_sdk::gpui::{self, AnyView, App, AppContext, Context, Entity, Task, Window, div, prelude::*, px};
use delight_sdk::theme::{Theme, theme};
use delight_sdk::ui;
use delight_sdk::{Action, Detection, Input, Plugin, PluginManifest, ToolContext, ToolView};
use serde_json::Value;

use super::{manifest, op};
use crate::stats::human_bytes;

pub struct CurlPlugin {
    manifest: PluginManifest,
}

impl CurlPlugin {
    pub fn new() -> Self {
        Self {
            manifest: manifest(
                "delight.curl",
                "curl",
                "Run a pasted curl command and show the response.",
                ">_",
                "#5856D6",
                &["curl", "http", "request", "api"],
                vec![op("run", "Run curl", "Run the pasted curl command", &["curl", "http", "request"], vec![])],
            ),
        }
    }
}

impl Default for CurlPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl Plugin for CurlPlugin {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    fn detect(&self, input: &Input) -> Vec<Detection> {
        // Cheap shape check before splitting the whole input.
        let t = input.trimmed;
        if !(t.starts_with("curl") && t[4..].starts_with(char::is_whitespace)) {
            return Vec::new();
        }
        let Ok(args) = parse(t) else { return Vec::new() };
        let request = summary(&args);
        vec![Detection::new("run", 0.97).reason("curl command").preview(format!("{} {}", request.method, request.url))]
    }

    fn tool_view(&self, _operation_id: &str, cx: &mut App) -> Option<Box<dyn ToolView>> {
        let view = cx.new(|_| CurlView { input: String::new(), state: State::Idle, show_headers: false, _task: None });
        Some(Box::new(CurlTool(view)))
    }
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

/// curl's arguments (without `curl`), or why the input isn't a curl command.
fn parse(input: &str) -> Result<Vec<String>, String> {
    // `\`-newline continuations: shlex 1.3 turns them into empty arguments.
    let joined = input.trim().replace("\\\r\n", " ").replace("\\\n", " ");
    let words = shlex::split(&joined).ok_or("unbalanced quotes")?;
    match words.split_first() {
        Some((first, rest)) if first == "curl" && !rest.is_empty() => Ok(rest.to_vec()),
        _ => Err("not a curl command".into()),
    }
}

/// `--name=value` → `("--name", Some("value"))`.
fn split_eq(arg: &str) -> (&str, Option<&str>) {
    match arg.split_once('=') {
        Some((name, value)) if arg.starts_with("--") => (name, Some(value)),
        _ => (arg, None),
    }
}

#[derive(Debug, Clone, PartialEq)]
struct Summary {
    method: String,
    url: String,
    headers: usize,
    has_body: bool,
    /// Why the body isn't valid JSON, when the request says it sends JSON.
    json_error: Option<String>,
}

/// A preview before running (after running, curl reports the real method
/// and URL). Only the options that matter for it are looked at.
fn summary(args: &[String]) -> Summary {
    let (mut method, mut url, mut headers, mut has_body) = (None, None, 0, false);
    let (mut body, mut sends_json) = (None::<String>, false);
    for (i, arg) in args.iter().enumerate() {
        let (name, inline) = split_eq(arg);
        let value = || inline.map(str::to_string).or_else(|| args.get(i + 1).cloned());
        match name {
            "-X" | "--request" => method = value(),
            "-I" | "--head" => method = Some("HEAD".into()),
            "-H" | "--header" => {
                headers += 1;
                let h = value().unwrap_or_default().to_ascii_lowercase();
                sends_json |= h.starts_with("content-type:") && h.contains("json");
            }
            "--json" => {
                (has_body, sends_json) = (true, true);
                body = value();
            }
            "-d" | "--data" | "--data-raw" | "--data-binary" => {
                has_body = true;
                body = body.or_else(value);
            }
            "--url" => url = value(),
            "--data-urlencode" | "--data-ascii" | "-F" | "--form" => has_body = true,
            _ if url.is_none() && arg.contains("://") && !arg.starts_with('-') => url = Some(arg.clone()),
            _ => {}
        }
    }
    let method = method.unwrap_or_else(|| if has_body { "POST" } else { "GET" }.into()).to_uppercase();
    // `@file` bodies are read by curl; only inline JSON is checked.
    let json_error = body
        .filter(|b| sends_json && !b.starts_with('@'))
        .and_then(|b| serde_json::from_str::<Value>(&b).err())
        .map(|e| format!("{e}"));
    Summary { method, url: url.unwrap_or_default(), headers, has_body, json_error }
}

/// Drops the flags that redirect or reshape output (the tool adds its own)
/// and pasted `Accept-Encoding` headers (`--compressed` sets a decodable one).
fn sanitize(args: &[String]) -> Vec<String> {
    const WITH_VALUE: &[&str] = &["-o", "--output", "-w", "--write-out", "-D", "--dump-header"];
    const FLAGS: &[&str] = &["-i", "--include", "-v", "--verbose", "-s", "--silent", "-S", "--show-error", "-#", "--progress-bar"];
    let is_accept_encoding = |h: &str| h.trim_start().to_ascii_lowercase().starts_with("accept-encoding:");
    let mut out = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        let (name, inline) = split_eq(arg);
        i += 1;
        if WITH_VALUE.contains(&name) {
            i += usize::from(inline.is_none());
        } else if FLAGS.contains(&arg) {
        } else if matches!(name, "-H" | "--header") {
            let header = inline.map(str::to_string).or_else(|| args.get(i).cloned()).unwrap_or_default();
            i += usize::from(inline.is_none());
            if !is_accept_encoding(&header) {
                out.extend([name.to_string(), header]);
            }
        } else if arg.len() > 2
            && arg.starts_with('-')
            && !arg.starts_with("--")
            && arg[1..].chars().all(|c| c.is_ascii_alphabetic())
            && !arg.contains('o')
        {
            // Bundled short flags like `-sSL`: keep the rest.
            let kept: String = arg[1..].chars().filter(|c| !"sSiv".contains(*c)).collect();
            if !kept.is_empty() {
                out.push(format!("-{kept}"));
            }
        } else {
            out.push(arg.to_string());
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Running
// ---------------------------------------------------------------------------

const MARKER: &str = "\n__DELIGHT_CURL__";
/// Output is clipped for display; Copy gives everything.
const MAX_SHOWN: usize = 64 * 1024;

#[derive(Debug, Clone)]
struct Response {
    /// As curl reports them (after redirects).
    method: String,
    url: String,
    status: u16,
    time_ms: u64,
    size: usize,
    content_type: String,
    headers: String,
    body: Vec<u8>,
}

impl Response {
    fn body_text(&self) -> String {
        let text = String::from_utf8_lossy(&self.body);
        match serde_json::from_str::<Value>(&text) {
            Ok(v) => serde_json::to_string_pretty(&v).unwrap_or_else(|_| text.to_string()),
            Err(_) => text.to_string(),
        }
    }

    fn is_binary(&self) -> bool {
        std::str::from_utf8(&self.body).is_err()
    }
}

fn run(args: &[String]) -> Result<Response, String> {
    let mut cmd = Command::new("/usr/bin/curl");
    cmd.args(["--silent", "--show-error", "--compressed", "--dump-header", "/dev/stderr"]);
    cmd.arg("--write-out").arg(format!("%{{stderr}}{MARKER}%{{json}}"));
    if !args.iter().any(|a| a == "-m" || a == "--max-time") {
        cmd.args(["--max-time", "30"]);
    }
    let out = cmd.args(sanitize(args)).output().map_err(|e| format!("cannot run curl: {e}"))?;
    let stderr = String::from_utf8_lossy(&out.stderr);
    let (headers, info) = stderr.rsplit_once(MARKER).unwrap_or((&stderr, "{}"));
    if !out.status.success() {
        let message = headers.lines().rev().find(|l| l.starts_with("curl:")).unwrap_or(headers.trim());
        return Err(message.trim().to_string());
    }
    let info: Value = serde_json::from_str(info.trim()).unwrap_or(Value::Null);
    Ok(Response {
        method: info["method"].as_str().unwrap_or("").to_string(),
        url: info["url_effective"].as_str().unwrap_or("").to_string(),
        status: info["response_code"].as_u64().unwrap_or(0) as u16,
        time_ms: (info["time_total"].as_f64().unwrap_or(0.) * 1000.) as u64,
        size: out.stdout.len(),
        content_type: info["content_type"].as_str().unwrap_or("").to_string(),
        headers: headers.trim().replace("\r\n", "\n"),
        body: out.stdout,
    })
}

// ---------------------------------------------------------------------------
// Tool page
// ---------------------------------------------------------------------------

enum State {
    Idle,
    Running(String),
    Done { command: String, response: Arc<Response> },
    Failed { command: String, error: String },
}

struct CurlView {
    input: String,
    state: State,
    show_headers: bool,
    _task: Option<Task<()>>,
}

impl CurlView {
    fn run(&mut self, cx: &mut Context<Self>) {
        if matches!(self.state, State::Running(_)) {
            return;
        }
        let Ok(args) = parse(&self.input) else { return };
        let command = self.input.clone();
        self.state = State::Running(command.clone());
        cx.notify();
        self._task = Some(cx.spawn(async move |this, cx| {
            let result = cx.background_executor().spawn(async move { run(&args) }).await;
            let _ = this.update(cx, |this, cx| {
                this.state = match result {
                    Ok(response) => State::Done { command, response: Arc::new(response) },
                    Err(error) => State::Failed { command, error },
                };
                cx.notify();
            });
        }));
    }

    fn last_command(&self) -> Option<&str> {
        match &self.state {
            State::Idle => None,
            State::Running(c) | State::Done { command: c, .. } | State::Failed { command: c, .. } => Some(c),
        }
    }
}

fn mono_box(t: &Theme, text: String) -> impl IntoElement {
    let text = if text.len() > MAX_SHOWN {
        let mut end = MAX_SHOWN;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}\n… (truncated — Copy gives the full output)", &text[..end])
    } else {
        text
    };
    div()
        .rounded(px(8.))
        .bg(t.fill)
        .border_1()
        .border_color(t.separator)
        .px(px(12.))
        .py(px(10.))
        .font_family(t.mono_font.clone())
        .text_size(px(12.))
        .line_height(px(18.))
        .child(text)
}

fn hint(t: &Theme, text: impl Into<String>) -> impl IntoElement {
    div().text_size(px(12.)).text_color(t.tertiary_label).child(text.into())
}

impl Render for CurlView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(window, cx);
        let mut page = div().flex().flex_col().gap(px(12.));
        let args = match parse(&self.input) {
            Ok(args) => args,
            Err(e) => return page.child(hint(&t, format!("Can't read this command: {e}"))),
        };
        let mut s = summary(&args);
        if let State::Done { command, response } = &self.state
            && *command == self.input
            && !response.method.is_empty()
        {
            (s.method, s.url) = (response.method.clone(), response.url.clone());
        }

        // The request.
        let mut request = div()
            .flex()
            .items_center()
            .gap(px(8.))
            .text_size(px(12.5))
            .child(div().font_weight(gpui::FontWeight::SEMIBOLD).text_color(t.accent).child(s.method.clone()))
            .child(div().flex_1().min_w(px(0.)).truncate().font_family(t.mono_font.clone()).child(s.url.clone()));
        let mut details = Vec::new();
        if s.headers > 0 {
            details.push(format!("{} header{}", s.headers, if s.headers == 1 { "" } else { "s" }));
        }
        if s.has_body {
            details.push("body".to_string());
        }
        if !details.is_empty() {
            request = request.child(div().text_size(px(11.)).text_color(t.tertiary_label).child(details.join(" · ")));
        }
        page = page.child(request);
        if let Some(error) = &s.json_error {
            page = page.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .text_size(px(12.))
                    .child(ui::icon("icons/triangle-alert.svg", 14., t.orange))
                    .child(format!("The request body isn't valid JSON — {error}")),
            );
        }

        // Status, and Body / Headers once there's a response.
        let stale = self.last_command().is_some_and(|c| c != self.input);
        let mut bar = div().flex().items_center().gap(px(12.)).min_h(px(24.));
        bar = match &self.state {
            State::Idle => bar.child(hint(&t, "Press ↵ to run")),
            State::Running(_) => bar.child(hint(&t, "Running…")),
            State::Done { response: r, .. } => {
                let color = match r.status {
                    200..=299 => t.green,
                    300..=399 => t.blue,
                    _ => t.red,
                };
                let mut line = div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .text_size(px(12.))
                    .child(div().font_weight(gpui::FontWeight::SEMIBOLD).text_color(color).child(r.status.to_string()))
                    .child(div().text_color(t.secondary_label).child(format!("{} ms · {}", r.time_ms, human_bytes(r.size))));
                if !r.content_type.is_empty() {
                    line = line.child(div().text_color(t.tertiary_label).child(r.content_type.clone()));
                }
                let show_headers = self.show_headers;
                bar.child(line).child(div().flex_1()).child(ui::segmented(
                    &t,
                    "curl-view",
                    &[("Body".into(), !show_headers), ("Headers".into(), show_headers)],
                    |i, seg| {
                        seg.on_click(cx.listener(move |this, _, _, cx| {
                            this.show_headers = i == 1;
                            cx.notify();
                        }))
                    },
                ))
            }
            State::Failed { .. } => bar,
        };
        if stale && !matches!(self.state, State::Running(_)) {
            bar = bar.child(hint(&t, "Command changed — press ↵ to run it"));
        }
        page = page.child(bar);

        // The response.
        match &self.state {
            State::Failed { error, .. } => page.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .text_size(px(12.5))
                    .child(ui::icon("icons/circle-x.svg", 14., t.red))
                    .child(error.clone()),
            ),
            State::Done { response, .. } if self.show_headers => page.child(mono_box(&t, response.headers.clone())),
            State::Done { response, .. } if response.is_binary() => {
                page.child(hint(&t, format!("Binary response, {} — Copy isn't available", human_bytes(response.size))))
            }
            State::Done { response, .. } if response.body.is_empty() => page.child(hint(&t, "Empty body")),
            State::Done { response, .. } => page.child(mono_box(&t, response.body_text())),
            _ => page,
        }
    }
}

struct CurlTool(Entity<CurlView>);

impl ToolView for CurlTool {
    fn view(&self) -> AnyView {
        self.0.clone().into()
    }

    fn update(&self, context: &ToolContext, cx: &mut App) {
        self.0.update(cx, |view, cx| {
            if view.input != context.input {
                view.input = context.input.clone();
                cx.notify();
            }
        });
    }

    fn actions(&self, cx: &App) -> Vec<Action> {
        let view = self.0.read(cx);
        if parse(&view.input).is_err() {
            return Vec::new();
        }
        let mut actions = vec![Action::custom("run", "Run").primary()];
        if let State::Done { response, .. } = &view.state {
            if !response.is_binary() && !response.body.is_empty() {
                actions.push(Action::copy("copy_body", "Copy body", response.body_text()));
            }
            actions.push(Action::copy("copy_headers", "Copy headers", response.headers.clone()));
        }
        actions
    }

    fn perform(&self, action_id: &str, cx: &mut App) {
        if action_id == "run" {
            self.0.update(cx, |view, cx| view.run(cx));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_like_a_shell() {
        let args = parse(r#"curl -H 'A: b c' "x\"y" a\ b"#).unwrap();
        assert_eq!(args, ["-H", "A: b c", "x\"y", "a b"]);
        // Multi-line paste with continuations and a multi-line body (trailing comma kept verbatim).
        let args = parse("curl -X POST 'http://h/p' \\\n  --header 'Accept: */*' \\\n  --data '{\n  \"a\": 1,\n}'").unwrap();
        assert_eq!(args, ["-X", "POST", "http://h/p", "--header", "Accept: */*", "--data", "{\n  \"a\": 1,\n}"]);
        assert!(parse("curl 'oops").is_err());
        assert!(parse("wget http://x").is_err());
    }

    #[test]
    fn summarises_method_and_url() {
        let s = summary(&parse("curl -H 'A: b' --data '{}' http://h/p").unwrap());
        assert_eq!(s, Summary { method: "POST".into(), url: "http://h/p".into(), headers: 1, has_body: true, json_error: None });
        let bad = summary(&parse("curl http://h -H 'Content-Type: application/json' --data '{\"a\": 1,}'").unwrap());
        assert!(bad.json_error.unwrap().contains("trailing comma"));
        let not_json = summary(&parse("curl http://h --data 'a=1,'").unwrap());
        assert_eq!(not_json.json_error, None);
        assert_eq!(summary(&parse("curl -X put 'http://h'").unwrap()).method, "PUT");
        assert_eq!(summary(&parse("curl http://h").unwrap()).method, "GET");
    }

    #[test]
    fn drops_output_flags() {
        let args = parse("curl -sSL -o out.txt -i --write-out='%{http_code}' -H 'A: b' -H 'Accept-Encoding: br, zstd' http://h").unwrap();
        assert_eq!(sanitize(&args), ["-L", "-H", "A: b", "http://h"]);
    }

    /// Runs the real curl against a local server.
    #[test]
    fn runs_against_a_local_server() {
        use std::io::{BufRead, BufReader, Read, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/items", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let (mut head, mut line) = (String::new(), String::new());
            while reader.read_line(&mut line).unwrap() > 2 {
                head.push_str(&line);
                line.clear();
            }
            let length: usize = head
                .lines()
                .find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse().unwrap()))
                .unwrap_or(0);
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            let reply = r#"{"ok":true}"#;
            write!(
                stream,
                "HTTP/1.1 201 Created\r\nContent-Type: application/json\r\nX-Test: yes\r\nContent-Length: {}\r\n\r\n{reply}",
                reply.len()
            )
            .unwrap();
            (head, String::from_utf8(body).unwrap())
        });

        let cmd = format!("curl -X POST '{url}' -H 'Authorization: t0k' --data '{{\"a\":1}}' -v -o /dev/null");
        let response = run(&parse(&cmd).unwrap()).unwrap();
        let (head, body) = server.join().unwrap();

        assert!(head.starts_with("POST /items "));
        assert!(head.contains("Authorization: t0k"));
        assert_eq!(body, r#"{"a":1}"#);
        assert_eq!(response.status, 201);
        assert_eq!((response.method.as_str(), response.url.as_str()), ("POST", url.as_str()));
        assert_eq!(response.content_type, "application/json");
        assert!(response.headers.contains("X-Test: yes"));
        assert_eq!(response.body_text(), "{\n  \"ok\": true\n}");
    }
}
