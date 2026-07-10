# Provider API
tags: provider, api, model, openai, ollama, credential

Provider calls are explicit execution paths. OpenAI uses the Responses API, Ollama Cloud uses the chat API, Mistral uses chat completions, DeepSeek defaults to V4 Pro with thinking enabled, maximum reasoning effort, and a 900-second bounded request window, and custom providers can select responses, chat completions, or ollama-chat wire formats with endpoint, model, key env, auth header, auth scheme, response-key, body-template, and bounded timeout controls. Transient timeout, 429, and 5xx failures receive bounded retries; authentication and contract failures do not. Every provider path must keep credentials out of process arguments while surfacing failures as blockers.

Successful provider execution must return to the Spiderweb Bus as provider-executed evidence before it is stored. Response bodies are captured to a bounded temporary artifact before parsing, so a remote endpoint cannot force unbounded memory growth. The adapter structurally validates the configured Responses, Chat Completions, or Ollama Chat envelope; top-level errors, wrong response paths, malformed JSON, empty output, and oversized output fail closed. Provider execution always uses meticulous work packets. The corrected file outputs are assembled locally, written as a content-addressed artifact, and bound to the canonical expanded work manifest. This produces `verification pending`; only a real product harness may create reusable success evidence.

Provider requests may include relevant durable learning excerpts. Those excerpts are explicitly marked as untrusted historical data; instructions embedded in stored evidence must never override the current operator intent. Valid JSON Unicode escapes are decoded strictly, including surrogate pairs, and malformed response strings fail closed.

[[provider-model-loop]]
[[gate:fail-closed]]
[[wiki:self-learning]]
[[work-packets]]
