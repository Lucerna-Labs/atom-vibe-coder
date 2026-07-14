# Idiomatic error handling with a wrapping enum, From, and `?` propagation
tags: error-handling, result, enum, display, from, question-mark, parsing, config, key-value, dependency-free

A reference architecture for a small std-only Rust program that parses a text format into a typed struct while propagating errors correctly through every layer. The reference implementation parses `key = value` config lines, but the shape works for any single-file parser that needs to fail with a specific reason instead of `unwrap`ping and panicking.

## Structural pattern

- **One error enum wraps every lower-level failure.** Variants name domain failures directly (`MalformedLine`, `UnknownKey`, `InvalidInt`, `InvalidBool`); a separate variant carries `ParseIntError` via a `From` impl so `str.parse::<u64>()` can just use `?` at the call site.
- **`From` impls turn primitive errors into the domain error type.** `impl From<ParseIntError> for ConfigError` lets the parsing code write `s.parse::<u64>()?` instead of `s.parse::<u64>().map_err(ConfigError::InvalidInt)?`.
- **`Result` propagates all the way to `main`.** Nothing panics; nothing returns a bare `Option`. The top-level function returns `Result<_, ConfigError>` and `main` matches on the outcome to decide the exit code.
- **Display impl is exhaustive.** Each variant explains what went wrong in one sentence with enough detail for a user to fix their input.

## When to imitate this pattern

Reach for this shape whenever the operator asks for a parser, config loader, small CLI-argument reader, or anything that must convert text into a typed struct. This is also the correct starting point for any REPL that reads structured input.

## Anti-patterns to avoid

Do not use `unwrap` or `expect` inside the parser body. Do not return `Option` from parsing routines when a real reason exists — return `Result` with an enum variant. Do not add a `String` payload named `description` and then let the caller string-match on it; the enum variants themselves carry the parse fact directly.

## Related

[[wiki:production-app-build]]

## Reference implementation

knowledge/wiki/examples/error_handling_and_parsing.rs — consult only if you need precise line-level syntax; the structural pattern described above is what to imitate.
