# Provider API
tags: provider, api, model, openai, ollama, credential

Provider calls are explicit execution paths. OpenAI uses the Responses API, Ollama Cloud uses the chat API, Mistral uses chat completions, DeepSeek uses the V4 Flash chat completions model, and custom providers can select responses, chat completions, or ollama-chat wire formats with endpoint, model, key env, auth header, auth scheme, response-key, and custom body-template controls. Every provider path must keep credentials out of process arguments while surfacing 401, 429, and network failures as blockers.

Successful provider execution must return to the Spiderweb Bus as provider-executed evidence before it is stored. The adapter structurally validates the configured Responses, Chat Completions, or Ollama Chat envelope; top-level errors, wrong response paths, malformed JSON, empty output, and oversized output fail closed. The exact model output is written as a content-addressed artifact, and the proof store records its path, SHA-256 hash, and byte length so graph reload can recompute the evidence.

Provider requests may include relevant durable learning excerpts. Those excerpts are explicitly marked as untrusted historical data; instructions embedded in stored evidence must never override the current operator intent. Valid JSON Unicode escapes are decoded strictly, including surrogate pairs, and malformed response strings fail closed.

[[provider-model-loop]]
[[gate:fail-closed]]
[[wiki:self-learning]]
