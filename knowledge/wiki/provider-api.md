# Provider API
tags: provider, api, model, openai, ollama, credential

Provider calls are explicit execution paths. OpenAI uses the Responses API, Ollama Cloud uses the chat API, and both must keep credentials out of process arguments while surfacing 401, 429, and network failures as blockers.

[[provider-model-loop]]
[[gate:fail-closed]]
