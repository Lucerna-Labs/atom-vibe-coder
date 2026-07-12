# Provider Runtime Requirements

Atom Vibe Coder is a meticulous software-construction loop, not a one-shot text generator. Its provider must reason across planning, file generation, review, compiler feedback, repair, and final verification.

## Model floor

- A reasoning-capable model is mandatory.
- The recommended local minimum is Qwen3.5 9B Q8 with thinking enabled, or a demonstrably stronger thinking model.
- Lower quantizations may be used for diagnostics, but they do not qualify production release evidence.
- Smaller or lower-precision models may be evaluated, but they are not the documented production baseline.
- A model that only returns final text without positive reasoning evidence is rejected, even if the text looks plausible.

## Thinking policy

Thinking is enabled for every provider work packet and every correction attempt. Accepted levels are `low`, `medium`, and `high`. The default is `low`; this keeps reasoning active without using maximum effort. `none`, `off`, `disabled`, `max`, and `xhigh` are invalid and fail closed.

Set the policy with:

```powershell
$env:MATH_ATOMS_PROVIDER_THINKING_LEVEL="low"
```

The built-in request shapes map the level to the provider protocol:

- Responses API: `reasoning.effort`
- OpenAI-compatible and Mistral chat: `reasoning_effort`
- Ollama chat: `think`
- DeepSeek Pro: explicit `thinking.type = enabled`

For a custom body template, carry the controller-owned value with `{{thinking_json}}`:

```powershell
$env:MATH_ATOMS_PROVIDER_BODY_TEMPLATE='{"model":{{model_json}},"messages":[{"role":"system","content":{{instructions_json}}},{"role":"user","content":{{data_json}}}],"reasoning_effort":{{thinking_json}},"stream":false}'
```

Templates that omit the instructions, packet data, or thinking placeholder are invalid.

## Response proof

A successful transport response is not enough. The adapter requires positive response-side evidence that thinking ran:

- a positive reasoning-token count;
- a non-empty `reasoning_content` or Ollama `message.thinking` field; or
- a typed reasoning/thinking response block.

Missing evidence produces `ThinkingEvidenceMissing` and blocks the packet. This applies to initial generation and every repair, so a corrected artifact cannot bypass the thinking requirement.

## Preferred local Qwen profile

The previously validated diagnostic profile used LM Studio's OpenAI-compatible endpoint with Qwen3.5 9B Q6_K and low thinking. It proves adapter compatibility only; production qualification requires the Q8-or-stronger baseline and the full real-world gates:

```powershell
$env:MATH_ATOMS_PROVIDER_KIND="custom"
$env:MATH_ATOMS_PROVIDER_FORMAT="chat"
$env:MATH_ATOMS_PROVIDER_MODEL="atom-qwen35-9b-q6"
$env:MATH_ATOMS_PROVIDER_URL="http://127.0.0.1:1234/v1/chat/completions"
$env:MATH_ATOMS_PROVIDER_KEY_ENV="MATH_ATOMS_LOCAL_LMSTUDIO_KEY"
$env:MATH_ATOMS_LOCAL_LMSTUDIO_KEY="local-lmstudio"
$env:MATH_ATOMS_PROVIDER_THINKING_LEVEL="low"
```

The local server may ignore the dummy bearer credential, but Atom still scopes resumable work plans to the configured credential and endpoint. Production remote providers must use a real secret from the named environment variable.
