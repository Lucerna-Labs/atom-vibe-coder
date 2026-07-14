# Stdin REPL with tokenizer, stack evaluator, and exhaustive error handling
tags: cli, stdin, repl, calculator, rpn, stack, tokenizer, parser, enum, error-handling, exhaustive-match, dependency-free

A reference architecture for a small std-only Rust program that reads one input line at a time from stdin, tokenizes it, evaluates it against a stack, and prints either a typed result or a clear error. The reference implementation is a reverse-polish-notation calculator but the shape is general: any REPL that walks input tokens through a stack machine follows the same skeleton.

## Structural pattern

- **Typed error enum first.** Before writing any parsing, declare an enum whose variants name every failure mode the program can hit — `UnknownToken`, `NotANumber`, `StackUnderflow`, `DivideByZero`, `TrailingOperands`. Every one of these carries only what a downstream reader needs to reproduce the failure; no anonymous strings.
- **Display impl uses exhaustive match.** Each variant produces its own message so the compiler blocks a silent hole when a variant is added later.
- **Read loop uses BufRead line iteration.** Wrap `io::stdin().lock()` in a `BufReader` and iterate `.lines()`. Each iteration is one independent evaluation attempt so a single bad line does not poison the next one.
- **Tokenizer returns `Vec<&str>` from a single `split_whitespace`.** No regex, no dependency, no ownership drama — the token slices live in the input line for the duration of the evaluation.
- **Stack machine is a `Vec<f64>`.** Push/pop are the only operations. Every operator variant is a match arm that pops the right number of operands and returns `Result`; no operator uses `unwrap`.

## When to imitate this pattern

Reach for this shape when the operator asks for a small CLI tool that reads one command per line and prints one line of output per command. The specific arithmetic is not the point; the borrow-safe stdin loop plus the exhaustive-match error enum plus the stack evaluator are the reusable structure.

## Anti-patterns to avoid

Do not `unwrap` inside the evaluator — that turns any user typo into a panic. Do not build a `String` per token when a `&str` slice into the line does the job. Do not swallow parse errors with `.ok()` — surface them as a real error variant so the user sees `unknown token 'x'` instead of silence.

## Related

[[wiki:atom-quantizer]]
[[wiki:production-app-build]]

## Reference implementation

knowledge/wiki/examples/stdin_rpn_calculator.rs — consult only if you need precise line-level syntax; the structural pattern described above is what to imitate.
