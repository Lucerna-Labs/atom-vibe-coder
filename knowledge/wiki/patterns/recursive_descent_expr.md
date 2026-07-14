# Lexer plus recursive-descent parser plus Boxed AST evaluator
tags: parser, recursive-descent, ast, lexer, tokenizer, expression, interpreter, precedence, enum, box, recursion, dependency-free

A reference architecture for a single-file std-only Rust program that lexes text into tokens, parses the tokens into a recursive AST, and evaluates the AST — all with correct precedence, associativity, parentheses, and unary operators. The reference implementation is an infix arithmetic evaluator, but the shape generalises to any language whose grammar can be expressed as a small precedence hierarchy.

## Structural pattern

- **AST is a `Box`-recursive enum.** Variants like `Number(f64)`, `Neg(Box<Expr>)`, `Add(Box<Expr>, Box<Expr>)`. `Box` is what makes the enum finite-sized; without it, the compiler rejects the recursion. This is the smallest working idiom for a Rust AST.
- **Lexer returns `Vec<Token>` in one pass.** No lookback, no backtracking. Tokens are their own enum with structured payloads for numbers.
- **Parser is a struct with a token cursor.** Methods form the precedence ladder: `parse_expr` calls `parse_term`, which calls `parse_factor`, which calls `parse_unary`, which calls `parse_primary`. Each level handles one precedence class and delegates the rest downward.
- **Evaluator is a single `fn eval(&Expr) -> f64` that walks the AST recursively.** Because the AST is exhaustive, adding a token means adding a variant and updating three functions — every call site fails at compile time until you handle it.

## When to imitate this pattern

Reach for this shape whenever the operator asks for a small expression evaluator, formula parser, mini-language interpreter, template engine, or any tool that turns structured input text into a runtime tree it then walks. Also correct for a config-language parser more sophisticated than the flat `key = value` shape.

## Anti-patterns to avoid

Do not use `Rc` or `Arc` for the AST when `Box` is enough — the AST is not shared. Do not thread the tokens through as `&[Token]` when a cursor struct is cleaner. Do not skip precedence classes to save typing; grammar bugs appear as evaluation surprises later. Do not use `unwrap` in the parser body — every syntax error must be a real `Result` variant.

## Related

[[wiki:production-app-build]]

## Reference implementation

knowledge/wiki/examples/recursive_descent_expr.rs — consult only if you need precise line-level syntax; the structural pattern described above is what to imitate.
