//! Support bundle text scrubbing. The parser preserves delimiters so JSON stays valid.

use std::path::Path;

const MASK: &str = "[REDACTED]";
const MAX_LINE_BYTES: usize = 64 * 1024;
/// Bare auth schemes: a header value that is only one of these has its credential on the next
/// line. Single list for the unquoted and the shell-quoted continuation paths.
const AUTH_SCHEMES: [&str; 3] = ["bearer", "basic", "token"];

fn redact_json(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(fields) => {
            for (key, value) in fields {
                if sensitive_key(key) {
                    *value = serde_json::Value::String(MASK.into());
                } else {
                    redact_json(value);
                }
            }
        }
        serde_json::Value::Array(items) => {
            let mut mask_next = false;
            for item in items {
                if mask_next {
                    *item = serde_json::Value::String(MASK.into());
                    mask_next = false;
                } else if let Some(arg) = item.as_str() {
                    if let Some(key) = arg.strip_prefix("--") {
                        if let Some((key, _)) = key.split_once('=') {
                            if sensitive_key(key) {
                                *item = serde_json::Value::String(format!("--{key}={MASK}"));
                            }
                        } else if sensitive_key(key) {
                            mask_next = true;
                        }
                    }
                }
                redact_json(item);
            }
        }
        _ => {}
    }
}

fn sensitive_key(key: &str) -> bool {
    let normalized: String = key
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect();
    [
        "password",
        "passwd",
        "secret",
        "apikey",
        "accesskey",
        "credential",
        "privatekey",
        "clientkeydata",
        "clientcertificatedata",
        "certificateauthoritydata",
        "authorization",
        "cookie",
        "passphrase",
        "dockerconfigjson",
        "signingkey",
        "encryptionkey",
    ]
    .iter()
    .any(|term| normalized.contains(term))
        || key
            .to_ascii_lowercase()
            .match_indices("token")
            .any(|(start, _)| {
                let end = start + "token".len();
                end == key.len()
                    || !key.as_bytes()[end].is_ascii_alphanumeric()
                    || key.as_bytes()[end].is_ascii_uppercase()
            })
        || matches!(
            normalized.as_str(),
            "pwd" | "pass" | "auth" | "session" | "xamzsignature"
        )
        || normalized.ends_with("pass")
        || (normalized.ends_with("key")
            && !["monkey", "keyboardkey", "hotkey", "sortkey", "primarykey"]
                .contains(&normalized.as_str()))
}

fn is_key_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.')
}

fn is_word_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'-'
}

fn quoted_end(bytes: &[u8], start: usize, quote: u8) -> Option<usize> {
    let mut i = start;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            i += 2;
        } else if bytes[i] == quote {
            return Some(i);
        } else {
            i += 1;
        }
    }
    None
}

/// End of a shell-quoted value whose closing quote never appears on the line. The line may be a
/// JSON string body, so stop at the first unescaped `"`/`,`/`}`/`]` (kept) rather than eating
/// the JSON structure; plain text without any of them runs to end of line.
fn unclosed_value_end(bytes: &[u8], start: usize) -> usize {
    let mut i = start;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            b'"' | b',' | b'}' | b']' => return i,
            _ => i += 1,
        }
    }
    bytes.len()
}

// Accepted limits of the shell-quote handling below (each: reason, cost). None is pinned as
// correct behaviour by a test except the over-redaction ones.
// - Prose `"password: not set"` -> `"password: [REDACTED] set"`: a short real credential after
//   `key: ` is indistinguishable from prose; narrowing reopens the leak. Cost: a masked word.
// - A shell quote left open across a BLANK, whitespace-only or oversized (>64KB) line
//   (`'Authorization: Basic` NEWLINE blank NEWLINE `SECRET'`) is not followed: the carry lasts
//   exactly one line so a stray apostrophe cannot mask unrelated text. Cost: leak of that
//   multi-line, separated shape.
// - The carry also fires after a bare `'<sensitive-key>:` / `'<sensitive-key>=` at end of line
//   (not only Authorization), so `echo 'password=` masks the whole next line. Cost: one
//   unrelated line masked.
// - `"Authorization:" Basic SECRET` (UNQUOTED follow-up after a split quote) and
//   `curl -u user:SECRET` are not recognised: no key=value shape to anchor on without
//   masking every word after any quote. Cost: leak.
// - An unclosed quote ends its value at the first `,` `}` `]` `"`, so a secret containing a
//   bare `,` is masked only up to it. Cost: partial leak of such a secret after the comma.

/// Returns the redacted line, whether a block continuation follows, and the quote char of a
/// shell-quoted value that is still open at end of line (D-style: the next line is masked up to
/// that quote instead of wholesale, so text after the closing quote survives).
fn replace_sensitive_values(line: &str) -> (String, bool, Option<u8>) {
    let bytes = line.as_bytes();
    let mut out = String::with_capacity(line.len());
    let mut cursor = 0;
    let mut i = 0;
    let mut continuation = false;
    let mut open_quote = None;
    while i < bytes.len() {
        let quoted = (bytes[i] == b'"' || bytes[i] == b'\'')
            && i + 1 < bytes.len()
            && is_key_char(bytes[i + 1]);
        let key_start = if quoted { i + 1 } else { i };
        if !is_key_char(bytes[key_start]) || (!quoted && i > 0 && is_word_char(bytes[i - 1])) {
            i += 1;
            continue;
        }
        let cli = !quoted && bytes[i..].starts_with(b"--");
        let key_start = if cli { i + 2 } else { key_start };
        if key_start >= bytes.len() || !is_key_char(bytes[key_start]) {
            i += 1;
            continue;
        }
        let mut end = key_start;
        while end < bytes.len() && is_key_char(bytes[end]) {
            end += 1;
        }
        if !sensitive_key(&line[key_start..end]) {
            i = end;
            continue;
        }
        let key = &line[key_start..end];
        let header = key.to_ascii_lowercase().ends_with("authorization")
            || key.eq_ignore_ascii_case("cookie");
        let mut sep = end;
        // Shell form `-H 'Authorization: x'`: the quote opens the whole header, so no quote
        // follows the key. The value then ends at that same quote, not at end of line.
        let mut shell_quote = None;
        if quoted {
            if sep < bytes.len() && bytes[sep] == bytes[i] {
                sep += 1;
            } else {
                shell_quote = Some(bytes[i]);
            }
        }
        while sep < bytes.len() && bytes[sep].is_ascii_whitespace() {
            sep += 1;
        }
        if sep < bytes.len() && matches!(bytes[sep], b':' | b'=') {
            sep += 1;
        } else if !cli || sep == end {
            i = end;
            continue;
        }
        while sep < bytes.len() && bytes[sep].is_ascii_whitespace() {
            sep += 1;
        }
        if sep == bytes.len()
            || matches!(bytes[sep], b'|' | b'>') && line[sep + 1..].trim().is_empty()
        {
            if shell_quote.is_some() {
                open_quote = shell_quote;
            } else {
                continuation = true;
            }
            i = sep.max(end);
            continue;
        }
        if let Some(quote) = shell_quote {
            let found = quoted_end(bytes, sep, quote);
            // Non-header keys only take the split form (`-H "X-Auth-Token:" "S"`); a closed
            // `'x-token: v'` value still goes through the generic path below.
            if header || found == Some(sep) {
                let close = found.unwrap_or_else(|| unclosed_value_end(bytes, sep));
                // Cross-line carry only for a bare scheme at end of line (`'Authorization: Basic`
                // NEWLINE `SECRET' x`); anything else with an open quote is unrelated text.
                if found.is_none()
                    && close == bytes.len()
                    && AUTH_SCHEMES
                        .iter()
                        .any(|scheme| line[sep..close].trim().eq_ignore_ascii_case(scheme))
                {
                    open_quote = Some(quote);
                }
                if close > sep && &line[sep..close] != MASK {
                    out.push_str(&line[cursor..sep]);
                    out.push_str(MASK);
                    cursor = close;
                } else if close == sep {
                    // Split forms `-H "Authorization:" "Basic S"` / `-H 'Authorization:'"Basic S"`:
                    // the quote closes right after the key, so the credential is the next quoted
                    // token. Covers every sensitive key, not only headers.
                    let mut next = close + 1;
                    while next < bytes.len() && bytes[next].is_ascii_whitespace() {
                        next += 1;
                    }
                    if next < bytes.len() && matches!(bytes[next], b'"' | b'\'') {
                        let value = next + 1;
                        let value_end =
                            quoted_end(bytes, value, bytes[next]).unwrap_or(bytes.len());
                        if value_end > value && &line[value..value_end] != MASK {
                            out.push_str(&line[cursor..value]);
                            out.push_str(MASK);
                            cursor = value_end;
                        }
                        i = value_end.max(i + 1);
                        continue;
                    }
                }
                i = close.max(i + 1);
                continue;
            }
        }
        if header && !quoted {
            if key.eq_ignore_ascii_case("authorization")
                && AUTH_SCHEMES
                    .iter()
                    .any(|scheme| line[sep..].trim().eq_ignore_ascii_case(scheme))
            {
                continuation = true;
            }
            out.push_str(&line[cursor..sep]);
            out.push_str(MASK);
            cursor = bytes.len();
            break;
        }
        let (value_start, value_end, next) = if matches!(bytes[sep], b'"' | b'\'') {
            let quote = bytes[sep];
            match quoted_end(bytes, sep + 1, quote) {
                Some(close) => (sep + 1, close, close + 1),
                None => (sep + 1, bytes.len(), bytes.len()),
            }
        } else {
            let mut end = sep;
            while end < bytes.len()
                && !bytes[end].is_ascii_whitespace()
                && !matches!(bytes[end], b',' | b';' | b'&' | b'}' | b'\'' | b'"')
            {
                end += 1;
            }
            (sep, end, end)
        };
        if value_end > value_start && &line[value_start..value_end] != MASK {
            out.push_str(&line[cursor..value_start]);
            out.push_str(MASK);
            cursor = value_end;
        }
        i = next.max(i + 1);
    }
    out.push_str(&line[cursor..]);
    (out, continuation, open_quote)
}

fn replace_bearer(line: &str) -> (String, bool) {
    let lower = line.to_ascii_lowercase();
    let bytes = line.as_bytes();
    let mut out = String::with_capacity(line.len());
    let mut cursor = 0;
    let mut copied = 0;
    let mut continuation = false;
    while cursor < line.len() {
        let Some(offset) = lower[cursor..].find("bearer") else {
            break;
        };
        let start = cursor + offset;
        let end = start + 6;
        if (start > 0 && is_word_char(bytes[start - 1]))
            || end == line.len()
            || !bytes[end].is_ascii_whitespace()
        {
            cursor = end;
            continue;
        }
        let mut value = end;
        while value < line.len() && bytes[value].is_ascii_whitespace() {
            value += 1;
        }
        if value == line.len() {
            continuation = true;
            break;
        }
        let mut stop = value;
        while stop < line.len()
            && !bytes[stop].is_ascii_whitespace()
            && !matches!(bytes[stop], b'"' | b'\'' | b',' | b';')
        {
            stop += 1;
        }
        if &line[value..stop] != MASK {
            out.push_str(&line[copied..value]);
            out.push_str(MASK);
            copied = stop;
        }
        cursor = stop.max(end);
    }
    out.push_str(&line[copied..]);
    (out, continuation)
}

fn replace_url_userinfo(line: &str) -> String {
    let mut out = line.to_string();
    let mut cursor = 0;
    while cursor < out.len() {
        let Some(offset) = out[cursor..].find("://") else {
            break;
        };
        let marker = cursor + offset;
        let scheme_start = out[..marker]
            .rfind(|c: char| !c.is_ascii_alphanumeric() && c != '+' && c != '-' && c != '.')
            .map_or(0, |i| i + 1);
        if scheme_start == marker || !out.as_bytes()[scheme_start].is_ascii_alphabetic() {
            cursor = marker + 3;
            continue;
        }
        let start = marker + 3;
        let authority = &out[start..];
        let end = authority
            .find(|c: char| c.is_whitespace() || matches!(c, '/' | '?' | '#' | '"' | '\''))
            .unwrap_or(authority.len());
        if let Some(at) = authority[..end].rfind('@') {
            out.replace_range(start..start + at, MASK);
            cursor = start + MASK.len() + 1;
            continue;
        }
        cursor = start + end;
    }
    out
}

fn standalone_token(token: &str) -> bool {
    let lower = token.to_ascii_lowercase();
    let prefixed = [
        "github_pat_",
        "ghp_",
        "gho_",
        "ghs_",
        "hf_",
        "xoxb-",
        "xoxp-",
        "sk-",
        "glpat-",
        "sk_live_",
        "npm_",
        "aiza",
    ];
    if prefixed
        .iter()
        .any(|prefix| lower.starts_with(prefix) && token.len() >= prefix.len() + 8)
    {
        return true;
    }
    if token.len() == 20
        && (token.starts_with("AKIA") || token.starts_with("ASIA"))
        && token[4..]
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
    {
        return true;
    }
    let parts: Vec<_> = token.split('.').collect();
    parts.len() == 3
        && parts[0].starts_with("eyJ")
        && parts.iter().all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'='))
        })
}

fn replace_standalone_tokens(line: &str) -> String {
    let bytes = line.as_bytes();
    let mut out = String::new();
    let mut cursor = 0;
    let mut i = 0;
    while i < bytes.len() {
        if !is_word_char(bytes[i]) || i > 0 && is_word_char(bytes[i - 1]) {
            i += 1;
            continue;
        }
        let start = i;
        while i < bytes.len() && (is_word_char(bytes[i]) || bytes[i] == b'.') {
            i += 1;
        }
        if standalone_token(&line[start..i]) {
            out.push_str(&line[cursor..start]);
            out.push_str(MASK);
            cursor = i;
        }
    }
    out.push_str(&line[cursor..]);
    out
}

fn redact_pem(text: &str) -> String {
    let mut out = String::new();
    let mut cursor = 0;
    while let Some(offset) = text[cursor..].find("-----BEGIN ") {
        let start = cursor + offset;
        out.push_str(&text[cursor..start]);
        let Some(end_offset) = text[start..].find("-----END ") else {
            out.push_str(MASK);
            return out;
        };
        let end_start = start + end_offset;
        let end = text[end_start + 9..]
            .find("-----")
            .map_or(text.len(), |i| end_start + 9 + i + 5);
        out.push_str(MASK);
        cursor = end;
    }
    out.push_str(&text[cursor..]);
    out
}

fn replace_home(text: &str, home: &Path) -> String {
    let home = home.to_string_lossy();
    if home.is_empty() || home == "/" {
        return text.to_string();
    }
    let mut out = text.to_string();
    for needle in [home.to_string(), home.replace('/', "\\/")] {
        let lower = out.to_ascii_lowercase();
        let needle_lower = needle.to_ascii_lowercase();
        let mut replaced = String::with_capacity(out.len());
        let mut cursor = 0;
        let mut copied = 0;
        while cursor < out.len() {
            let Some(offset) = lower[cursor..].find(&needle_lower) else {
                break;
            };
            let start = cursor + offset;
            let end = start + needle.len();
            let boundary =
                end == out.len() || out.as_bytes()[end] == b'/' || out[end..].starts_with("\\/");
            if boundary {
                replaced.push_str(&out[copied..start]);
                replaced.push('~');
                copied = end;
            }
            cursor = end;
        }
        replaced.push_str(&out[copied..]);
        out = replaced;
    }
    out
}

pub fn redact_text(text: &str, home: Option<&Path>) -> String {
    let text = redact_pem(text);
    let mut result = String::with_capacity(text.len());
    let mut block_indent = None;
    let mut open_quote: Option<u8> = None;
    for line in text.split_inclusive('\n') {
        let bare = line.strip_suffix('\n').unwrap_or(line);
        // Taken before any `continue` so oversized and blank lines also end the carry.
        let carry = open_quote.take();
        if bare.len() > MAX_LINE_BYTES {
            result.push_str("[REDACTED:oversized-line]");
            if line.ends_with('\n') {
                result.push('\n');
            }
            continue;
        }
        // A shell-quoted header value left open by the previous line: mask up to the closing
        // quote. Lasts one line only, so a stray apostrophe cannot swallow the rest of the file.
        let carried;
        let bare = match carry.filter(|_| !bare.trim().is_empty()) {
            Some(quote) => match quoted_end(bare.as_bytes(), 0, quote) {
                Some(close) => {
                    carried = format!("{MASK}{}", &bare[close..]);
                    carried.as_str()
                }
                None => {
                    result.push_str(MASK);
                    if line.ends_with('\n') {
                        result.push('\n');
                    }
                    continue;
                }
            },
            None => bare,
        };
        let trimmed = bare.trim_start();
        let indent = bare.len() - trimmed.len();
        if let Some(parent_indent) = block_indent {
            if trimmed.is_empty() || indent > parent_indent {
                result.push_str(MASK);
                if line.ends_with('\n') {
                    result.push('\n');
                }
                continue;
            }
            block_indent = None;
        }
        let json = if trimmed.starts_with(['{', '[']) {
            serde_json::from_str::<serde_json::Value>(bare)
                .ok()
                .map(|mut value| {
                    redact_json(&mut value);
                    value.to_string()
                })
        } else {
            None
        };
        let source = json.as_deref().unwrap_or(bare);
        let (redacted, key_continuation, pending_quote) = replace_sensitive_values(source);
        open_quote = pending_quote;
        let (redacted, bearer_continuation) = replace_bearer(&redacted);
        if key_continuation || bearer_continuation {
            block_indent = Some(indent);
        }
        result.push_str(&replace_standalone_tokens(&replace_url_userinfo(&redacted)));
        if line.ends_with('\n') {
            result.push('\n');
        }
    }
    match home {
        Some(home) => replace_home(&result, home),
        None => result,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn test_new_secret_shapes() {
        let cases = [
            r#"{"auth":"SECRETMIX"}"#,
            "tls.key: SECRETMIX",
            r#"{"tls.key":"SECRETMIX"}"#,
            ".dockerconfigjson: SECRETMIX",
            "passphrase: SECRETMIX",
            "DB_PASS=SECRETMIX",
            "export PASS=SECRETMIX",
            "session=SECRETMIX",
            "signing_key: SECRETMIX",
            "encryption_key=SECRETMIX",
            r#"["--token","SECRETMIX"]"#,
            r#"["--token=SECRETMIX"]"#,
            r#"["--password","SECRETMIX"]"#,
            r#"{"credentials":{"pass":"SECRETMIX"}}"#,
            r#"{"secret":["SECRETMIX"]}"#,
            "id=eyJabc.SECRETMIX.sig",
            "Proxy-Authorization: Basic SECRETMIX",
            "ASIASECRETMIX1234567",
            "sk_live_SECRETMIX123456",
            "AIzaSECRETMIX123456",
            "npm_SECRETMIX123456",
            "X-Amz-Signature=SECRETMIX",
            "https://SECRETMIX@host",
        ];
        for input in cases {
            let output = redact_text(input, None);
            assert!(
                !output.contains("SECRETMIX"),
                "leaked from {input}: {output}"
            );
            if input.starts_with(['{', '[']) {
                serde_json::from_str::<serde_json::Value>(&output).unwrap();
            }
        }
        for input in [
            r#"{"token":null}"#,
            r#"{"password":123}"#,
            r#"{"auth":true}"#,
        ] {
            serde_json::from_str::<serde_json::Value>(&redact_text(input, None)).unwrap();
        }
    }

    #[test]
    fn test_oversized_line_cost_is_bounded() {
        for input in [
            "bearer x ".repeat(240_000),
            "/Users/developer/models ".repeat(60_000),
        ] {
            let start = Instant::now();
            let output = redact_text(&input, Some(Path::new("/Users/developer")));
            assert_eq!(output, "[REDACTED:oversized-line]");
            assert!(start.elapsed().as_secs() < 5);
        }
    }

    #[test]
    fn test_redact_bearer_token() {
        let input = "2026-09-29 12:00:00 Authorization: Bearer eyJhbGciOiJIUzI1NiJ9.secret-token-123\ncurl -H 'Bearer ghp_1234567890abcdef' http://127.0.0.1:8080";
        let redacted = redact_text(input, None);
        assert!(!redacted.contains("eyJhbGciOiJIUzI1NiJ9.secret-token-123"));
        assert!(!redacted.contains("ghp_1234567890abcdef"));
        assert!(redacted.contains("Bearer [REDACTED]"));
    }

    #[test]
    fn test_redaction_regression_inputs() {
        let cases = [
            "Authorization: BEARER SECRETMIX",
            "AUTHORIZATION: BEARER SECRETMIX",
            "BEARER SECRETMIX",
            "Authorization: Token SECRETMIX",
            "DB_PASSWORD=SECRETMIX",
            "HF_TOKEN=SECRETMIX",
            "MLFLOW_TRACKING_PASSWORD=SECRETMIX",
            "SECRET_ACCESS_KEY=SECRETMIX",
            "--password=SECRETMIX",
            "--token SECRETMIX",
            "{\"Password\":\"SECRETMIX\"}",
            "{\"TOKEN\":\"SECRETMIX\"}",
            "{\"X-Api-Key\":\"SECRETMIX\"}",
            "secretKey: SECRETMIX",
            "accessTokenValue: SECRETMIX",
            "{'password': 'SECRETMIX'}",
            "postgresql://mlflow:SECRETMIX@db",
            "HTTPS://u:SECRETMIX@h",
            "password = SECRETMIX",
            "aws_secret_access_key = SECRETMIX",
            "password: \"SECRETMIX with spaces\"",
            "password: |\n  SECRETMIX",
            "token:\n  SECRETMIX",
            "client-key-data:\n  SECRETMIX",
            "Authorization: Bearer\n  SECRETMIX",
            "eyJhbGciOiJIUzI1NiJ9.SECRETMIX.signature",
            "ghs_SECRETMIX123456",
            "xoxb-SECRETMIX123456",
            "Cookie: session=SECRETMIX",
            r#"{"pem":"-----BEGIN PRIVATE KEY-----\nSECRETMIX\n-----END PRIVATE KEY-----"}"#,
        ];
        for input in cases {
            let output = redact_text(input, None);
            assert!(
                !output.contains("SECRETMIX"),
                "leaked from {input}: {output}"
            );
            if input.starts_with('{') && input.contains('"') {
                serde_json::from_str::<serde_json::Value>(&output).unwrap();
            }
        }
        let benign = "task-queue-123 tokenizer: bert /Users/developer";
        assert_eq!(redact_text(benign, Some(Path::new("/Users/dev"))), benign);
        assert_eq!(
            redact_text("/users/DEV/models", Some(Path::new("/Users/dev"))),
            "~/models"
        );
    }

    #[test]
    fn test_redact_kubeconfig_key_data() {
        let yaml_input = r#"apiVersion: v1
clusters:
- cluster:
    certificate-authority-data: LS0tLS1CRUdJTiBDRVJUSUZJQ0FURS0tLS0tCg==
  name: colima
users:
- name: colima
  user:
    client-certificate-data: LS0tLS1CRUdJTiBDRVJUSUZJQ0FURS0tLS0tCg==
    client-key-data: LS0tLS1CRUdJTiBSU0EgUFJJVkFURSBLRVktLS0tLQo=
"#;
        let redacted_yaml = redact_text(yaml_input, None);
        assert!(!redacted_yaml.contains("LS0tLS1CRUdJTiBDRVJUSUZJQ0FURS0tLS0tCg=="));
        assert!(!redacted_yaml.contains("LS0tLS1CRUdJTiBSU0EgUFJJVkFURSBLRVktLS0tLQo="));
        assert!(redacted_yaml.contains("certificate-authority-data: [REDACTED]"));
        assert!(redacted_yaml.contains("client-certificate-data: [REDACTED]"));
        assert!(redacted_yaml.contains("client-key-data: [REDACTED]"));

        let json_input = r#"{"client-key-data": "LS0tLS1CRUdJTiBSU0EgUFJJVkFURSBLRVktLS0tLQo="}"#;
        let redacted_json = redact_text(json_input, None);
        assert!(!redacted_json.contains("LS0tLS1CRUdJTiBSU0EgUFJJVkFURSBLRVktLS0tLQo="));
        let value: serde_json::Value = serde_json::from_str(&redacted_json).unwrap();
        assert_eq!(value["client-key-data"], MASK);
    }

    #[test]
    fn test_redact_home_path() {
        let home = Path::new("/Users/developer");
        let input = "Model loaded from /Users/developer/.kubemetal/models/qwen2.5\nEscaped: \\/Users\\/developer\\/adapters";
        let redacted = redact_text(input, Some(home));
        assert!(!redacted.contains("/Users/developer"));
        assert!(redacted.contains("~/.kubemetal/models/qwen2.5"));
        assert!(redacted.contains("~\\/adapters"));
    }

    #[test]
    fn test_redact_api_keys_and_tokens() {
        let input = r#"
api_key: sk-1234567890abcdef12345
export HF_TOKEN=hf_abcdef1234567890
{"password": "super-secret-pass", "normal_field": "keep-me"}
"#;
        let redacted = redact_text(input, None);
        assert!(!redacted.contains("sk-1234567890abcdef12345"));
        assert!(!redacted.contains("hf_abcdef1234567890"));
        assert!(!redacted.contains("super-secret-pass"));
        assert!(redacted.contains("keep-me"));
        assert!(redacted.contains("[REDACTED]"));
    }

    #[test]
    fn test_redact_multiline_yaml_pem_and_url_userinfo() {
        let input = "client-key-data: |\n  first-secret-line\n  second-secret-line\nname: visible\n-----BEGIN PRIVATE KEY-----\npem-secret-line\n-----END PRIVATE KEY-----\nurl=https://user:password@host/path";
        let redacted = redact_text(input, None);
        for secret in [
            "first-secret-line",
            "second-secret-line",
            "pem-secret-line",
            "user:password",
        ] {
            assert!(!redacted.contains(secret), "leaked {secret}");
        }
        assert!(redacted.contains("name: visible"));
        assert!(redacted.contains("host/path"));
    }

    #[test]
    fn test_redact_multiple_env_secrets_on_one_line() {
        let input = "AWS_SECRET_ACCESS_KEY=alpha123 AWS_ACCESS_KEY_ID=AKIA1234567890123456 GITHUB_TOKEN=github_pat_1234567890 ghp_1234567890 gho_1234567890 hf_1234567890 password=first password=second";
        let redacted = redact_text(input, None);
        for secret in [
            "alpha123",
            "AKIA1234567890123456",
            "github_pat_1234567890",
            "ghp_1234567890",
            "gho_1234567890",
            "hf_1234567890",
            "first",
            "second",
        ] {
            assert!(!redacted.contains(secret), "leaked {secret}");
        }
    }

    #[test]
    fn test_redacted_json_remains_valid() {
        let redacted = redact_text(
            r#"{"password":"secret","password":"second","normal":"keep"}"#,
            None,
        );
        let value: serde_json::Value = serde_json::from_str(&redacted).unwrap();
        assert_eq!(value["normal"], "keep");
        assert!(!redacted.contains("secret"));
        assert!(!redacted.contains("second"));
    }

    #[test]
    fn test_redact_shell_quoted_headers() {
        let single = redact_text(
            "curl -H 'Authorization: Basic dXNlcjpwYXNz' https://host/x",
            None,
        );
        assert_eq!(single, "curl -H 'Authorization: [REDACTED]' https://host/x");
        let double = redact_text(
            r#"curl -H "Authorization: Bearer abc.def" https://host/x"#,
            None,
        );
        assert_eq!(
            double,
            r#"curl -H "Authorization: [REDACTED]" https://host/x"#
        );
        let more = redact_text(
            "kubectl run x -H 'Proxy-Authorization: Basic c2VjcmV0' --flag=1 --other",
            None,
        );
        assert_eq!(
            more,
            "kubectl run x -H 'Proxy-Authorization: [REDACTED]' --flag=1 --other"
        );
        let cookie = redact_text("curl -H 'Cookie: sid=abc; t=def' https://host", None);
        assert_eq!(cookie, "curl -H 'Cookie: [REDACTED]' https://host");
    }

    #[test]
    fn test_redact_two_shell_quoted_headers_on_one_line() {
        let out = redact_text(
            "curl -H 'Authorization: Basic AAA111' -H 'X-Api-Key: BBB222' https://host",
            None,
        );
        assert_eq!(
            out,
            "curl -H 'Authorization: [REDACTED]' -H 'X-Api-Key: [REDACTED]' https://host"
        );
    }

    #[test]
    fn test_unquoted_header_still_redacted() {
        let out = redact_text("Authorization: Basic dXNlcjpwYXNz", None);
        assert_eq!(out, "Authorization: [REDACTED]");
    }

    #[test]
    fn test_shell_quoted_non_sensitive_key_untouched() {
        let input = "curl -H 'Content-Type: application/json' -H 'Accept: text/plain' https://host";
        assert_eq!(redact_text(input, None), input);
    }

    #[test]
    fn test_shell_quoted_header_continues_onto_next_line() {
        for (input, want) in [
            (
                "curl -H 'Authorization: Basic\n  SECRET' https://x",
                "curl -H 'Authorization: [REDACTED]\n[REDACTED]' https://x",
            ),
            (
                "curl -H \"Authorization: Bearer\n  SECRET\" -d x",
                "curl -H \"Authorization: [REDACTED]\n[REDACTED]\" -d x",
            ),
            (
                "curl -H 'Authorization:\nSECRET' https://x\nnext line",
                "curl -H 'Authorization:\n[REDACTED]' https://x\nnext line",
            ),
        ] {
            let out = redact_text(input, None);
            assert!(!out.contains("SECRET"), "leaked: {out}");
            assert_eq!(out, want);
        }
    }

    #[test]
    fn test_shell_quoted_continuation_is_bounded_to_one_line() {
        let out = redact_text("curl -H 'Authorization: Basic\nno close\nkeep me", None);
        assert_eq!(
            out,
            "curl -H 'Authorization: [REDACTED]\n[REDACTED]\nkeep me"
        );
    }

    #[test]
    fn test_split_quote_header_forms() {
        assert_eq!(
            redact_text(r#"curl -H "Authorization:" "Basic SECRET" https://x"#, None),
            r#"curl -H "Authorization:" "[REDACTED]" https://x"#
        );
        assert_eq!(
            redact_text(r#"curl -H 'Authorization:'"Basic SECRET" https://x"#, None),
            r#"curl -H 'Authorization:'"[REDACTED]" https://x"#
        );
        let benign = r#"curl -H "Accept:" "application/json" https://x"#;
        assert_eq!(redact_text(benign, None), benign);
    }

    #[test]
    fn test_shell_quoted_env_assignment_is_redacted() {
        assert_eq!(
            redact_text("docker run -e 'PASSWORD=S more' img", None),
            "docker run -e 'PASSWORD=[REDACTED] more' img"
        );
    }

    /// Accepted over-redaction (D-style: a short real credential after `key: ` is
    /// indistinguishable from prose, and narrowing would reopen the shell-quote leak; cost is a
    /// masked word in a log line).
    #[test]
    fn test_shell_quoted_prose_over_redaction_is_accepted() {
        assert_eq!(
            redact_text("\"session: started ok\"", None),
            "\"session: [REDACTED] ok\""
        );
        assert_eq!(
            redact_text("\"token: 5 left\"", None),
            "\"token: [REDACTED] left\""
        );
    }

    #[test]
    fn test_unclosed_shell_quote_keeps_json_structure() {
        let compact = r#"{"cmd":"curl -H 'Authorization: Basic SECRET","n":1}"#;
        let out = redact_text(compact, None);
        assert!(!out.contains("SECRET"), "leaked: {out}");
        let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed["n"], 1);
        let pretty = "{\n  \"cmd\": \"curl -H 'Authorization: Basic SECRET\",\n  \"other\": 1\n}\n";
        let out = redact_text(pretty, None);
        assert!(!out.contains("SECRET"), "leaked: {out}");
        assert!(out.contains("\"other\": 1"), "structure eaten: {out}");
        serde_json::from_str::<serde_json::Value>(&out).unwrap();
        // Non-JSON: still masked to end of line.
        assert_eq!(
            redact_text("curl -H 'Authorization: Basic SECRET more", None),
            "curl -H 'Authorization: [REDACTED]"
        );
    }

    #[test]
    fn test_carry_only_after_bare_scheme_and_lasts_one_line() {
        assert_eq!(
            redact_text("curl -H 'Authorization: Basic\n  SECRET' x", None),
            "curl -H 'Authorization: [REDACTED]\n[REDACTED]' x"
        );
        let unrelated = "event 'Authorization: pending\nkubelet ready pods=3 status=ok\n";
        assert_eq!(
            redact_text(unrelated, None),
            "event 'Authorization: [REDACTED]\nkubelet ready pods=3 status=ok\n"
        );
        // Oversized line consumes the carry; the line after it is untouched.
        let big = "x".repeat(MAX_LINE_BYTES + 1);
        let out = redact_text(
            &format!("curl -H 'Authorization: Basic\n{big}\nkeep me\n"),
            None,
        );
        assert!(
            out.ends_with("[REDACTED:oversized-line]\nkeep me\n"),
            "{out}"
        );
        // Accepted limit: a blank line ends the carry (the blank line itself survives).
        assert_eq!(
            redact_text("curl -H 'Authorization: Basic\n\nkeep me\n", None),
            "curl -H 'Authorization: [REDACTED]\n\nkeep me\n"
        );
    }

    #[test]
    fn test_split_quote_form_covers_non_header_keys() {
        assert_eq!(
            redact_text(r#"curl -H "X-Auth-Token:" "SECRET" https://x"#, None),
            r#"curl -H "X-Auth-Token:" "[REDACTED]" https://x"#
        );
        let benign = r#"curl -H "Accept:" "application/json" https://x"#;
        assert_eq!(redact_text(benign, None), benign);
    }
}
