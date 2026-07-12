# Provider and Model Requirements

## Mandatory Thinking

Thinking is mandatory for every provider turn. Atom Vibe Coder fails closed at
both boundaries:

1. The provider configuration must select `low`, `medium`, or `high` thinking.
2. The response must expose nonzero reasoning tokens, a nonempty reasoning field,
   or a typed reasoning/thinking block.

A provider returning ordinary text without that evidence produces
`ThinkingEvidenceMissing`; the output cannot enter the scratchpad, gate, ledger,
learning graph, or generated project.

Maximum and `xhigh` modes are intentionally unsupported. The objective is
reliable reasoning, not maximum token consumption.

## Recommended Minimum

The recommended minimum local baseline is **Qwen3.5 9B Q8 with thinking enabled**,
or a demonstrably stronger thinking model. Q8 is the release baseline, not merely
a memory-dependent preference. Q6, Q5, Q4, and smaller models may be used to
debug transport and prompts but do not qualify production release evidence.

Cloud and custom models are not ranked by brand name. They qualify only when
they expose thinking evidence and pass the same artifact-backed real workflows.

## Supported Wire Formats

| Format | Request shape | Accepted thinking evidence |
| --- | --- | --- |
| OpenAI Responses | `instructions`, user `input`, `reasoning.effort` | reasoning token usage or typed reasoning block |
| Chat Completions | system/user messages and `reasoning_effort` | reasoning token usage, `reasoning_content`, or typed block |
| DeepSeek Pro chat | system/user messages and `thinking.type=enabled` | reasoning token usage or `reasoning_content` |
| Ollama chat | system/user messages and `think` | nonempty `message.thinking` |
| Custom | validated body template | evidence appropriate to the selected response format |

Custom templates must include instruction, data, and thinking placeholders. A
credential is named by environment variable; its value is never persisted.

## Local Qwen Example

For an OpenAI-compatible loopback server exposing Qwen3.5 9B Q8:

```powershell
$env:MATH_ATOMS_PROVIDER_KIND="custom"
$env:MATH_ATOMS_PROVIDER_FORMAT="chat"
$env:MATH_ATOMS_PROVIDER_MODEL="qwen3.5-9b-q8"
$env:MATH_ATOMS_PROVIDER_URL="http://127.0.0.1:1234/v1/chat/completions"
$env:MATH_ATOMS_PROVIDER_KEY_ENV="MATH_ATOMS_PROVIDER_API_KEY"
$env:MATH_ATOMS_PROVIDER_API_KEY="session-local-value"
$env:MATH_ATOMS_PROVIDER_THINKING_LEVEL="low"
```

The server must expose reasoning through a supported response field. Visible
`<think>` prose embedded in ordinary answer text is not trusted as controller
evidence.

## Provider Switching

Provider and model identity are part of the scratchpad scope. Switching either
opens a separate scratchpad projection and invalidates prepared calls tied to the
previous credential scope. This prevents cross-model hidden-context leakage.
