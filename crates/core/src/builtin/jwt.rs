use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use delight_sdk::{
    Action, Block, Detection, FieldKind, Input, KeyValueRow, NoticeLevel, Plugin, PluginError, PluginManifest,
    RunRequest, ToolOutput,
};
use hmac::{Hmac, Mac};
use serde_json::{Map, Value, json};
use sha2::{Sha256, Sha384, Sha512};

use super::{field, flag, manifest, op, param, select, toggle};
use crate::util::{format_unix, now_unix, preview, relative};

pub struct JwtPlugin {
    manifest: PluginManifest,
}

impl JwtPlugin {
    pub fn new() -> Self {
        let mut secret = field("secret", "Secret", FieldKind::Secret, None);
        secret.placeholder = Some("HMAC secret (optional, verifies HS256/384/512)".into());
        let mut sign_secret = field("secret", "Secret", FieldKind::Secret, None);
        sign_secret.placeholder = Some("HMAC secret".into());
        Self {
            manifest: manifest(
                "delight.jwt",
                "JWT",
                "Decode, inspect, verify and sign JSON Web Tokens.",
                "JWT",
                "#AF52DE",
                &["jwt", "token", "auth", "bearer"],
                vec![
                    op("decode", "Decode JWT", "Show header, payload, claims and expiry", &["jwt", "decode", "inspect"], vec![secret]),
                    op("encode", "Sign as JWT", "Sign a JSON payload with HMAC", &["jwt", "encode", "sign"], vec![
                        select("alg", "Algorithm", &[("HS256", "HS256"), ("HS384", "HS384"), ("HS512", "HS512")], "HS256"),
                        sign_secret,
                        toggle("iat", "Add iat", false),
                    ]),
                ],
            ),
        }
    }
}

impl Default for JwtPlugin {
    fn default() -> Self {
        Self::new()
    }
}

struct Parsed<'a> {
    header: Value,
    payload: Value,
    signing_input: &'a str,
    signature: &'a str,
}

fn strip_bearer(t: &str) -> &str {
    let t = t.trim();
    match t.get(..7) {
        Some(p) if p.eq_ignore_ascii_case("bearer ") => t[7..].trim(),
        _ => t,
    }
}

fn decode_part(part: &str) -> Option<Value> {
    let bytes = URL_SAFE_NO_PAD.decode(part.trim_end_matches('=')).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn parse(token: &str) -> Option<Parsed<'_>> {
    let token = strip_bearer(token);
    let mut parts = token.split('.');
    let (h, p, s) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() || h.is_empty() || p.is_empty() {
        return None;
    }
    let header = decode_part(h).filter(|v| v.get("alg").is_some())?;
    let payload = decode_part(p)?;
    Some(Parsed { header, payload, signing_input: &token[..h.len() + 1 + p.len()], signature: s })
}

fn hmac_sign(alg: &str, secret: &[u8], data: &[u8]) -> Option<Vec<u8>> {
    macro_rules! sign {
        ($h:ty) => {{
            let mut mac = Hmac::<$h>::new_from_slice(secret).ok()?;
            mac.update(data);
            Some(mac.finalize().into_bytes().to_vec())
        }};
    }
    match alg {
        "HS256" => sign!(Sha256),
        "HS384" => sign!(Sha384),
        "HS512" => sign!(Sha512),
        _ => None,
    }
}

fn claim_time(payload: &Value, key: &str) -> Option<i64> {
    payload.get(key)?.as_f64().map(|f| f as i64)
}

impl Plugin for JwtPlugin {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    fn detect(&self, input: &Input) -> Vec<Detection> {
        let mut out = Vec::new();
        if let Some(parsed) = parse(input.trimmed) {
            let alg = parsed.header.get("alg").and_then(Value::as_str).unwrap_or("?");
            let mut bits = vec![alg.to_string()];
            if let Some(sub) = parsed.payload.get("sub").and_then(Value::as_str) {
                bits.push(format!("sub {sub}"));
            }
            if let Some(exp) = claim_time(&parsed.payload, "exp") {
                let now = now_unix();
                bits.push(if exp < now { format!("expired {}", relative(exp, now)) } else { format!("expires {}", relative(exp, now)) });
            }
            out.push(Detection::new("decode", 0.98).reason("header.payload.signature").preview(bits.join(" · ")));
        } else if input.json().is_some_and(Value::is_object) {
            out.push(Detection::new("encode", 0.3).reason("JSON object can be a JWT payload"));
        }
        out
    }

    fn run(&self, req: &RunRequest) -> Result<ToolOutput, PluginError> {
        match req.operation_id.as_str() {
            "decode" => {
                let parsed = parse(&req.input).ok_or_else(|| PluginError::Invalid("not a JWT".into()))?;
                let now = now_unix();
                let alg = parsed.header.get("alg").and_then(Value::as_str).unwrap_or("none").to_string();

                let status = match (claim_time(&parsed.payload, "exp"), claim_time(&parsed.payload, "nbf")) {
                    (_, Some(nbf)) if nbf > now => {
                        Block::Notice { level: NoticeLevel::Warning, text: format!("Not valid yet — becomes valid {}", relative(nbf, now)) }
                    }
                    (Some(exp), _) if exp < now => {
                        Block::Notice { level: NoticeLevel::Error, text: format!("Expired {} ({})", relative(exp, now), format_unix(exp)) }
                    }
                    (Some(exp), _) => {
                        Block::Notice { level: NoticeLevel::Success, text: format!("Active — expires {} ({})", relative(exp, now), format_unix(exp)) }
                    }
                    (None, _) => Block::Notice { level: NoticeLevel::Info, text: "No `exp` claim — token never expires".into() },
                };

                let mut claims = Vec::new();
                let names = [
                    ("iss", "Issuer"),
                    ("sub", "Subject"),
                    ("aud", "Audience"),
                    ("jti", "JWT ID"),
                    ("iat", "Issued at"),
                    ("nbf", "Not before"),
                    ("exp", "Expires"),
                ];
                for (key, label) in names {
                    let Some(v) = parsed.payload.get(key) else { continue };
                    let row = match (key, v.as_f64()) {
                        ("iat" | "nbf" | "exp", Some(t)) => {
                            KeyValueRow::new(label, format_unix(t as i64)).hint(relative(t as i64, now))
                        }
                        _ => KeyValueRow::new(label, v.as_str().map(str::to_owned).unwrap_or_else(|| v.to_string())),
                    };
                    claims.push(row);
                }

                let secret = param(&self.manifest, req, "secret");
                let verification = if alg.eq_ignore_ascii_case("none") {
                    KeyValueRow::new("Signature", "unsigned (alg: none)")
                } else if secret.is_empty() {
                    KeyValueRow::new("Signature", "not verified").hint("enter the secret below to verify")
                } else {
                    match hmac_sign(&alg, secret.as_bytes(), parsed.signing_input.as_bytes()) {
                        Some(sig) if URL_SAFE_NO_PAD.encode(&sig) == parsed.signature.trim_end_matches('=') => {
                            KeyValueRow::new("Signature", "✓ verified")
                        }
                        Some(_) => KeyValueRow::new("Signature", "✗ invalid signature"),
                        None => KeyValueRow::new("Signature", format!("cannot verify {alg} with a shared secret")),
                    }
                };

                let header = super::json::pretty(&parsed.header, "2");
                let payload = super::json::pretty(&parsed.payload, "2");
                let mut out = ToolOutput::default().block(status).block(Block::code("Payload", "json", payload.clone()));
                if !claims.is_empty() {
                    out = out.block(Block::KeyValue { label: Some("Claims".into()), rows: claims });
                }
                Ok(out
                    .block(Block::code("Header", "json", header.clone()))
                    .block(Block::KeyValue {
                        label: Some("Signature".into()),
                        rows: vec![
                            KeyValueRow::new("Algorithm", alg),
                            verification,
                            KeyValueRow::new("Value", preview(parsed.signature, 48)),
                        ],
                    })
                    .block(Block::Field { key: "secret".into() })
                    .action(Action::copy("copy_payload", "Copy payload", payload.clone()).primary())
                    .action(Action::copy("copy_header", "Copy header", header))
                    .action(Action::replace_input("replace", "Edit payload", payload)))
            }
            "encode" => {
                let mut payload: Map<String, Value> = serde_json::from_str(req.input.trim())
                    .map_err(|e| PluginError::Invalid(format!("payload must be a JSON object: {e}")))?;
                if flag(&self.manifest, req, "iat") {
                    payload.insert("iat".into(), json!(now_unix()));
                }
                let alg = param(&self.manifest, req, "alg").to_string();
                let secret = param(&self.manifest, req, "secret");
                if secret.is_empty() {
                    return Ok(ToolOutput::notice(NoticeLevel::Info, "Enter a secret above to sign this payload."));
                }
                let header = json!({ "alg": alg, "typ": "JWT" });
                let signing_input = format!(
                    "{}.{}",
                    URL_SAFE_NO_PAD.encode(header.to_string()),
                    URL_SAFE_NO_PAD.encode(Value::Object(payload).to_string())
                );
                let sig = hmac_sign(&alg, secret.as_bytes(), signing_input.as_bytes())
                    .ok_or_else(|| PluginError::Invalid(format!("unsupported algorithm {alg}")))?;
                let token = format!("{signing_input}.{}", URL_SAFE_NO_PAD.encode(sig));
                Ok(ToolOutput::default()
                    .block(Block::code("Token", "text", token.clone()))
                    .block(Block::KeyValue {
                        label: None,
                        rows: vec![KeyValueRow::new("Header", header.to_string()), KeyValueRow::new("Length", token.len().to_string())],
                    })
                    .action(Action::copy("copy", "Copy token", token.clone()).primary())
                    .action(Action::copy("copy_bearer", "Copy as Bearer header", format!("Authorization: Bearer {token}")))
                    .action(Action::replace_input("replace", "Replace input", token)))
            }
            other => Err(PluginError::UnknownOperation(other.into())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // jwt.io sample, secret "your-256-bit-secret".
    const TOKEN: &str = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvaG4gRG9lIiwiaWF0IjoxNTE2MjM5MDIyfQ.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c";

    fn run(op: &str, input: &str, params: &[(&str, &str)]) -> ToolOutput {
        JwtPlugin::new()
            .run(&RunRequest::new(op, input).params(params.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()))
            .unwrap()
    }

    fn signature_row(out: &ToolOutput) -> String {
        out.blocks
            .iter()
            .find_map(|b| match b {
                Block::KeyValue { label: Some(l), rows } if l == "Signature" => Some(rows[1].value.clone()),
                _ => None,
            })
            .unwrap()
    }

    #[test]
    fn decodes_and_verifies() {
        assert_eq!(signature_row(&run("decode", TOKEN, &[("secret", "your-256-bit-secret")])), "✓ verified");
        assert_eq!(signature_row(&run("decode", TOKEN, &[("secret", "nope")])), "✗ invalid signature");
        assert!(parse(&format!("Bearer {TOKEN}")).is_some());
    }

    #[test]
    fn sign_then_verify() {
        let out = run("encode", r#"{"sub":"1234567890","name":"John Doe","iat":1516239022}"#, &[("secret", "your-256-bit-secret")]);
        let Block::Code { text, .. } = &out.blocks[0] else { panic!() };
        assert_eq!(signature_row(&run("decode", text, &[("secret", "your-256-bit-secret")])), "✓ verified");
    }
}
