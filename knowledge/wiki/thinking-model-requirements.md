# Thinking Model Requirements
tags: provider, model, thinking, reasoning, qwen, qwen3.5, q8, release-gate

Status: active

Atom Vibe Coder requires thinking on every intake, blueprint, implementation,
review, correction, test, and launch-verification turn. Merely requesting
thinking is insufficient. The adapter must observe positive response-side
evidence such as nonzero reasoning tokens, a nonempty reasoning field, or a
typed reasoning/thinking block. Missing evidence fails closed.

The recommended minimum local baseline is Qwen3.5 9B Q8 with thinking enabled,
or a demonstrably stronger thinking model. Lower quantizations can be used for
diagnostics and adapter development but cannot qualify release evidence. Cloud
and custom models qualify by completing the same artifact-backed real-world
gates; a model name alone never counts as proof.

Thinking levels are bounded to low, medium, or high. Disabled thinking is
invalid, and maximum or xhigh modes are intentionally unsupported. Provider
switches create a new scratchpad model scope so one model does not inherit
another model's private working context.

[[wiki:provider-api]]
[[wiki:model-scratchpad]]
[[wiki:atom-vibe-build-spine]]
