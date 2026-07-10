//! Credential detection and format-preserving redaction at durable evidence boundaries.

pub fn contains_credential_material(value: &str) -> bool {
    !credential_spans(value).is_empty()
}

pub fn redact_sensitive_text(value: &str) -> String {
    let spans = credential_spans(value);
    if spans.is_empty() {
        return value.to_string();
    }
    let mut output = String::with_capacity(value.len());
    let mut cursor = 0;
    for (start, end) in spans {
        if start < cursor {
            continue;
        }
        output.push_str(&value[cursor..start]);
        output.push_str("[REDACTED]");
        cursor = end;
    }
    output.push_str(&value[cursor..]);
    output
}

fn credential_spans(value: &str) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    collect_prefixed_tokens(value, &mut spans);
    collect_jwts(value, &mut spans);
    collect_bearer_tokens(value, &mut spans);
    collect_assignments(value, &mut spans);
    spans.sort_unstable();
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (start, end) in spans {
        if start >= end || !value.is_char_boundary(start) || !value.is_char_boundary(end) {
            continue;
        }
        if let Some(last) = merged.last_mut() {
            if start <= last.1 {
                last.1 = last.1.max(end);
                continue;
            }
        }
        merged.push((start, end));
    }
    merged
}

fn collect_prefixed_tokens(value: &str, spans: &mut Vec<(usize, usize)>) {
    const PREFIXES: [&[u8]; 8] = [
        b"sk-",
        b"ghp_",
        b"github_pat_",
        b"xoxb-",
        b"xoxp-",
        b"hf_",
        b"akia",
        b"aiza",
    ];
    let bytes = value.as_bytes();
    for start in 0..bytes.len() {
        if start > 0 && (bytes[start - 1].is_ascii_alphanumeric() || bytes[start - 1] == b'_') {
            continue;
        }
        let Some(prefix) = PREFIXES
            .iter()
            .find(|prefix| starts_with_ignore_ascii_case(bytes, start, prefix))
        else {
            continue;
        };
        let mut end = start + prefix.len();
        while end < bytes.len() && credential_char(bytes[end]) {
            end += 1;
        }
        if end - start > 12 {
            spans.push((start, end));
        }
    }
}

fn collect_jwts(value: &str, spans: &mut Vec<(usize, usize)>) {
    let bytes = value.as_bytes();
    let mut start = 0;
    while start < bytes.len() {
        if !jwt_char(bytes[start]) {
            start += 1;
            continue;
        }
        let mut end = start;
        while end < bytes.len() && jwt_char(bytes[end]) {
            end += 1;
        }
        let token = &bytes[start..end];
        let segments = token.split(|byte| *byte == b'.').collect::<Vec<_>>();
        if segments.len() == 3
            && token.len() >= 24
            && segments[0].starts_with(b"eyJ")
            && segments
                .iter()
                .all(|segment| segment.len() >= 6 && segment.iter().all(|byte| base64url(*byte)))
        {
            spans.push((start, end));
        }
        start = end;
    }
}

fn collect_bearer_tokens(value: &str, spans: &mut Vec<(usize, usize)>) {
    let bytes = value.as_bytes();
    let needle = b"bearer";
    for start in 0..bytes.len() {
        if !starts_with_ignore_ascii_case(bytes, start, needle)
            || (start > 0 && bytes[start - 1].is_ascii_alphanumeric())
        {
            continue;
        }
        let mut token_start = start + needle.len();
        if token_start < bytes.len() && bytes[token_start].is_ascii_alphanumeric() {
            continue;
        }
        while token_start < bytes.len() && bytes[token_start].is_ascii_whitespace() {
            token_start += 1;
        }
        let mut end = token_start;
        while end < bytes.len() && credential_char(bytes[end]) {
            end += 1;
        }
        if end.saturating_sub(token_start) >= 5 {
            spans.push((token_start, end));
        }
    }
}

fn collect_assignments(value: &str, spans: &mut Vec<(usize, usize)>) {
    let bytes = value.as_bytes();
    for separator in 0..bytes.len() {
        if !matches!(bytes[separator], b'=' | b':') {
            continue;
        }
        let label_end = skip_backward_label_spacing(value, separator);
        let mut label_start = label_end;
        while label_start > 0 && label_char(bytes[label_start - 1]) {
            label_start -= 1;
        }
        if label_start == label_end {
            continue;
        }
        let label = value[label_start..label_end].to_ascii_lowercase();
        if !sensitive_label(&label) {
            continue;
        }
        let mut start = skip_forward_whitespace(value, separator + 1);
        let quote = bytes
            .get(start)
            .copied()
            .filter(|byte| matches!(byte, b'"' | b'\'' | b'`'));
        if quote.is_some() {
            start += 1;
        }
        if starts_with_ignore_ascii_case(bytes, start, b"bearer") {
            start += b"bearer".len();
            start = skip_forward_whitespace(value, start);
        }
        let mut end = start;
        if let Some(quote) = quote {
            while end < bytes.len() {
                if bytes[end] == quote && (end == start || bytes[end - 1] != b'\\') {
                    break;
                }
                end += 1;
            }
        } else {
            while end < bytes.len() && !assignment_delimiter(bytes[end]) {
                end += 1;
            }
        }
        if should_redact_assignment(&label, &value[start..end]) {
            spans.push((start, end));
        }
    }
}

fn should_redact_assignment(label: &str, candidate: &str) -> bool {
    let candidate = candidate.trim();
    if candidate.len() < 5
        || candidate.bytes().all(|byte| byte.is_ascii_digit())
        || matches!(
            candidate.to_ascii_lowercase().as_str(),
            "none" | "null" | "false" | "true" | "example" | "placeholder" | "redacted"
        )
        || candidate.starts_with("${")
        || candidate.starts_with("{{")
        || candidate.starts_with('[')
    {
        return false;
    }
    if label == "key" {
        return candidate.len() >= 16;
    }
    true
}

fn sensitive_label(label: &str) -> bool {
    matches!(
        label,
        "api_key"
            | "apikey"
            | "api-key"
            | "x-api-key"
            | "key"
            | "token"
            | "access_token"
            | "access-token"
            | "password"
            | "passwd"
            | "secret"
            | "client_secret"
            | "client-secret"
            | "authorization"
            | "auth"
    )
}

fn starts_with_ignore_ascii_case(bytes: &[u8], start: usize, needle: &[u8]) -> bool {
    bytes
        .get(start..start.saturating_add(needle.len()))
        .is_some_and(|slice| slice.eq_ignore_ascii_case(needle))
}

fn label_char(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-')
}

fn credential_char(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.')
}

fn base64url(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-')
}

fn jwt_char(byte: u8) -> bool {
    base64url(byte) || byte == b'.'
}

fn assignment_delimiter(byte: u8) -> bool {
    byte.is_ascii_whitespace()
        || matches!(
            byte,
            b'"' | b'\'' | b'`' | b',' | b';' | b'&' | b'}' | b']' | b')'
        )
}

fn skip_forward_whitespace(value: &str, mut index: usize) -> usize {
    while index < value.len() {
        let Some(ch) = value[index..].chars().next() else {
            break;
        };
        if !ch.is_whitespace() {
            break;
        }
        index += ch.len_utf8();
    }
    index
}

fn skip_backward_label_spacing(value: &str, mut index: usize) -> usize {
    while index > 0 {
        let Some(ch) = value[..index].chars().next_back() else {
            break;
        };
        if !ch.is_whitespace() && !matches!(ch, '"' | '\'' | '`') {
            break;
        }
        index -= ch.len_utf8();
    }
    index
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_assignments_bearer_queries_json_jwt_and_known_families() {
        let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.c2lnbmF0dXJl";
        let input = format!(
            "api_key=secret-value\ntoken = hunter2\n?token=hunter2&x=1\n\"password\" : \"secret-value\"\nAuthorization: Bearer sk-abcdefghijklmnopqrstuvwxyz\nghp_abcdefghijklmnopqrstuvwxyz\n{jwt}"
        );
        let output = redact_sensitive_text(&input);
        for secret in [
            "secret-value",
            "hunter2",
            "sk-abcdefghijklmnopqrstuvwxyz",
            "ghp_abcdefghijklmnopqrstuvwxyz",
            jwt,
        ] {
            assert!(!output.contains(secret));
        }
        assert_eq!(output.lines().count(), input.lines().count());
        assert!(contains_credential_material(&input));
    }

    #[test]
    fn preserves_non_secret_code_byte_for_byte() {
        let input = "fn main() {\n    let key=42;\n    let token_count=3;\n    let opcode = driver.transport.sent_packets[0].opcode;\n}\n";
        assert_eq!(redact_sensitive_text(input), input);
        assert!(!contains_credential_material(input));
    }

    #[test]
    fn preserves_layout_around_real_code_secret() {
        let input = "fn main() {\n    let api_key=\"sk-abcdefghijklmnopqrstuvwxyz\";\n}\n";
        let output = redact_sensitive_text(input);
        assert!(output.starts_with("fn main() {\n    let api_key=\""));
        assert!(output.ends_with("\";\n}\n"));
        assert!(!output.contains("sk-abcdefghijklmnopqrstuvwxyz"));
    }

    #[test]
    fn redacts_obfuscated_whitespace_credentials_in_quoted_assignments() {
        for input in [
            "let api_key = \"s k - a b c d e f g h i j k\";",
            "token = \"h\tu\tn\nt\ne\tr\t2\"",
            "\"password\" : \"p a s s w o r d value\"",
            "api_key = \"\u{2003}s k - spaced unicode\"",
            "api_key\u{2003}=\u{2003}\"spaced-secret-value\"",
        ] {
            let output = redact_sensitive_text(input);
            assert!(output.contains("[REDACTED]"), "{input}");
            assert!(contains_credential_material(input), "{input}");
        }
    }
}
