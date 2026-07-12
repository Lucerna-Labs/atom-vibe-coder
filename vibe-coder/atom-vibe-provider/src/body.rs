use crate::{ProviderAdapterError, ProviderRequest};
use math_atoms_core::{ProviderConfig, ProviderKind, ProviderThinkingLevel, ProviderWireFormat};

pub(crate) fn request_body(
    config: &ProviderConfig,
    request: &ProviderRequest,
) -> Result<String, ProviderAdapterError> {
    let thinking = config.thinking_level.ok_or_else(|| {
        ProviderAdapterError::InvalidConfiguration("thinking must be enabled".to_string())
    })?;
    if !config.body_template.trim().is_empty() {
        for field in ["instructions", "data", "thinking"] {
            if !template_has_value(&config.body_template, field) {
                return Err(ProviderAdapterError::InvalidConfiguration(format!(
                    "custom body template is missing {field}"
                )));
            }
        }
        return Ok(render_template(config, request, thinking));
    }
    Ok(match config.wire_format {
        ProviderWireFormat::OpenAiResponses => format!(
            "{{\"model\":\"{}\",\"instructions\":\"{}\",\"input\":[{{\"role\":\"user\",\"content\":[{{\"type\":\"input_text\",\"text\":\"{}\"}}]}}],\"reasoning\":{{\"effort\":\"{}\"}},\"temperature\":0.1}}",
            json_escape(&config.model),
            json_escape(&request.system_instructions),
            json_escape(&request.data),
            thinking.as_str()
        ),
        ProviderWireFormat::OllamaChat => format!(
            "{{\"model\":\"{}\",\"messages\":[{{\"role\":\"system\",\"content\":\"{}\"}},{{\"role\":\"user\",\"content\":\"{}\"}}],\"think\":\"{}\",\"stream\":false}}",
            json_escape(&config.model),
            json_escape(&request.system_instructions),
            json_escape(&request.data),
            thinking.as_str()
        ),
        ProviderWireFormat::ChatCompletions
            if config.kind == ProviderKind::DeepSeekChat
                && config.model.to_ascii_lowercase().contains("pro") =>
        {
            format!(
                "{{\"model\":\"{}\",\"messages\":[{{\"role\":\"system\",\"content\":\"{}\"}},{{\"role\":\"user\",\"content\":\"{}\"}}],\"thinking\":{{\"type\":\"enabled\"}},\"stream\":false}}",
                json_escape(&config.model),
                json_escape(&request.system_instructions),
                json_escape(&request.data)
            )
        }
        ProviderWireFormat::ChatCompletions => format!(
            "{{\"model\":\"{}\",\"messages\":[{{\"role\":\"system\",\"content\":\"{}\"}},{{\"role\":\"user\",\"content\":\"{}\"}}],\"reasoning_effort\":\"{}\",\"temperature\":0.1,\"stream\":false}}",
            json_escape(&config.model),
            json_escape(&request.system_instructions),
            json_escape(&request.data),
            thinking.as_str()
        ),
    })
}

fn render_template(
    config: &ProviderConfig,
    request: &ProviderRequest,
    thinking: ProviderThinkingLevel,
) -> String {
    config
        .body_template
        .replace("{{model}}", &json_escape(&config.model))
        .replace(
            "{{instructions}}",
            &json_escape(&request.system_instructions),
        )
        .replace("{{data}}", &json_escape(&request.data))
        .replace(
            "{{model_json}}",
            &format!("\"{}\"", json_escape(&config.model)),
        )
        .replace(
            "{{instructions_json}}",
            &format!("\"{}\"", json_escape(&request.system_instructions)),
        )
        .replace(
            "{{data_json}}",
            &format!("\"{}\"", json_escape(&request.data)),
        )
        .replace("{{thinking}}", thinking.as_str())
        .replace("{{thinking_json}}", &format!("\"{}\"", thinking.as_str()))
}

fn template_has_value(template: &str, name: &str) -> bool {
    template.contains(&format!("{{{{{name}}}}}"))
        || template.contains(&format!("{{{{{name}_json}}}}"))
}

fn json_escape(input: &str) -> String {
    let mut output = String::new();
    for ch in input.chars() {
        match ch {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            ch if ch.is_control() => output.push(' '),
            ch => output.push(ch),
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use math_atoms_json::parse;

    #[test]
    fn bodies_are_valid_json_for_every_wire_format() {
        for (kind, format) in [
            ("openai", "responses"),
            ("mistral", "chat"),
            ("ollama", "ollama-chat"),
            ("deepseek-pro", "chat"),
        ] {
            let config = ProviderConfig::from_pairs(&[
                ("MATH_ATOMS_PROVIDER_KIND", kind),
                ("MATH_ATOMS_PROVIDER_FORMAT", format),
                ("MATH_ATOMS_PROVIDER_MODEL", "thinking-model"),
                ("MATH_ATOMS_PROVIDER_THINKING_LEVEL", "medium"),
                ("OPENAI_API_KEY", "configured"),
                ("MISTRAL_API_KEY", "configured"),
                ("OLLAMA_API_KEY", "configured"),
                ("DEEPSEEK_API_KEY", "configured"),
            ]);
            let body =
                request_body(&config, &ProviderRequest::new("turn-1", "system", "data")).unwrap();
            assert!(parse(&body).is_ok(), "{kind}: {body}");
        }
    }
}
