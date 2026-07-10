//! Minimal strict JSON string-field decoding for provider response envelopes.

pub fn string_field(input: &str, key: &str) -> Option<String> {
    if key.is_empty()
        || key
            .chars()
            .any(|ch| matches!(ch, '"' | '\\') || ch.is_control())
    {
        return None;
    }
    let needle = format!("\"{key}\"");
    let mut cursor = 0;
    while let Some(offset) = input[cursor..].find(&needle) {
        let start = cursor + offset + needle.len();
        if let Some(text) = string_after_colon(&input[start..]) {
            return Some(text);
        }
        cursor = start;
    }
    None
}

fn string_after_colon(input: &str) -> Option<String> {
    let mut chars = input.chars().skip_while(|ch| ch.is_whitespace());
    if chars.next()? != ':' {
        return None;
    }
    if chars.by_ref().find(|ch| !ch.is_whitespace())? != '"' {
        return None;
    }
    let mut out = String::new();
    loop {
        let ch = chars.next()?;
        match ch {
            '"' => return Some(out),
            '\\' => match chars.next()? {
                '"' => out.push('"'),
                '\\' => out.push('\\'),
                '/' => out.push('/'),
                'b' => out.push('\u{0008}'),
                'f' => out.push('\u{000c}'),
                'n' => out.push('\n'),
                'r' => out.push('\r'),
                't' => out.push('\t'),
                'u' => out.push(unicode_scalar(&mut chars)?),
                _ => return None,
            },
            ch if ch.is_control() => return None,
            _ => out.push(ch),
        }
    }
}

fn unicode_scalar(chars: &mut impl Iterator<Item = char>) -> Option<char> {
    let high = hex_unit(chars)?;
    let scalar = if (0xd800..=0xdbff).contains(&high) {
        if chars.next()? != '\\' || chars.next()? != 'u' {
            return None;
        }
        let low = hex_unit(chars)?;
        if !(0xdc00..=0xdfff).contains(&low) {
            return None;
        }
        0x1_0000 + ((u32::from(high) - 0xd800) << 10) + (u32::from(low) - 0xdc00)
    } else if (0xdc00..=0xdfff).contains(&high) {
        return None;
    } else {
        u32::from(high)
    };
    char::from_u32(scalar)
}

fn hex_unit(chars: &mut impl Iterator<Item = char>) -> Option<u16> {
    let mut value = 0u16;
    for _ in 0..4 {
        value = value.checked_mul(16)?;
        value = value.checked_add(chars.next()?.to_digit(16)?.try_into().ok()?)?;
    }
    Some(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_provider_punctuation_escapes() {
        let body =
            r#"{"choices":[{"message":{"content":"Vec\u003c\u0026\u0027static str\u003e"}}]}"#;
        assert_eq!(
            string_field(body, "content").as_deref(),
            Some("Vec<&'static str>")
        );
    }

    #[test]
    fn decodes_surrogate_pairs() {
        assert_eq!(
            string_field(r#"{"content":"ready \uD83D\uDE80"}"#, "content").as_deref(),
            Some("ready 🚀")
        );
    }

    #[test]
    fn decodes_standard_escapes() {
        assert_eq!(
            string_field(r#"{"content":"line 1\n\"line 2\"\\done"}"#, "content").as_deref(),
            Some("line 1\n\"line 2\"\\done")
        );
    }

    #[test]
    fn rejects_malformed_or_unpaired_unicode() {
        for body in [
            r#"{"content":"\u12xz"}"#,
            r#"{"content":"\uD83D"}"#,
            r#"{"content":"\uDE80"}"#,
            r#"{"content":"\uD83D\u0041"}"#,
        ] {
            assert_eq!(string_field(body, "content"), None, "{body}");
        }
    }

    #[test]
    fn skips_non_field_key_occurrences() {
        let body = r#"{"note":"content","content":"answer"}"#;
        assert_eq!(string_field(body, "content").as_deref(), Some("answer"));
        assert_eq!(string_field(body, "bad\"key"), None);
    }
}
