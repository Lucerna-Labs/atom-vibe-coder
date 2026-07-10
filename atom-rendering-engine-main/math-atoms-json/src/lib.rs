//! Dependency-free, fully consuming JSON parser for provider and ledger boundaries.

use std::fmt;

const MAX_DEPTH: usize = 64;

#[derive(Clone, Debug, PartialEq)]
pub enum JsonValue {
    Null,
    Bool(bool),
    Number(String),
    String(String),
    Array(Vec<JsonValue>),
    Object(Vec<(String, JsonValue)>),
}

impl JsonValue {
    pub fn as_object(&self) -> Option<&[(String, JsonValue)]> {
        match self {
            Self::Object(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[JsonValue]> {
        match self {
            Self::Array(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_u64(&self) -> Option<u64> {
        match self {
            Self::Number(value)
                if !value.starts_with('-')
                    && !value.contains('.')
                    && !value.contains(['e', 'E']) =>
            {
                value.parse().ok()
            }
            _ => None,
        }
    }

    pub fn get(&self, key: &str) -> Option<&JsonValue> {
        self.as_object()?
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value)
    }

    pub fn at_path<'a>(&'a self, path: &[&str]) -> Option<&'a JsonValue> {
        let mut value = self;
        for segment in path {
            value = value.get(segment)?;
        }
        Some(value)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JsonError {
    pub offset: usize,
    pub message: &'static str,
}

impl fmt::Display for JsonError {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            output,
            "JSON error at byte {}: {}",
            self.offset, self.message
        )
    }
}

impl std::error::Error for JsonError {}

pub fn parse(input: &str) -> Result<JsonValue, JsonError> {
    let mut parser = Parser { input, pos: 0 };
    parser.skip_whitespace();
    let value = parser.parse_value(0)?;
    parser.skip_whitespace();
    if parser.pos != input.len() {
        return Err(parser.error("trailing data after JSON value"));
    }
    Ok(value)
}

pub fn string_field(input: &str, key: &str) -> Option<String> {
    if key.is_empty() {
        return None;
    }
    find_string_field(&parse(input).ok()?, key)
}

fn find_string_field(value: &JsonValue, key: &str) -> Option<String> {
    match value {
        JsonValue::Object(fields) => {
            for (name, value) in fields {
                if name == key {
                    return value.as_str().map(str::to_string);
                }
            }
            fields
                .iter()
                .find_map(|(_, value)| find_string_field(value, key))
        }
        JsonValue::Array(values) => values
            .iter()
            .find_map(|value| find_string_field(value, key)),
        _ => None,
    }
}

struct Parser<'a> {
    input: &'a str,
    pos: usize,
}

impl Parser<'_> {
    fn parse_value(&mut self, depth: usize) -> Result<JsonValue, JsonError> {
        if depth > MAX_DEPTH {
            return Err(self.error("maximum JSON nesting depth exceeded"));
        }
        self.skip_whitespace();
        match self.peek_char() {
            Some('{') => self.parse_object(depth + 1),
            Some('[') => self.parse_array(depth + 1),
            Some('"') => self.parse_string().map(JsonValue::String),
            Some('t') => self.parse_literal("true", JsonValue::Bool(true)),
            Some('f') => self.parse_literal("false", JsonValue::Bool(false)),
            Some('n') => self.parse_literal("null", JsonValue::Null),
            Some('-' | '0'..='9') => self.parse_number().map(JsonValue::Number),
            Some(_) => Err(self.error("unexpected character")),
            None => Err(self.error("unexpected end of input")),
        }
    }

    fn parse_object(&mut self, depth: usize) -> Result<JsonValue, JsonError> {
        self.expect_char('{')?;
        self.skip_whitespace();
        let mut fields = Vec::new();
        if self.consume_char('}') {
            return Ok(JsonValue::Object(fields));
        }
        loop {
            self.skip_whitespace();
            if self.peek_char() != Some('"') {
                return Err(self.error("object key must be a string"));
            }
            let key = self.parse_string()?;
            if fields.iter().any(|(name, _)| name == &key) {
                return Err(self.error("duplicate object key"));
            }
            self.skip_whitespace();
            self.expect_char(':')?;
            let value = self.parse_value(depth)?;
            fields.push((key, value));
            self.skip_whitespace();
            if self.consume_char('}') {
                break;
            }
            self.expect_char(',')?;
        }
        Ok(JsonValue::Object(fields))
    }

    fn parse_array(&mut self, depth: usize) -> Result<JsonValue, JsonError> {
        self.expect_char('[')?;
        self.skip_whitespace();
        let mut values = Vec::new();
        if self.consume_char(']') {
            return Ok(JsonValue::Array(values));
        }
        loop {
            values.push(self.parse_value(depth)?);
            self.skip_whitespace();
            if self.consume_char(']') {
                break;
            }
            self.expect_char(',')?;
        }
        Ok(JsonValue::Array(values))
    }

    fn parse_string(&mut self) -> Result<String, JsonError> {
        self.expect_char('"')?;
        let mut output = String::new();
        loop {
            let Some(ch) = self.next_char() else {
                return Err(self.error("unterminated string"));
            };
            match ch {
                '"' => return Ok(output),
                '\\' => match self.next_char() {
                    Some('"') => output.push('"'),
                    Some('\\') => output.push('\\'),
                    Some('/') => output.push('/'),
                    Some('b') => output.push('\u{0008}'),
                    Some('f') => output.push('\u{000c}'),
                    Some('n') => output.push('\n'),
                    Some('r') => output.push('\r'),
                    Some('t') => output.push('\t'),
                    Some('u') => output.push(self.parse_unicode_scalar()?),
                    Some(_) => return Err(self.error("unknown string escape")),
                    None => return Err(self.error("unterminated string escape")),
                },
                ch if ch.is_control() => {
                    return Err(self.error("unescaped control character in string"))
                }
                _ => output.push(ch),
            }
        }
    }

    fn parse_unicode_scalar(&mut self) -> Result<char, JsonError> {
        let high = self.parse_hex_unit()?;
        let scalar = if (0xd800..=0xdbff).contains(&high) {
            if self.next_char() != Some('\\') || self.next_char() != Some('u') {
                return Err(self.error("high surrogate must be followed by low surrogate"));
            }
            let low = self.parse_hex_unit()?;
            if !(0xdc00..=0xdfff).contains(&low) {
                return Err(self.error("invalid low surrogate"));
            }
            0x1_0000 + ((u32::from(high) - 0xd800) << 10) + (u32::from(low) - 0xdc00)
        } else if (0xdc00..=0xdfff).contains(&high) {
            return Err(self.error("unpaired low surrogate"));
        } else {
            u32::from(high)
        };
        char::from_u32(scalar).ok_or_else(|| self.error("invalid Unicode scalar"))
    }

    fn parse_hex_unit(&mut self) -> Result<u16, JsonError> {
        let mut value = 0u16;
        for _ in 0..4 {
            let digit = self
                .next_char()
                .and_then(|ch| ch.to_digit(16))
                .ok_or_else(|| self.error("invalid Unicode escape"))?;
            value = value * 16 + digit as u16;
        }
        Ok(value)
    }

    fn parse_number(&mut self) -> Result<String, JsonError> {
        let start = self.pos;
        self.consume_char('-');
        match self.peek_char() {
            Some('0') => {
                self.next_char();
                if matches!(self.peek_char(), Some('0'..='9')) {
                    return Err(self.error("number has a leading zero"));
                }
            }
            Some('1'..='9') => self.consume_digits(),
            _ => return Err(self.error("invalid number")),
        }
        if self.consume_char('.') {
            if !matches!(self.peek_char(), Some('0'..='9')) {
                return Err(self.error("fraction requires a digit"));
            }
            self.consume_digits();
        }
        if matches!(self.peek_char(), Some('e' | 'E')) {
            self.next_char();
            if matches!(self.peek_char(), Some('+' | '-')) {
                self.next_char();
            }
            if !matches!(self.peek_char(), Some('0'..='9')) {
                return Err(self.error("exponent requires a digit"));
            }
            self.consume_digits();
        }
        Ok(self.input[start..self.pos].to_string())
    }

    fn consume_digits(&mut self) {
        while matches!(self.peek_char(), Some('0'..='9')) {
            self.next_char();
        }
    }

    fn parse_literal(
        &mut self,
        literal: &'static str,
        value: JsonValue,
    ) -> Result<JsonValue, JsonError> {
        if self.input[self.pos..].starts_with(literal) {
            self.pos += literal.len();
            Ok(value)
        } else {
            Err(self.error("invalid literal"))
        }
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.peek_char(), Some(' ' | '\t' | '\n' | '\r')) {
            self.next_char();
        }
    }

    fn expect_char(&mut self, expected: char) -> Result<(), JsonError> {
        if self.consume_char(expected) {
            Ok(())
        } else {
            Err(self.error("unexpected delimiter"))
        }
    }

    fn consume_char(&mut self, expected: char) -> bool {
        if self.peek_char() == Some(expected) {
            self.next_char();
            true
        } else {
            false
        }
    }

    fn peek_char(&self) -> Option<char> {
        self.input[self.pos..].chars().next()
    }

    fn next_char(&mut self) -> Option<char> {
        let ch = self.peek_char()?;
        self.pos += ch.len_utf8();
        Some(ch)
    }

    fn error(&self, message: &'static str) -> JsonError {
        JsonError {
            offset: self.pos,
            message,
        }
    }
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
            Some("ready \u{1f680}")
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
    fn fully_consumes_and_rejects_duplicate_keys() {
        assert!(parse(r#"{"content":"answer"} garbage"#).is_err());
        assert!(parse(r#"{"content":"first","content":"second"}"#).is_err());
        assert!(parse(r#"{"content":"\q"}"#).is_err());
    }

    #[test]
    fn parses_nested_values_and_strict_numbers() {
        let root = parse(r#"{"ok":true,"count":42,"items":[null,{"text":"done"}]}"#).unwrap();
        assert_eq!(root.get("count").and_then(JsonValue::as_u64), Some(42));
        assert_eq!(
            root.get("items")
                .and_then(JsonValue::as_array)
                .and_then(|items| items.get(1))
                .and_then(|item| item.get("text"))
                .and_then(JsonValue::as_str),
            Some("done")
        );
        for invalid in ["01", "1.", "1e", "--1", "+1"] {
            assert!(parse(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn rejects_excessive_nesting() {
        let input = format!(
            "{}0{}",
            "[".repeat(MAX_DEPTH + 2),
            "]".repeat(MAX_DEPTH + 2)
        );
        assert!(parse(&input).is_err());
    }
}
