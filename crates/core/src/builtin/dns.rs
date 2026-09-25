//! Paste a hostname, URL or IP: DNS records, the resolver that answered,
//! what the system resolver (what apps get) returns, and reverse lookups.
//!
//! Uses macOS's own tools: `scutil --dns` for the resolver configuration
//! (including VPN resolvers scoped to domains, which plain `dig` ignores) and
//! `dig` against the matching resolver; the system lookup is `getaddrinfo`
//! (honours `/etc/hosts` and scoped resolvers).

use std::collections::BTreeSet;
use std::net::{IpAddr, ToSocketAddrs};
use std::process::Command;
use std::time::Instant;

use delight_sdk::{
    Action, Block, Detection, Input, KeyValueRow, NoticeLevel, Plugin, PluginError, PluginManifest, RunRequest,
    ToolOutput,
};

use super::{manifest, op};

pub struct DnsPlugin {
    manifest: PluginManifest,
}

impl DnsPlugin {
    pub fn new() -> Self {
        let mut lookup = op("lookup", "DNS lookup", "DNS records, resolver and reverse lookup for a host", &[
            "dns", "host", "nslookup", "dig", "ip",
        ], vec![]);
        lookup.run_delay_ms = 400;
        Self {
            manifest: manifest(
                "delight.dns",
                "DNS",
                "Look up a host's DNS records, resolver and addresses.",
                "DNS",
                "#30B0C7",
                &["dns", "network", "host"],
                vec![lookup],
            ),
        }
    }
}

impl Default for DnsPlugin {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Input
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum Target {
    Host(String),
    Ip(IpAddr),
}

fn is_hostname(s: &str) -> bool {
    let labels: Vec<&str> = s.trim_end_matches('.').split('.').collect();
    labels.len() >= 2
        && labels.iter().all(|l| {
            !l.is_empty() && l.len() <= 63 && !l.starts_with('-') && !l.ends_with('-') && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        })
        && labels.last().is_some_and(|tld| tld.len() >= 2 && tld.chars().all(|c| c.is_ascii_alphabetic()))
}

/// The host in `input`: a bare hostname or IP, `host:port`, or a URL.
fn target(input: &str) -> Option<(Target, bool)> {
    let t = input.trim();
    if t.contains(char::is_whitespace) || t.is_empty() {
        return None;
    }
    let is_url = t.contains("://");
    let rest = t.split_once("://").map_or(t, |(_, r)| r);
    let authority = rest.split(['/', '?', '#']).next()?;
    let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    // [v6]:port, v6, host:port, host
    let host = if let Some(v6) = host.strip_prefix('[') {
        v6.split(']').next()?
    } else if host.matches(':').count() == 1 {
        host.split(':').next()?
    } else {
        host
    };
    if let Ok(ip) = host.parse::<IpAddr>() {
        return Some((Target::Ip(ip), is_url));
    }
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    is_hostname(&host).then_some((Target::Host(host), is_url))
}

// ---------------------------------------------------------------------------
// Resolvers (`scutil --dns`)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, PartialEq)]
struct Resolver {
    domain: Option<String>,
    nameservers: Vec<String>,
    search: Vec<String>,
}

/// The unscoped section of `scutil --dns`.
fn parse_scutil(text: &str) -> Vec<Resolver> {
    let main = text.split("DNS configuration (for scoped queries)").next().unwrap_or(text);
    let mut out = Vec::new();
    let mut current: Option<Resolver> = None;
    for line in main.lines() {
        let line = line.trim();
        if line.starts_with("resolver #") {
            out.extend(current.take());
            current = Some(Resolver::default());
            continue;
        }
        let (Some(r), Some((key, value))) = (current.as_mut(), line.split_once(':')) else { continue };
        let (key, value) = (key.trim(), value.trim().to_string());
        match key {
            "domain" => r.domain = Some(value.trim_end_matches('.').to_ascii_lowercase()),
            k if k.starts_with("nameserver[") => r.nameservers.push(value),
            k if k.starts_with("search domain[") => r.search.push(value),
            _ => {}
        }
    }
    out.extend(current);
    out.retain(|r| !r.nameservers.is_empty());
    out
}

/// The resolver macOS uses for `host`: the longest matching scoped domain,
/// otherwise the default (first unscoped) one.
fn resolver_for<'a>(resolvers: &'a [Resolver], host: &str) -> Option<&'a Resolver> {
    let scoped = resolvers
        .iter()
        .filter(|r| r.domain.as_deref().is_some_and(|d| host == d || host.ends_with(&format!(".{d}"))))
        .max_by_key(|r| r.domain.as_ref().map_or(0, String::len));
    scoped.or_else(|| resolvers.iter().find(|r| r.domain.is_none()))
}

// ---------------------------------------------------------------------------
// Queries (`dig`)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
struct Record {
    name: String,
    ttl: u32,
    kind: String,
    data: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
struct Answer {
    status: String,
    answer: Vec<Record>,
    authority: Vec<Record>,
    query_ms: Option<u32>,
    server: Option<String>,
    error: Option<String>,
}

fn parse_record(line: &str) -> Option<Record> {
    let mut parts = line.split_whitespace();
    let name = parts.next()?.trim_end_matches('.').to_string();
    let ttl = parts.next()?.parse().ok()?;
    let _class = parts.next()?;
    let kind = parts.next()?.to_string();
    let data = parts.collect::<Vec<_>>().join(" ");
    Some(Record { name, ttl, kind, data: data.trim_end_matches('.').to_string() })
}

fn parse_dig(text: &str) -> Answer {
    let mut a = Answer::default();
    let mut section = "";
    for line in text.lines() {
        let l = line.trim();
        if let Some(rest) = l.split_once("status: ").map(|(_, r)| r) {
            a.status = rest.split(',').next().unwrap_or("").to_string();
        } else if l.starts_with(";; ANSWER SECTION") {
            section = "answer";
        } else if l.starts_with(";; AUTHORITY SECTION") {
            section = "authority";
        } else if l.starts_with(";; ADDITIONAL SECTION") || l.starts_with(";; OPT") {
            section = "";
        } else if let Some(ms) = l.strip_prefix(";; Query time: ") {
            a.query_ms = ms.split_whitespace().next().and_then(|n| n.parse().ok());
        } else if let Some(s) = l.strip_prefix(";; SERVER: ") {
            a.server = Some(s.split(['#', '(']).next().unwrap_or(s).to_string());
        } else if l.starts_with(";; connection timed out") || l.starts_with(";; communications error") {
            a.error = Some(l.trim_start_matches(";; ").to_string());
        } else if !l.is_empty() && !l.starts_with(';') {
            match (section, parse_record(l)) {
                ("answer", Some(r)) => a.answer.push(r),
                ("authority", Some(r)) => a.authority.push(r),
                _ => {}
            }
        }
    }
    a
}

fn dig(server: Option<&str>, name: &str, kind: &str) -> Answer {
    let mut cmd = Command::new("/usr/bin/dig");
    if let Some(server) = server {
        cmd.arg(format!("@{server}"));
    }
    cmd.args(["+noall", "+comments", "+answer", "+authority", "+stats", "+time=2", "+tries=1"]);
    if kind == "PTR" {
        cmd.args(["-x", name]);
    } else {
        cmd.args([name, kind]);
    }
    match cmd.output() {
        Ok(out) => {
            let mut a = parse_dig(&String::from_utf8_lossy(&out.stdout));
            if a.status.is_empty() && a.error.is_none() {
                a.error = Some(String::from_utf8_lossy(&out.stderr).trim().to_string()).filter(|e| !e.is_empty());
            }
            a
        }
        Err(e) => Answer { error: Some(format!("cannot run dig: {e}")), ..Default::default() },
    }
}

/// What apps get: `getaddrinfo`, with its time.
fn system_lookup(host: &str) -> (Result<Vec<IpAddr>, String>, u128) {
    let started = Instant::now();
    let result = (host, 0)
        .to_socket_addrs()
        .map(|addrs| addrs.map(|a| a.ip()).collect::<BTreeSet<_>>().into_iter().collect())
        .map_err(|e| e.to_string());
    (result, started.elapsed().as_millis())
}

// ---------------------------------------------------------------------------
// Output
// ---------------------------------------------------------------------------

fn ttl(seconds: u32) -> String {
    match seconds {
        s if s >= 86_400 && s % 86_400 == 0 => format!("TTL {}d", s / 86_400),
        s if s >= 3600 && s % 3600 == 0 => format!("TTL {}h", s / 3600),
        s if s >= 60 && s % 60 == 0 => format!("TTL {}m", s / 60),
        s => format!("TTL {s}s"),
    }
}

fn rows(records: &[Record], key: impl Fn(&Record) -> String) -> Vec<KeyValueRow> {
    records.iter().map(|r| KeyValueRow::new(key(r), r.data.clone()).hint(ttl(r.ttl))).collect()
}

fn section(label: &str, rows: Vec<KeyValueRow>) -> Option<Block> {
    (!rows.is_empty()).then(|| Block::KeyValue { label: Some(label.into()), rows })
}

fn resolver_row(resolver: Option<&Resolver>, answered_by: Option<&str>) -> Vec<KeyValueRow> {
    let mut rows = Vec::new();
    if let Some(server) = answered_by {
        let scope = resolver.and_then(|r| r.domain.clone()).map(|d| format!("scoped to *.{d}")).unwrap_or_else(|| "default resolver".into());
        rows.push(KeyValueRow::new("Answered by", server.to_string()).hint(scope));
    }
    if let Some(r) = resolver
        && !r.search.is_empty()
    {
        rows.push(KeyValueRow::new("Search domains", r.search.join(", ")));
    }
    rows
}

fn lookup_host(host: &str, resolvers: &[Resolver]) -> ToolOutput {
    let resolver = resolver_for(resolvers, host);
    let server = resolver.and_then(|r| r.nameservers.first()).map(String::as_str);
    let kinds = ["A", "AAAA", "CNAME", "MX", "NS", "TXT", "SOA"];
    let (answers, (system, system_ms)) = std::thread::scope(|s| {
        let handles: Vec<_> = kinds.iter().map(|k| s.spawn(move || dig(server, host, k))).collect();
        let system = s.spawn(|| system_lookup(host));
        (handles.into_iter().map(|h| h.join().unwrap_or_default()).collect::<Vec<_>>(), system.join().unwrap_or((Err("lookup failed".into()), 0)))
    });
    let get = |k: &str| &answers[kinds.iter().position(|x| *x == k).expect("kind")];
    let (a, aaaa) = (get("A"), get("AAAA"));
    let of_kind = |ans: &Answer, kind: &str| ans.answer.iter().filter(|r| r.kind == kind).cloned().collect::<Vec<_>>();

    let mut dns_ips: Vec<IpAddr> = of_kind(a, "A").iter().chain(&of_kind(aaaa, "AAAA")).filter_map(|r| r.data.parse().ok()).collect();
    dns_ips.sort();
    dns_ips.dedup();
    // CNAMEs come back in the A/AAAA answers as the chain to the address.
    let mut chain = of_kind(a, "CNAME");
    if chain.is_empty() {
        chain = of_kind(get("CNAME"), "CNAME");
    }

    let mut out = ToolOutput::default();
    // Summary.
    let status = a.status.as_str();
    let server_label = a.server.as_deref().or(server).unwrap_or("?");
    let time = a.query_ms.map(|ms| format!(" · {ms} ms")).unwrap_or_default();
    out = out.block(match (&a.error, status, dns_ips.is_empty()) {
        (Some(e), _, _) => Block::Notice { level: NoticeLevel::Error, text: format!("DNS query failed via {server_label}: {e}") },
        (_, "NXDOMAIN", _) => Block::Notice { level: NoticeLevel::Error, text: format!("{host} does not exist (NXDOMAIN) — per {server_label}{time}") },
        (_, "NOERROR", true) => Block::Notice { level: NoticeLevel::Warning, text: format!("{host} exists but has no A/AAAA records — per {server_label}{time}") },
        (_, "NOERROR", false) => Block::Notice {
            level: NoticeLevel::Success,
            text: format!("{host} → {} — per {server_label}{time}", dns_ips.iter().map(ToString::to_string).collect::<Vec<_>>().join(", ")),
        },
        (_, other, _) => Block::Notice { level: NoticeLevel::Error, text: format!("DNS status {other} from {server_label}{time}") },
    });

    // What apps get, if it differs.
    let system_ips: Vec<IpAddr> = system.clone().unwrap_or_default();
    match &system {
        Ok(ips) if !ips.is_empty() && *ips != dns_ips => {
            out = out.block(Block::Notice {
                level: NoticeLevel::Warning,
                text: "The system resolver (what apps use) returns different addresses — check /etc/hosts, VPN or proxy settings".into(),
            });
        }
        Err(e) if !dns_ips.is_empty() => {
            out = out.block(Block::Notice { level: NoticeLevel::Warning, text: format!("DNS answers, but the system resolver fails: {e}") });
        }
        _ => {}
    }

    // Records. Behind a CNAME they belong to the target name: say so.
    let owner = |r: &Record| if r.name == host { r.kind.clone() } else { r.name.clone() };
    let addresses: Vec<Record> = of_kind(a, "A").into_iter().chain(of_kind(aaaa, "AAAA")).collect();
    for block in [
        section("Addresses", rows(&addresses, |r| r.kind.clone())),
        section("CNAME chain", chain.iter().map(|r| KeyValueRow::new(r.name.clone(), format!("→ {}", r.data)).hint(ttl(r.ttl))).collect()),
        section("Mail (MX)", rows(&of_kind(get("MX"), "MX"), owner)),
        section("Name servers (NS)", rows(&of_kind(get("NS"), "NS"), owner)),
        section("TXT", rows(&of_kind(get("TXT"), "TXT"), owner)),
    ]
    .into_iter()
    .flatten()
    {
        out = out.block(block);
    }
    // SOA: the host's own, or its zone's (from the authority section).
    let soa = get("SOA");
    if let Some(r) = soa.answer.iter().chain(&soa.authority).find(|r| r.kind == "SOA") {
        let f: Vec<&str> = r.data.split_whitespace().collect();
        let mut rs = vec![KeyValueRow::new("Zone", r.name.clone())];
        if f.len() >= 3 {
            rs.push(KeyValueRow::new("Primary NS", f[0].trim_end_matches('.').to_string()));
            rs.push(KeyValueRow::new("Admin", f[1].trim_end_matches('.').to_string()));
            rs.push(KeyValueRow::new("Serial", f[2].to_string()));
        }
        out = out.block(Block::KeyValue { label: Some("Zone (SOA)".into()), rows: rs });
    }

    // System resolver and reverse lookups.
    let system_row = match &system {
        Ok(ips) if ips.is_empty() => KeyValueRow::new("Addresses", "none"),
        Ok(ips) => KeyValueRow::new("Addresses", ips.iter().map(ToString::to_string).collect::<Vec<_>>().join(", ")),
        Err(e) => KeyValueRow::new("Error", e.clone()),
    }
    .hint(format!("{system_ms} ms"));
    out = out.block(Block::KeyValue { label: Some("System resolver (getaddrinfo)".into()), rows: vec![system_row] });

    let reverse: Vec<KeyValueRow> = std::thread::scope(|s| {
        let handles: Vec<_> = dns_ips
            .iter()
            .take(4)
            .map(|ip| {
                let ip = ip.to_string();
                s.spawn(move || {
                    let ptr = dig(server, &ip, "PTR");
                    let names: Vec<String> = ptr.answer.iter().filter(|r| r.kind == "PTR").map(|r| r.data.clone()).collect();
                    KeyValueRow::new(ip, if names.is_empty() { "no PTR record".into() } else { names.join(", ") })
                })
            })
            .collect();
        handles.into_iter().filter_map(|h| h.join().ok()).collect()
    });
    if let Some(b) = section("Reverse (PTR)", reverse) {
        out = out.block(b);
    }
    if let Some(b) = section("Resolver", resolver_row(resolver, a.server.as_deref().or(server))) {
        out = out.block(b);
    }

    // Actions.
    let ips: Vec<String> = if dns_ips.is_empty() { system_ips.iter().map(ToString::to_string).collect() } else { dns_ips.iter().map(ToString::to_string).collect() };
    if !ips.is_empty() {
        out = out.action(Action::copy("copy_ips", "Copy addresses", ips.join("\n")).primary());
    }
    let dig_cmd = match server {
        Some(s) => format!("dig @{s} {host} A"),
        None => format!("dig {host} A"),
    };
    out.action(Action::copy("copy_dig", "Copy dig command", dig_cmd))
}

fn lookup_ip(ip: IpAddr, resolvers: &[Resolver]) -> ToolOutput {
    let server = resolver_for(resolvers, "").and_then(|r| r.nameservers.first()).map(String::as_str);
    let ptr = dig(server, &ip.to_string(), "PTR");
    let names: Vec<String> = ptr.answer.iter().filter(|r| r.kind == "PTR").map(|r| r.data.clone()).collect();
    let time = ptr.query_ms.map(|ms| format!(" · {ms} ms")).unwrap_or_default();
    let mut out = ToolOutput::default().block(match (&ptr.error, names.is_empty()) {
        (Some(e), _) => Block::Notice { level: NoticeLevel::Error, text: format!("Reverse lookup failed: {e}") },
        (_, true) => Block::Notice { level: NoticeLevel::Warning, text: format!("No PTR record for {ip}{time}") },
        (_, false) => Block::Notice { level: NoticeLevel::Success, text: format!("{ip} → {}{time}", names.join(", ")) },
    });
    let kind = if ip.is_loopback() {
        "loopback"
    } else if match ip {
        IpAddr::V4(v4) => v4.is_private() || v4.is_link_local(),
        IpAddr::V6(v6) => v6.is_unicast_link_local() || (v6.segments()[0] & 0xfe00) == 0xfc00,
    } {
        "private"
    } else {
        "public"
    };
    out = out.block(Block::KeyValue {
        label: Some("Address".into()),
        rows: vec![
            KeyValueRow::new("Version", if ip.is_ipv4() { "IPv4" } else { "IPv6" }),
            KeyValueRow::new("Range", kind),
        ],
    });
    if let Some(b) = section("Resolver", resolver_row(resolver_for(resolvers, ""), ptr.server.as_deref().or(server))) {
        out = out.block(b);
    }
    if !names.is_empty() {
        out = out.action(Action::copy("copy_names", "Copy names", names.join("\n")).primary());
    }
    out
}

impl Plugin for DnsPlugin {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    fn detect(&self, input: &Input) -> Vec<Detection> {
        let Some((t, is_url)) = target(input.trimmed) else { return Vec::new() };
        let (confidence, what) = match &t {
            Target::Ip(_) => (0.8, "IP address"),
            Target::Host(_) if is_url => (0.45, "URL"),
            Target::Host(_) => (0.75, "hostname"),
        };
        let name = match t {
            Target::Host(h) => h,
            Target::Ip(ip) => ip.to_string(),
        };
        vec![Detection::new("lookup", confidence).reason(what).preview(name)]
    }

    fn run(&self, req: &RunRequest) -> Result<ToolOutput, PluginError> {
        let (t, _) = target(&req.input).ok_or_else(|| PluginError::Invalid("not a hostname, URL or IP address".into()))?;
        let resolvers = Command::new("/usr/sbin/scutil")
            .arg("--dns")
            .output()
            .map(|o| parse_scutil(&String::from_utf8_lossy(&o.stdout)))
            .unwrap_or_default();
        Ok(match t {
            Target::Host(host) => lookup_host(&host, &resolvers),
            Target::Ip(ip) => lookup_ip(ip, &resolvers),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_host() {
        let host = |s: &str| target(s).map(|(t, _)| t);
        assert_eq!(host("api.corp.example"), Some(Target::Host("api.corp.example".into())));
        assert_eq!(host("https://User@Api.Example.com:8443/v1?x=1"), Some(Target::Host("api.example.com".into())));
        assert_eq!(host("example.com:443"), Some(Target::Host("example.com".into())));
        assert_eq!(host("10.1.2.3"), Some(Target::Ip("10.1.2.3".parse().unwrap())));
        assert_eq!(host("http://[::1]:80/"), Some(Target::Ip("::1".parse().unwrap())));
        assert_eq!(host("hello world"), None);
        assert_eq!(host("{\"a\":1}"), None);
        assert_eq!(host("file.123"), None);
    }

    #[test]
    fn parses_dig() {
        let out = ";; Got answer:\n;; ->>HEADER<<- opcode: QUERY, status: NOERROR, id: 5\n;; ANSWER SECTION:\n\
                   www.x.com.\t300\tIN\tCNAME\tx.com.\nx.com.\t50\tIN\tA\t1.2.3.4\n\n;; AUTHORITY SECTION:\n\
                   x.com.\t900\tIN\tSOA\tns1.x.com. admin.x.com. 2024 7200 3600 1209600 300\n\n\
                   ;; Query time: 43 msec\n;; SERVER: 10.255.18.1#53(10.255.18.1)\n";
        let a = parse_dig(out);
        assert_eq!(a.status, "NOERROR");
        assert_eq!(a.answer.len(), 2);
        assert_eq!(a.answer[1], Record { name: "x.com".into(), ttl: 50, kind: "A".into(), data: "1.2.3.4".into() });
        assert_eq!(a.authority[0].kind, "SOA");
        assert_eq!((a.query_ms, a.server.as_deref()), (Some(43), Some("10.255.18.1")));
        assert_eq!(parse_dig(";; ->>HEADER<<- opcode: QUERY, status: NXDOMAIN, id: 1\n").status, "NXDOMAIN");
    }

    #[test]
    fn picks_the_scoped_resolver() {
        let scutil = "DNS configuration\n\nresolver #1\n  search domain[0] : corp.example\n  nameserver[0] : 10.0.0.1\n\n\
                      resolver #2\n  domain   : local\n  options  : mdns\n\n\
                      resolver #3\n  domain   : corp.example\n  nameserver[0] : 172.16.0.2\n\n\
                      DNS configuration (for scoped queries)\n\nresolver #1\n  nameserver[0] : 9.9.9.9\n";
        let rs = parse_scutil(scutil);
        assert_eq!(rs.len(), 2);
        assert_eq!(resolver_for(&rs, "api.corp.example").unwrap().nameservers, ["172.16.0.2"]);
        assert_eq!(resolver_for(&rs, "example.com").unwrap().nameservers, ["10.0.0.1"]);
        assert_eq!(resolver_for(&rs, "example.com").unwrap().search, ["corp.example"]);
    }

    #[test]
    fn formats_ttls() {
        assert_eq!(ttl(86_400), "TTL 1d");
        assert_eq!(ttl(300), "TTL 5m");
        assert_eq!(ttl(50), "TTL 50s");
    }
}
