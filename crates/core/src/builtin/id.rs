//! IDs: generate by keyword and decode pasted ones.
//!
//! Generating has a tab per kind (UUID v4 · UUID v7 · ULID · NanoID ·
//! ObjectId · Password · Secret); the keyword typed opens the matching tab
//! (`uuid`/`guid` → v4, `uuid7`/`uuid v7` → v7, `secret 40` → Secret, 40
//! chars). A number after a space is the count, or the length for Password
//! and Secret. Decoding: UUID version/variant and v1/v6/v7 time, ULID time,
//! ObjectId time.
//!
//! Randomness comes from `/dev/urandom` (the OS CSPRNG); passwords and NanoIDs
//! sample their alphabets without bias. The formats are small enough to
//! implement exactly here (RFC 9562 UUIDs, the ULID spec, MongoDB ObjectId).

use std::io::Read;
use std::time::{SystemTime, UNIX_EPOCH};

use delight_sdk::{
    Action, Block, Detection, Input, KeyValueRow, NoticeLevel, Plugin, PluginError, PluginManifest, RunRequest,
    ToolOutput,
};

use super::{manifest, op, select};
use crate::util::{format_unix, now_unix, relative};

pub struct IdPlugin {
    manifest: PluginManifest,
}

impl IdPlugin {
    pub fn new() -> Self {
        Self {
            manifest: manifest(
                "delight.id",
                "IDs",
                "Generate UUIDs, ULIDs, NanoIDs, ObjectIds and passwords; decode pasted IDs.",
                "ID",
                "#5AC8FA",
                &["id", "uuid", "ulid", "nanoid", "objectid", "password", "generate"],
                vec![
                    op(
                        "generate",
                        "Generate IDs",
                        "Generate UUIDs, ULIDs, NanoIDs, ObjectIds, passwords or secrets",
                        &["uuid", "ulid", "nanoid", "password", "secret", "generate"],
                        vec![select("kind", "Kind", &Kind::ALL.map(|k| (k.value(), k.label())), Kind::UuidV4.value())],
                    )
                    .mode("kind"),
                    op("decode", "Decode ID", "Version and embedded time of a UUID, ULID or ObjectId", &["uuid", "ulid", "objectid"], vec![]),
                ],
            ),
        }
    }
}

impl Default for IdPlugin {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Randomness
// ---------------------------------------------------------------------------

fn random_bytes<const N: usize>() -> [u8; N] {
    let mut buf = [0u8; N];
    std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut buf)).expect("reading /dev/urandom");
    buf
}

/// `len` characters drawn uniformly from `alphabet` (rejection sampling: no
/// modulo bias for alphabets that aren't a power of two).
fn random_string(alphabet: &[u8], len: usize) -> String {
    let limit = 256 - 256 % alphabet.len();
    let mut out = String::with_capacity(len);
    while out.len() < len {
        for b in random_bytes::<64>() {
            if (b as usize) < limit && out.len() < len {
                out.push(alphabet[b as usize % alphabet.len()] as char);
            }
        }
    }
    out
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Formats
// ---------------------------------------------------------------------------

fn uuid_string(b: [u8; 16]) -> String {
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])
}

fn uuid_v4() -> String {
    let mut b = random_bytes::<16>();
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    uuid_string(b)
}

/// Time-ordered: 48-bit Unix milliseconds, then random (RFC 9562 §5.7).
fn uuid_v7() -> String {
    let mut b = random_bytes::<16>();
    b[..6].copy_from_slice(&now_ms().to_be_bytes()[2..]);
    b[6] = (b[6] & 0x0f) | 0x70;
    b[8] = (b[8] & 0x3f) | 0x80;
    uuid_string(b)
}

const CROCKFORD: &[u8] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// 48-bit Unix milliseconds + 80 random bits, Crockford base32 (26 chars).
fn ulid() -> String {
    let r = random_bytes::<10>();
    let mut v: u128 = u128::from(now_ms()) << 80;
    for (i, b) in r.iter().enumerate() {
        v |= u128::from(*b) << (8 * (9 - i));
    }
    (0..26).map(|i| CROCKFORD[((v >> (5 * (25 - i))) & 31) as usize] as char).collect()
}

fn nanoid() -> String {
    random_string(b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789_-", 21)
}

/// 4-byte seconds, 5 random bytes, 3-byte counter (random start).
fn objectid() -> String {
    let secs = (now_ms() / 1000) as u32;
    let r = random_bytes::<8>();
    let mut b = [0u8; 12];
    b[..4].copy_from_slice(&secs.to_be_bytes());
    b[4..].copy_from_slice(&r);
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Letters and digits only: safe in URLs, env files and configs.
fn secret(len: usize) -> String {
    random_string(b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789", len)
}

/// At least one of each class, then shuffled.
fn password(len: usize, symbols: bool) -> String {
    const UPPER: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ";
    const LOWER: &[u8] = b"abcdefghijkmnopqrstuvwxyz";
    const DIGITS: &[u8] = b"23456789";
    const SYMBOLS: &[u8] = b"!@#$%^&*()-_=+[]{};:,.?";
    let mut classes = vec![UPPER, LOWER, DIGITS];
    if symbols {
        classes.push(SYMBOLS);
    }
    let all: Vec<u8> = classes.concat();
    let mut chars: Vec<char> = classes.iter().map(|c| random_string(c, 1).remove(0)).collect();
    chars.extend(random_string(&all, len.saturating_sub(chars.len())).chars());
    // Fisher–Yates with unbiased indices.
    for i in (1..chars.len()).rev() {
        let j = loop {
            let r = u16::from_be_bytes(random_bytes::<2>()) as usize;
            let limit = 65536 - 65536 % (i + 1);
            if r < limit {
                break r % (i + 1);
            }
        };
        chars.swap(i, j);
    }
    chars.into_iter().collect()
}

// ---------------------------------------------------------------------------
// Parsing input
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind {
    UuidV4,
    UuidV7,
    Ulid,
    NanoId,
    ObjectId,
    Password,
    Secret,
}

impl Kind {
    const ALL: [Kind; 7] = [Kind::UuidV4, Kind::UuidV7, Kind::Ulid, Kind::NanoId, Kind::ObjectId, Kind::Password, Kind::Secret];

    fn label(self) -> &'static str {
        match self {
            Kind::UuidV4 => "UUID v4",
            Kind::UuidV7 => "UUID v7",
            Kind::Ulid => "ULID",
            Kind::NanoId => "NanoID",
            Kind::ObjectId => "ObjectId",
            Kind::Password => "Password",
            Kind::Secret => "Secret",
        }
    }

    /// The tab's value.
    fn value(self) -> &'static str {
        match self {
            Kind::UuidV4 => "uuid4",
            Kind::UuidV7 => "uuid7",
            Kind::Ulid => "ulid",
            Kind::NanoId => "nanoid",
            Kind::ObjectId => "objectid",
            Kind::Password => "password",
            Kind::Secret => "secret",
        }
    }

    fn from_value(v: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| k.value() == v)
    }

    /// Password and Secret take a length; the rest a count.
    fn sized(self) -> bool {
        matches!(self, Kind::Password | Kind::Secret)
    }

    fn generate(self, len: usize) -> String {
        match self {
            Kind::UuidV4 => uuid_v4(),
            Kind::UuidV7 => uuid_v7(),
            Kind::Ulid => ulid(),
            Kind::NanoId => nanoid(),
            Kind::ObjectId => objectid(),
            Kind::Password => password(len, true),
            Kind::Secret => secret(len),
        }
    }
}

/// What the input asks for: `kind` is the tab it names (`None` for a bare
/// `id`: keep the current tab); `number` a count, or a length for Password and
/// Secret; `note` explains a reading that isn't literal; `partial` when the
/// first word is only the start of a keyword (`obj` → objectid).
#[derive(Debug, Clone, PartialEq)]
struct Request {
    kind: Option<Kind>,
    number: Option<usize>,
    note: Option<String>,
    partial: bool,
}

/// Keywords and the tab each opens (`None`: any tab).
const KEYWORDS: &[(&str, Option<Kind>)] = &[
    ("uuid", Some(Kind::UuidV4)),
    ("guid", Some(Kind::UuidV4)),
    ("uuidv4", Some(Kind::UuidV4)),
    ("uuid4", Some(Kind::UuidV4)),
    ("guid4", Some(Kind::UuidV4)),
    ("uuidv7", Some(Kind::UuidV7)),
    ("uuid7", Some(Kind::UuidV7)),
    ("guid7", Some(Kind::UuidV7)),
    ("ulid", Some(Kind::Ulid)),
    ("nanoid", Some(Kind::NanoId)),
    ("objectid", Some(Kind::ObjectId)),
    ("oid", Some(Kind::ObjectId)),
    ("mongoid", Some(Kind::ObjectId)),
    ("password", Some(Kind::Password)),
    ("pass", Some(Kind::Password)),
    ("pwd", Some(Kind::Password)),
    ("secret", Some(Kind::Secret)),
    ("token", Some(Kind::Secret)),
    ("apikey", Some(Kind::Secret)),
    ("id", None),
    ("ids", None),
];

/// The tab for `word`: an exact keyword, or — for 2+ characters — the start
/// of one (`obj` → objectid, `sec` → secret). When several keywords start
/// that way the shortest decides (`uu` → `uuid` → v4, like typing `uuid`).
/// `Err` if nothing matches.
fn keyword(word: &str) -> Result<(Option<Kind>, bool), ()> {
    if let Some((_, k)) = KEYWORDS.iter().find(|(w, _)| *w == word) {
        return Ok((*k, false));
    }
    if word.len() < 2 {
        return Err(());
    }
    KEYWORDS
        .iter()
        .filter(|(w, _)| w.starts_with(word))
        .min_by_key(|(w, _)| w.len())
        .map(|(_, k)| (*k, true))
        .ok_or(())
}

fn parse_request(input: &str) -> Option<Request> {
    let t = input.trim();
    if t.len() > 40 {
        return None;
    }
    let lower = t.to_ascii_lowercase();
    let mut words = lower.split_whitespace();
    let first = words.next()?;
    let (mut number, mut note, mut partial) = (None, None, false);
    let mut kind = match keyword(first) {
        Ok((kind, is_partial)) => {
            partial = is_partial;
            kind
        }
        // `uuid5`, `uuid12`: digits glued to uuid/guid are a count — except
        // name-based versions 3 and 5, which can't be generated randomly.
        Err(()) => {
            let digits = first.strip_prefix("uuid").or_else(|| first.strip_prefix("guid"))?;
            let n: usize = digits.parse().ok()?;
            if matches!(n, 3 | 5) {
                note = Some(format!("UUID v{n} is name-based, not random — showing {n} UUID v4s instead"));
            }
            number = Some(n);
            Some(Kind::UuidV4)
        }
    };
    for w in words {
        match w {
            "v4" if kind.is_some_and(|k| matches!(k, Kind::UuidV4 | Kind::UuidV7)) => kind = Some(Kind::UuidV4),
            "v7" if kind.is_some_and(|k| matches!(k, Kind::UuidV4 | Kind::UuidV7)) => kind = Some(Kind::UuidV7),
            n => number = Some(n.parse::<usize>().ok()?),
        }
    }
    Some(Request { kind, number, note, partial })
}

/// A pasted ID: its type and fields.
#[derive(Debug, Clone, PartialEq)]
enum Parsed {
    Uuid { bytes: [u8; 16] },
    Ulid { ms: u64 },
    ObjectId { secs: u32 },
}

fn hex_bytes<const N: usize>(s: &str) -> Option<[u8; N]> {
    if s.len() != N * 2 || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let mut out = [0u8; N];
    for (i, o) in out.iter_mut().enumerate() {
        *o = u8::from_str_radix(&s[2 * i..2 * i + 2], 16).ok()?;
    }
    Some(out)
}

fn parse_id(input: &str) -> Option<Parsed> {
    let t = input.trim().trim_matches(|c| c == '{' || c == '}' || c == '"');
    let t = t.strip_prefix("urn:uuid:").unwrap_or(t);
    match t.len() {
        36 if [8, 13, 18, 23].iter().all(|&i| t.as_bytes()[i] == b'-') => {
            hex_bytes::<16>(&t.replace('-', "")).map(|bytes| Parsed::Uuid { bytes })
        }
        32 => hex_bytes::<16>(t).map(|bytes| Parsed::Uuid { bytes }),
        26 => {
            let up = t.to_ascii_uppercase();
            if !matches!(up.as_bytes()[0], b'0'..=b'7') {
                return None;
            }
            let mut v: u128 = 0;
            for c in up.bytes() {
                v = (v << 5) | CROCKFORD.iter().position(|&x| x == c)? as u128;
            }
            Some(Parsed::Ulid { ms: (v >> 80) as u64 })
        }
        24 => hex_bytes::<12>(t).map(|b| Parsed::ObjectId { secs: u32::from_be_bytes([b[0], b[1], b[2], b[3]]) }),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Output
// ---------------------------------------------------------------------------

/// `2026-09-25 10:00:00.123 UTC · 3h ago`
fn time_row(label: &str, ms: i64) -> KeyValueRow {
    let secs = ms.div_euclid(1000);
    let text = format_unix(secs).replace(" UTC", &format!(".{:03} UTC", ms.rem_euclid(1000)));
    KeyValueRow::new(label, text).hint(relative(secs, now_unix()))
}

/// Timestamp embedded in a UUID (v1, v6: 100 ns since 1582-10-15; v7: Unix ms).
fn uuid_time_ms(b: &[u8; 16]) -> Option<i64> {
    const GREGORIAN_OFFSET: i64 = 0x01B2_1DD2_1381_4000; // 100 ns units, 1582-10-15 → 1970-01-01
    let be = |r: std::ops::Range<usize>| b[r].iter().fold(0i64, |acc, x| (acc << 8) | i64::from(*x));
    match b[6] >> 4 {
        1 => Some(((be(6..8) & 0x0fff) << 48 | be(4..6) << 32 | be(0..4)) - GREGORIAN_OFFSET).map(|t| t / 10_000),
        6 => Some((be(0..4) << 28 | be(4..6) << 12 | (be(6..8) & 0x0fff)) - GREGORIAN_OFFSET).map(|t| t / 10_000),
        7 => Some(be(0..6)),
        _ => None,
    }
}

fn describe(parsed: &Parsed) -> ToolOutput {
    let mut rows = Vec::new();
    let title;
    match parsed {
        Parsed::Uuid { bytes } => {
            let version = bytes[6] >> 4;
            let variant = match bytes[8] >> 6 {
                0b10 => "RFC 9562",
                0b11 => "Microsoft (reserved)",
                _ => "NCS (reserved)",
            };
            let kind = match version {
                1 => "time-based (MAC)",
                3 => "name-based (MD5)",
                4 => "random",
                5 => "name-based (SHA-1)",
                6 => "time-based, sortable",
                7 => "Unix time, sortable",
                8 => "custom",
                _ => "unknown",
            };
            let nil = bytes.iter().all(|b| *b == 0);
            title = if nil { "Nil UUID".to_string() } else { format!("UUID v{version} — {kind}") };
            rows.push(KeyValueRow::new("Canonical", uuid_string(*bytes)));
            rows.push(KeyValueRow::new("Version", version.to_string()).hint(kind));
            rows.push(KeyValueRow::new("Variant", variant));
            if let Some(ms) = uuid_time_ms(bytes) {
                rows.push(time_row("Created", ms));
            }
        }
        Parsed::Ulid { ms } => {
            title = "ULID".into();
            rows.push(time_row("Created", *ms as i64));
        }
        Parsed::ObjectId { secs } => {
            title = "MongoDB ObjectId".into();
            rows.push(time_row("Created", i64::from(*secs) * 1000));
        }
    }
    let created = rows.iter().find(|r| r.key == "Created").map(|r| r.value.clone());
    let mut out = ToolOutput::default().block(Block::KeyValue { label: Some(title), rows });
    if let Some(created) = created {
        out = out.action(Action::copy("copy_time", "Copy time", created).primary());
    }
    out
}

impl Plugin for IdPlugin {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    fn detect(&self, input: &Input) -> Vec<Detection> {
        let t = input.trimmed;
        // Cheap shape check: every accepted form is short.
        if t.is_empty() || t.len() > 48 {
            return Vec::new();
        }
        if let Some(r) = parse_request(t) {
            // A partly typed keyword is suggested, but below real matches.
            let d = Detection::new("generate", if r.partial { 0.6 } else { 0.9 });
            return vec![match r.kind {
                Some(k) => d.mode(k.value()).preview(format!("Generate {}", k.label())),
                None => d.preview("Generate IDs"),
            }];
        }
        match parse_id(t) {
            Some(Parsed::Uuid { bytes }) => {
                vec![Detection::new("decode", 0.85).preview(format!("UUID v{}", bytes[6] >> 4))]
            }
            // 26-char / 24-hex strings could be other things: lower.
            Some(Parsed::Ulid { .. }) => vec![Detection::new("decode", 0.55).preview("ULID")],
            Some(Parsed::ObjectId { .. }) => vec![Detection::new("decode", 0.55).preview("ObjectId")],
            None => Vec::new(),
        }
    }

    fn run(&self, req: &RunRequest) -> Result<ToolOutput, PluginError> {
        match req.operation_id.as_str() {
            "generate" => {
                let r = parse_request(&req.input).unwrap_or(Request { kind: None, number: None, note: None, partial: false });
                // The tab decides (it was opened from the input's keyword).
                let kind = Kind::from_value(&req.params.get("kind").cloned().unwrap_or_default())
                    .or(r.kind)
                    .unwrap_or(Kind::UuidV4);
                let mut out = ToolOutput::default();
                if let Some(note) = r.note.clone().filter(|_| kind == Kind::UuidV4) {
                    out = out.block(Block::Notice { level: NoticeLevel::Info, text: note });
                }
                if kind.sized() {
                    let default = if kind == Kind::Password { 20 } else { 32 };
                    let len = r.number.unwrap_or(default).clamp(8, 256);
                    let v = kind.generate(len);
                    out = out
                        .block(Block::code(format!("{} · {len} characters", kind.label()), "text", v.clone()))
                        .action(Action::copy("copy", format!("Copy {}", kind.label().to_lowercase()), v).primary());
                } else {
                    let asked = r.number.unwrap_or(1);
                    let count = asked.clamp(1, 100);
                    if asked > 100 {
                        out = out.block(Block::Notice { level: NoticeLevel::Info, text: "Capped at 100 IDs".into() });
                    }
                    let values: Vec<String> = (0..count).map(|_| kind.generate(0)).collect();
                    let label = if count == 1 { kind.label().to_string() } else { format!("{} × {count}", kind.label()) };
                    let all = values.join("\n");
                    out = out
                        .block(Block::code(label, "text", all.clone()))
                        .action(Action::copy("copy", if count == 1 { "Copy" } else { "Copy all" }, all).primary());
                    if count > 1 {
                        out = out.action(Action::copy("copy_first", "Copy first", values[0].clone()));
                    }
                }
                Ok(out.action(Action::rerun("regenerate", "Regenerate")))
            }
            "decode" => {
                let parsed = parse_id(&req.input).ok_or_else(|| PluginError::Invalid("not a UUID, ULID or ObjectId".into()))?;
                Ok(describe(&parsed))
            }
            other => Err(PluginError::UnknownOperation(other.into())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_valid_ids() {
        for _ in 0..50 {
            let v4 = uuid_v4();
            let Some(Parsed::Uuid { bytes }) = parse_id(&v4) else { panic!("{v4}") };
            assert_eq!((bytes[6] >> 4, bytes[8] >> 6), (4, 0b10));

            let v7 = uuid_v7();
            let Some(Parsed::Uuid { bytes }) = parse_id(&v7) else { panic!("{v7}") };
            assert_eq!(bytes[6] >> 4, 7);
            assert!((uuid_time_ms(&bytes).unwrap() - now_ms() as i64).abs() < 5_000);

            let u = ulid();
            assert_eq!(u.len(), 26);
            let Some(Parsed::Ulid { ms }) = parse_id(&u) else { panic!("{u}") };
            assert!((ms as i64 - now_ms() as i64).abs() < 5_000);

            let n = nanoid();
            assert_eq!(n.len(), 21);
            assert!(n.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-'));

            let o = objectid();
            let Some(Parsed::ObjectId { secs }) = parse_id(&o) else { panic!("{o}") };
            assert!((i64::from(secs) - now_unix()).abs() < 5);

            let p = password(20, true);
            assert_eq!(p.chars().count(), 20);
            assert!(p.chars().any(|c| c.is_ascii_uppercase()) && p.chars().any(|c| c.is_ascii_lowercase()));
            assert!(p.chars().any(|c| c.is_ascii_digit()) && p.chars().any(|c| !c.is_ascii_alphanumeric()));
        }
        assert_ne!(uuid_v4(), uuid_v4());
    }

    #[test]
    fn reads_requests() {
        let kind = |s: &str| parse_request(s).map(|r| (r.kind, r.number));
        for s in ["uuid", "guid", "uuid4", "uuidv4", "UUID", "uuid v4"] {
            assert_eq!(kind(s), Some((Some(Kind::UuidV4), None)), "{s}");
        }
        for s in ["uuid7", "uuidv7", "uuid v7", "guid7"] {
            assert_eq!(kind(s), Some((Some(Kind::UuidV7), None)), "{s}");
        }
        assert_eq!(kind("uuid 5"), Some((Some(Kind::UuidV4), Some(5))));
        assert_eq!(kind("uuid v7 10"), Some((Some(Kind::UuidV7), Some(10))));
        assert_eq!(kind("uuid12"), Some((Some(Kind::UuidV4), Some(12))));
        assert!(parse_request("uuid5").unwrap().note.unwrap().contains("name-based"));
        assert_eq!(kind("secret 40"), Some((Some(Kind::Secret), Some(40))));
        assert_eq!(kind("password 24"), Some((Some(Kind::Password), Some(24))));
        assert_eq!(kind("id"), Some((None, None)));
        assert_eq!(kind("uuid please"), None);
        assert_eq!(kind("identity"), None);
        // Typing a keyword: its start (2+ chars) already opens the tab.
        for (typed, k) in [("ob", Kind::ObjectId), ("obj", Kind::ObjectId), ("objec", Kind::ObjectId), ("sec", Kind::Secret), ("ul", Kind::Ulid), ("pas", Kind::Password), ("uu", Kind::UuidV4), ("nan", Kind::NanoId)] {
            let r = parse_request(typed).unwrap();
            assert_eq!((r.kind, r.partial), (Some(k), true), "{typed}");
        }
        assert_eq!(kind("obj 3"), Some((Some(Kind::ObjectId), Some(3))));
        assert!(parse_request("o").is_none(), "one letter is too little");
        assert!(parse_request("pa").is_some(), "only password starts with pa");
        assert!(!parse_request("objectid").unwrap().partial);
    }

    fn generate(input: &str, tab: Option<&str>) -> ToolOutput {
        let params = tab.map(|t| [("kind".to_string(), t.to_string())].into_iter().collect()).unwrap_or_default();
        IdPlugin::new()
            .run(&RunRequest::new("generate", input).params(params))
            .unwrap()
    }

    fn code(out: &ToolOutput) -> String {
        out.blocks.iter().find_map(|b| match b {
            Block::Code { text, .. } => Some(text.clone()),
            _ => None,
        })
        .unwrap()
    }

    #[test]
    fn keyword_opens_the_matching_tab_and_the_tab_decides() {
        let det = |s: &str| IdPlugin::new().detect(&Input::new(s))[0].mode.clone();
        assert_eq!(det("uuid").as_deref(), Some("uuid4"));
        assert_eq!(det("uuid7").as_deref(), Some("uuid7"));
        assert_eq!(det("secret 40").as_deref(), Some("secret"));
        assert_eq!(det("id"), None);
        // The tab (host param) wins over the keyword: the user switched tabs.
        let ulids = code(&generate("uuid 3", Some("ulid")));
        assert_eq!(ulids.lines().count(), 3);
        assert!(ulids.lines().all(|l| matches!(parse_id(l), Some(Parsed::Ulid { .. }))));
        // Secrets: exact length, letters and digits only.
        let s = code(&generate("secret 40", Some("secret")));
        assert_eq!(s.len(), 40);
        assert!(s.bytes().all(|b| b.is_ascii_alphanumeric()));
        assert_eq!(code(&generate("secret", Some("secret"))).len(), 32);
    }

    #[test]
    fn decodes_known_ids() {
        // RFC 9562 test vectors.
        let v1 = parse_id("C232AB00-9414-11EC-B3C8-9F6BDECED846").unwrap();
        let Parsed::Uuid { bytes } = v1 else { panic!() };
        assert_eq!(uuid_time_ms(&bytes).unwrap() / 1000, 1_645_557_742); // 2022-02-22 19:22:22 UTC
        let Parsed::Uuid { bytes } = parse_id("1EC9414C-232A-6B00-B3C8-9F6BDECED846").unwrap() else { panic!() };
        assert_eq!(uuid_time_ms(&bytes).unwrap() / 1000, 1_645_557_742);
        let Parsed::Uuid { bytes } = parse_id("017F22E2-79B0-7CC3-98C4-DC0C0C07398F").unwrap() else { panic!() };
        assert_eq!(uuid_time_ms(&bytes).unwrap(), 1_645_557_742_000);
        // ObjectId: 0x507f1f77 = 2012-10-17 21:13:27 UTC.
        assert_eq!(parse_id("507f1f77bcf86cd799439011"), Some(Parsed::ObjectId { secs: 0x507f_1f77 }));
        // ULID spec example: 01ARZ3NDEK… = 2016-07-30 23:54:10.259 UTC.
        assert_eq!(parse_id("01ARZ3NDEKTSV4RRFFQ69G5FAV"), Some(Parsed::Ulid { ms: 1_469_922_850_259 }));
        assert_eq!(parse_id("not an id"), None);
    }

    #[test]
    fn detection() {
        let d = |s: &str| IdPlugin::new().detect(&Input::new(s)).into_iter().map(|d| d.operation_id).collect::<Vec<_>>();
        assert_eq!(d("uuid 3"), ["generate"]);
        assert_eq!(d("uuid7"), ["generate"]);
        assert_eq!(d("550e8400-e29b-41d4-a716-446655440000"), ["decode"]);
        assert!(d("hello world").is_empty());
        assert!(d(&"x".repeat(1000)).is_empty());
    }
}
