# Patterns — Map of Content
tags: index, moc, patterns, navigation

Reusable structural patterns each backed by a compileable exemplar under `knowledge/wiki/examples/`. Each note here describes a pattern in prose so a model can adapt the shape; the code file is only read when precise line-level syntax is needed.

## Members

- [[wiki:patterns:stdin-rpn-calculator]] — stdin REPL with tokenizer, stack evaluator, and exhaustive error handling.
- [[wiki:patterns:error-handling-and-parsing]] — idiomatic error handling with a wrapping enum, `From`, and `?` propagation.
- [[wiki:patterns:recursive-descent-expr]] — lexer plus recursive-descent parser plus `Box`ed AST evaluator.
- [[wiki:patterns:text-wordcount]] — HashMap counting with iterator chains and stable top-N sorting.
- [[wiki:patterns:atom-stack-kernel]] — atom-stack kernel driver (scan → hash → project → compare → order).
- [[wiki:patterns:cross-domain-atom-stack]] — cross-domain atom composition (PRE → EXTRACT → QUANTIZE → POST → VERIFY).
- [[wiki:patterns:typed-bus-strand]] — typed pub/sub bus with Strand trait, Socket by TypeId, deterministic tick executor.

## Parent

[[wiki:index]]
