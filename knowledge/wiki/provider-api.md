# Provider API
tags: provider, api, model, openai, ollama, credential

Provider calls are explicit execution paths. OpenAI uses the Responses API, Ollama Cloud uses the chat API, Mistral uses chat completions, DeepSeek uses the V4 Flash chat completions model, and custom providers can select responses, chat completions, or ollama-chat wire formats with endpoint, model, key env, auth header, auth scheme, response-key, and custom body-template controls. Every provider path must keep credentials out of process arguments while surfacing 401, 429, and network failures as blockers.

Successful provider execution must return to the Spiderweb Bus as provider-executed evidence before it is stored. The proof store records the provider model, endpoint, output byte length, and a stable output hash so wiki graph retrieval can distinguish a prepared provider request from a model answer that actually ran.

[[provider-model-loop]]
[[gate:fail-closed]]
