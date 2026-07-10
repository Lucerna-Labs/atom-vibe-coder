# Provider API
tags: provider, api, model, openai, ollama, credential

Provider calls are explicit execution paths. OpenAI uses the Responses API, Ollama Cloud uses the chat API, Mistral uses chat completions, DeepSeek defaults to V4 Pro with thinking enabled at the provider default and a 900-second bounded request window, and custom providers can select responses, chat completions, or ollama-chat wire formats with endpoint, model, key env, auth header, auth scheme, response-key, body-template, and bounded timeout controls. Each POST is submitted exactly once: timeout, 429, and 5xx responses become visible blockers because automatically repeating an ambiguous or costly generation could duplicate work. A separately bounded whole-plan deadline limits cumulative execution. Every provider path keeps credentials out of process arguments.

Successful provider execution must return to the Spiderweb Bus as provider-executed evidence before it is stored. Response bytes stream through bounded stdout and stderr readers with no response spool file; crossing 16 MiB closes the stream, and non-UTF-8 bodies fail closed before JSON parsing. The adapter requires a framed 2xx status and structurally validates the configured Responses, Chat Completions, or Ollama Chat envelope; top-level errors, wrong response paths, malformed JSON, empty output, and oversized output fail closed. Provider execution always uses meticulous work packets. Final-correction outputs are assembled locally, written as a content-addressed artifact, and bound to the canonical expanded work manifest. This produces `verification pending`; only a real product harness may create reusable success evidence.

Provider requests may include relevant durable learning excerpts. Trusted packet control is carried in a system or instructions role, while the operator request, graph evidence, and prior packet output are JSON-encoded in a separate user-data role. Instructions embedded in stored evidence therefore never gain controller placement. Valid JSON Unicode escapes are decoded strictly, including surrogate pairs, and malformed response strings fail closed.

[[provider-model-loop]]
[[gate:fail-closed]]
[[wiki:self-learning]]
[[wiki:work-packets]]
