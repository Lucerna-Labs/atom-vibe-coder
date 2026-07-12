use crate::{ProviderAdapterError, ProviderTurnReceipt, ThinkingEvidence, TokenUsage};
use math_atoms_core::{ProviderConfig, ProviderWireFormat};
use math_atoms_hash::sha256_tagged;
use math_atoms_json::{parse, JsonValue};

pub(crate) struct ReceiptInput<'a> {
    pub request_id: &'a str,
    pub request_body: &'a str,
    pub raw_response: &'a str,
    pub elapsed_ms: u128,
    pub output_limit: usize,
}

pub(crate) fn parse_receipt(
    config: &ProviderConfig,
    input: ReceiptInput<'_>,
) -> Result<ProviderTurnReceipt, ProviderAdapterError> {
    if input.raw_response.len() > math_atoms_provider_transport::MAX_PROVIDER_RESPONSE_BYTES {
        return Err(ProviderAdapterError::InvalidResponse(
            "response exceeded the transport byte limit".to_string(),
        ));
    }
    let root = parse(input.raw_response)
        .map_err(|error| ProviderAdapterError::InvalidResponse(error.to_string()))?;
    if !matches!(root.get("error"), None | Some(JsonValue::Null)) {
        return Err(ProviderAdapterError::InvalidResponse(
            "provider returned an error envelope".to_string(),
        ));
    }
    let usage = token_usage(&root, config.wire_format);
    let thinking = thinking_evidence(&root, config.wire_format, &usage)
        .ok_or(ProviderAdapterError::ThinkingEvidenceMissing)?;
    let text = response_text(&root, config)?;
    if text.trim().is_empty() {
        return Err(ProviderAdapterError::InvalidResponse(
            "provider returned empty text".to_string(),
        ));
    }
    if text.len() > input.output_limit {
        return Err(ProviderAdapterError::OutputTooLarge {
            actual: text.len(),
            limit: input.output_limit,
        });
    }
    Ok(ProviderTurnReceipt {
        request_id: input.request_id.to_string(),
        provider: config.kind.as_str().to_string(),
        model: config.model.clone(),
        request_body_hash: sha256_tagged(input.request_body.as_bytes()),
        raw_response_hash: sha256_tagged(input.raw_response.as_bytes()),
        output_hash: sha256_tagged(text.as_bytes()),
        elapsed_ms: input.elapsed_ms,
        text,
        usage,
        thinking,
    })
}

fn response_text(
    root: &JsonValue,
    config: &ProviderConfig,
) -> Result<String, ProviderAdapterError> {
    let preferred = config.response_key.trim();
    if !preferred.is_empty() && preferred != "output_text" {
        if let Some(text) = json_path(root, preferred).and_then(JsonValue::as_str) {
            return Ok(text.to_string());
        }
    }
    let text = match config.wire_format {
        ProviderWireFormat::OpenAiResponses => responses_text(root),
        ProviderWireFormat::ChatCompletions => chat_text(root),
        ProviderWireFormat::OllamaChat => root
            .get("message")
            .and_then(|message| message.get("content"))
            .and_then(JsonValue::as_str)
            .map(str::to_string),
    };
    text.ok_or_else(|| {
        ProviderAdapterError::InvalidResponse("provider text field was not found".to_string())
    })
}

fn json_path<'a>(root: &'a JsonValue, path: &str) -> Option<&'a JsonValue> {
    let mut current = root;
    for segment in path.split('.') {
        if segment.is_empty()
            || segment
                .chars()
                .any(|ch| !(ch.is_ascii_alphanumeric() || ch == '_' || ch == '-'))
        {
            return None;
        }
        current = current.get(segment)?;
    }
    Some(current)
}

fn responses_text(root: &JsonValue) -> Option<String> {
    if let Some(text) = root.get("output_text").and_then(JsonValue::as_str) {
        return Some(text.to_string());
    }
    for item in root.get("output")?.as_array()? {
        let Some(content) = item.get("content").and_then(JsonValue::as_array) else {
            continue;
        };
        for block in content {
            if matches!(
                block.get("type").and_then(JsonValue::as_str),
                Some("output_text" | "text")
            ) {
                if let Some(text) = block.get("text").and_then(JsonValue::as_str) {
                    return Some(text.to_string());
                }
            }
        }
    }
    None
}

fn chat_text(root: &JsonValue) -> Option<String> {
    let content = root
        .get("choices")?
        .as_array()?
        .first()?
        .get("message")?
        .get("content")?;
    if let Some(text) = content.as_str() {
        return Some(text.to_string());
    }
    let mut output = String::new();
    for block in content.as_array()? {
        if matches!(
            block.get("type").and_then(JsonValue::as_str),
            Some("text" | "output_text")
        ) {
            if let Some(text) = block.get("text").and_then(JsonValue::as_str) {
                output.push_str(text);
            }
        }
    }
    (!output.is_empty()).then_some(output)
}

fn token_usage(root: &JsonValue, wire_format: ProviderWireFormat) -> TokenUsage {
    let usage = root.get("usage");
    match wire_format {
        ProviderWireFormat::OpenAiResponses => TokenUsage {
            input_tokens: usage
                .and_then(|value| value.get("input_tokens"))
                .and_then(JsonValue::as_u64),
            output_tokens: usage
                .and_then(|value| value.get("output_tokens"))
                .and_then(JsonValue::as_u64),
            reasoning_tokens: usage
                .and_then(|value| value.get("output_tokens_details"))
                .and_then(|value| value.get("reasoning_tokens"))
                .and_then(JsonValue::as_u64),
        },
        ProviderWireFormat::ChatCompletions => TokenUsage {
            input_tokens: usage
                .and_then(|value| value.get("prompt_tokens"))
                .and_then(JsonValue::as_u64),
            output_tokens: usage
                .and_then(|value| value.get("completion_tokens"))
                .and_then(JsonValue::as_u64),
            reasoning_tokens: usage
                .and_then(|value| value.get("completion_tokens_details"))
                .and_then(|value| value.get("reasoning_tokens"))
                .and_then(JsonValue::as_u64),
        },
        ProviderWireFormat::OllamaChat => TokenUsage {
            input_tokens: root.get("prompt_eval_count").and_then(JsonValue::as_u64),
            output_tokens: root.get("eval_count").and_then(JsonValue::as_u64),
            reasoning_tokens: None,
        },
    }
}

fn thinking_evidence(
    root: &JsonValue,
    wire_format: ProviderWireFormat,
    usage: &TokenUsage,
) -> Option<ThinkingEvidence> {
    if usage.reasoning_tokens.is_some_and(|tokens| tokens > 0) {
        return Some(ThinkingEvidence {
            source: "usage.reasoning_tokens".to_string(),
            reasoning_tokens: usage.reasoning_tokens,
        });
    }
    let source = match wire_format {
        ProviderWireFormat::OpenAiResponses => root
            .get("output")
            .and_then(JsonValue::as_array)
            .is_some_and(|items| {
                items.iter().any(|item| {
                    matches!(
                        item.get("type").and_then(JsonValue::as_str),
                        Some("reasoning" | "thinking")
                    )
                })
            })
            .then_some("response.reasoning_block"),
        ProviderWireFormat::ChatCompletions => root
            .get("choices")
            .and_then(JsonValue::as_array)
            .and_then(|choices| choices.first())
            .and_then(|choice| choice.get("message"))
            .and_then(chat_thinking_source),
        ProviderWireFormat::OllamaChat => root
            .get("message")
            .and_then(|message| message.get("thinking"))
            .and_then(JsonValue::as_str)
            .is_some_and(|value| !value.trim().is_empty())
            .then_some("message.thinking"),
    }?;
    Some(ThinkingEvidence {
        source: source.to_string(),
        reasoning_tokens: usage.reasoning_tokens,
    })
}

fn chat_thinking_source(message: &JsonValue) -> Option<&'static str> {
    if message
        .get("reasoning_content")
        .and_then(JsonValue::as_str)
        .is_some_and(|value| !value.trim().is_empty())
    {
        return Some("message.reasoning_content");
    }
    message
        .get("content")
        .and_then(JsonValue::as_array)
        .is_some_and(|blocks| {
            blocks.iter().any(|block| {
                matches!(
                    block.get("type").and_then(JsonValue::as_str),
                    Some("reasoning" | "thinking")
                )
            })
        })
        .then_some("message.reasoning_block")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(kind: &str, format: &str) -> ProviderConfig {
        ProviderConfig::from_pairs(&[
            ("MATH_ATOMS_PROVIDER_KIND", kind),
            ("MATH_ATOMS_PROVIDER_FORMAT", format),
            ("MATH_ATOMS_PROVIDER_MODEL", "thinking-model"),
            ("MATH_ATOMS_PROVIDER_THINKING_LEVEL", "low"),
            ("OPENAI_API_KEY", "configured"),
            ("DEEPSEEK_API_KEY", "configured"),
            ("OLLAMA_API_KEY", "configured"),
        ])
    }

    #[test]
    fn chat_requires_real_reasoning_evidence() {
        let missing = r#"{"choices":[{"message":{"content":"ok"}}]}"#;
        let input = ReceiptInput {
            request_id: "turn-1",
            request_body: "{}",
            raw_response: missing,
            elapsed_ms: 1,
            output_limit: 100,
        };
        assert_eq!(
            parse_receipt(&config("deepseek", "chat"), input),
            Err(ProviderAdapterError::ThinkingEvidenceMissing)
        );

        let present = r#"{"choices":[{"message":{"reasoning_content":"checked","content":"ok"}}],"usage":{"prompt_tokens":2,"completion_tokens":3}}"#;
        let receipt = parse_receipt(
            &config("deepseek", "chat"),
            ReceiptInput {
                request_id: "turn-2",
                request_body: "{}",
                raw_response: present,
                elapsed_ms: 2,
                output_limit: 100,
            },
        )
        .unwrap();
        assert_eq!(receipt.text, "ok");
        assert_eq!(receipt.thinking.source, "message.reasoning_content");
    }
}
