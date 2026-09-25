use base64::Engine;
use base64::alphabet;
use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};
use delight_sdk::{
    Action, Block, Detection, Input, KeyValueRow, Plugin, PluginError, PluginManifest, RunRequest, ToolOutput,
};

use super::{flag, manifest, op, param, select, toggle};
use crate::util::preview;

const LENIENT: GeneralPurposeConfig = GeneralPurposeConfig::new()
    .with_decode_padding_mode(DecodePaddingMode::Indifferent)
    .with_decode_allow_trailing_bits(true);
const STANDARD_LENIENT: GeneralPurpose = GeneralPurpose::new(&alphabet::STANDARD, LENIENT);
const URL_LENIENT: GeneralPurpose = GeneralPurpose::new(&alphabet::URL_SAFE, LENIENT);

pub struct Base64Plugin {
    manifest: PluginManifest,
}

impl Base64Plugin {
    pub fn new() -> Self {
        Self {
            manifest: manifest(
                "delight.base64",
                "Base64",
                "Encode and decode Base64 (standard and URL-safe).",
                "64",
                "#34C759",
                &["base64", "encoding"],
                vec![
                    op("decode", "Decode Base64", "Decode Base64 / Base64URL to text or bytes", &["base64", "decode"], vec![]),
                    op("encode", "Encode Base64", "Encode text as Base64", &["base64", "encode"], vec![
                        select("variant", "Alphabet", &[("standard", "Standard"), ("url", "URL-safe")], "standard"),
                        toggle("padding", "Padding", true),
                    ]),
                ],
            ),
        }
    }
}

impl Default for Base64Plugin {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, PartialEq)]
enum Variant {
    Standard,
    Url,
}

/// Strict-ish shape check + decode. Whitespace (MIME line breaks) is ignored.
fn decode(text: &str) -> Option<(Vec<u8>, Variant)> {
    // Stops at the first character outside the alphabet, before copying anything.
    if !text.bytes().all(|b| b.is_ascii_alphanumeric() || b"+/-_=".contains(&b) || b.is_ascii_whitespace()) {
        return None;
    }
    let compact: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    if compact.len() < 4 {
        return None;
    }
    let body = compact.trim_end_matches('=');
    if compact.len() - body.len() > 2 || body.len() % 4 == 1 {
        return None;
    }
    let std_chars = body.bytes().any(|b| b == b'+' || b == b'/');
    let url_chars = body.bytes().any(|b| b == b'-' || b == b'_');
    if std_chars && url_chars || !body.bytes().all(|b| b.is_ascii_alphanumeric() || b"+/-_".contains(&b)) {
        return None;
    }
    if url_chars {
        URL_LENIENT.decode(body).ok().map(|b| (b, Variant::Url))
    } else {
        STANDARD_LENIENT.decode(body).ok().map(|b| (b, Variant::Standard))
    }
}

fn printable_text(bytes: &[u8]) -> Option<&str> {
    let s = std::str::from_utf8(bytes).ok()?;
    let total = s.chars().count().max(1);
    let printable = s.chars().filter(|c| !c.is_control() || matches!(c, '\n' | '\r' | '\t')).count();
    (printable * 100 / total >= 95).then_some(s)
}

fn hex_dump(bytes: &[u8], limit: usize) -> String {
    let mut out = String::new();
    for (i, chunk) in bytes.chunks(16).take(limit / 16).enumerate() {
        let hex: Vec<String> = chunk.iter().map(|b| format!("{b:02x}")).collect();
        let ascii: String = chunk.iter().map(|&b| if b.is_ascii_graphic() || b == b' ' { b as char } else { '.' }).collect();
        out.push_str(&format!("{:08x}  {:<47}  {}\n", i * 16, hex.join(" "), ascii));
    }
    if bytes.len() > limit {
        out.push_str(&format!("… {} more bytes", bytes.len() - limit));
    }
    out
}

impl Plugin for Base64Plugin {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    fn detect(&self, input: &Input) -> Vec<Detection> {
        let t = input.trimmed;
        let mut out = Vec::new();
        // A JWT is "base64url with dots": `decode` rejects the dots (not in the
        // alphabet) at the first one, so JWTs are left to the JWT plugin.
        if let Some((bytes, variant)) = decode(t) {
            let compact_len = t.chars().filter(|c| !c.is_whitespace()).count();
            let has_digit_or_symbol = t.bytes().any(|b| b.is_ascii_digit() || b"+/=-_".contains(&b));
            let has_mixed_case = t.bytes().any(|b| b.is_ascii_lowercase()) && t.bytes().any(|b| b.is_ascii_uppercase());
            if let Some(text) = printable_text(&bytes) {
                let looks_json = serde_json::from_str::<serde_json::Value>(text.trim())
                    .map(|v| v.is_object() || v.is_array())
                    .unwrap_or(false);
                let mut c: f32 = match compact_len {
                    0..8 => 0.3,
                    8..16 => 0.6,
                    _ => 0.82,
                };
                if has_digit_or_symbol && has_mixed_case {
                    c += 0.08;
                }
                if !has_digit_or_symbol && compact_len < 24 {
                    // Plain words ("password") happen to be valid base64.
                    c = c.min(0.25);
                }
                if looks_json {
                    c = 0.95;
                }
                let reason = match (looks_json, variant) {
                    (true, _) => "decodes to JSON",
                    (_, Variant::Url) => "base64url alphabet, decodes to UTF-8 text",
                    _ => "decodes to UTF-8 text",
                };
                out.push(Detection::new("decode", c).reason(reason).preview(preview(text, 80)));
            } else if compact_len >= 16 && has_mixed_case {
                out.push(
                    Detection::new("decode", 0.35)
                        .reason("decodes to binary data")
                        .preview(format!("binary · {} bytes", bytes.len())),
                );
            }
        }
        if !t.is_empty() {
            // Base64 of the first 60 bytes (a multiple of 3) is exactly the first
            // 80 characters of the full encoding: enough for the preview.
            let head = &t.as_bytes()[..t.len().min(60)];
            let more = if t.len() > 60 { "…" } else { "" };
            out.push(Detection::new("encode", 0.12).preview(format!("{}{more}", STANDARD_LENIENT.encode(head))));
        }
        out
    }

    fn run(&self, req: &RunRequest) -> Result<ToolOutput, PluginError> {
        match req.operation_id.as_str() {
            "decode" => {
                let (bytes, variant) =
                    decode(req.input.trim()).ok_or_else(|| PluginError::Invalid("not valid Base64".into()))?;
                let variant_label = if variant == Variant::Url { "URL-safe" } else { "Standard" };
                let mut out = ToolOutput::default();
                match std::str::from_utf8(&bytes) {
                    Ok(text) => {
                        let json = serde_json::from_str::<serde_json::Value>(text.trim())
                            .ok()
                            .filter(|v| v.is_object() || v.is_array());
                        let (lang, shown) = match &json {
                            Some(v) => ("json", super::json::pretty(v, "2")),
                            None => ("text", text.to_string()),
                        };
                        out = out
                            .block(Block::code("Decoded", lang, shown.clone()))
                            .action(Action::copy("copy", "Copy decoded", text.to_string()).primary());
                        if json.is_some() {
                            out = out.action(Action::copy("copy_pretty", "Copy formatted JSON", shown));
                        }
                        out = out.action(Action::replace_input("replace", "Replace input", text.to_string()));
                    }
                    Err(_) => {
                        let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
                        out = out
                            .block(Block::code("Hex dump", "text", hex_dump(&bytes, 4096)))
                            .action(Action::copy("copy_hex", "Copy as hex", hex).primary());
                    }
                }
                Ok(out.block(Block::KeyValue {
                    label: Some("Details".into()),
                    rows: vec![
                        KeyValueRow::new("Alphabet", variant_label),
                        KeyValueRow::new("Decoded size", format!("{} bytes", bytes.len())),
                        KeyValueRow::new("UTF-8", if std::str::from_utf8(&bytes).is_ok() { "yes" } else { "no (binary)" }),
                    ],
                }))
            }
            "encode" => {
                let url = param(&self.manifest, req, "variant") == "url";
                let pad = flag(&self.manifest, req, "padding");
                let engine = GeneralPurpose::new(
                    if url { &alphabet::URL_SAFE } else { &alphabet::STANDARD },
                    GeneralPurposeConfig::new().with_encode_padding(pad),
                );
                let encoded = engine.encode(req.input.as_bytes());
                Ok(ToolOutput::default()
                    .block(Block::code("Encoded", "text", encoded.clone()))
                    .action(Action::copy("copy", "Copy encoded", encoded.clone()).primary())
                    .action(Action::replace_input("replace", "Replace input", encoded)))
            }
            other => Err(PluginError::UnknownOperation(other.into())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode_conf(s: &str) -> Option<f32> {
        Base64Plugin::new()
            .detect(&Input::new(s))
            .into_iter()
            .find(|d| d.operation_id == "decode")
            .map(|d| d.confidence)
    }

    #[test]
    fn detection_heuristics() {
        assert!(decode_conf("aGVsbG8gd29ybGQ=").unwrap() > 0.8);
        assert!(decode_conf("eyJhIjoxfQ").unwrap() > 0.9); // {"a":1}
        assert!(decode_conf("password").unwrap_or(0.0) < 0.3);
        assert!(decode_conf("hello world").is_none());
        assert!(decode_conf("a.b.c").is_none());
    }

    #[test]
    fn roundtrip_url_safe() {
        let p = Base64Plugin::new();
        let enc = p
            .run(&RunRequest::new("encode", "??>>").params(
                [("variant".to_string(), "url".to_string()), ("padding".into(), "false".into())].into(),
            ))
            .unwrap();
        let Block::Code { text, .. } = &enc.blocks[0] else { panic!() };
        assert_eq!(text, "Pz8-Pg");
        assert_eq!(decode(text).unwrap(), (b"??>>".to_vec(), Variant::Url));
    }
}
