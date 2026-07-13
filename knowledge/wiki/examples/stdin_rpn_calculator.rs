// EXEMPLAR: stdin REPL, tokenizer, stack evaluator, exhaustive error handling.
// tags: cli, stdin, repl, calculator, rpn, stack, tokenizer, parser, enum, error-handling,
//       exhaustive-match, dependency-free
//
// A reverse-polish-notation calculator: reads one expression per line from stdin,
// tokenizes it, evaluates with a stack, and prints the result or a clear error.
// Demonstrates: a custom error enum with Display, exhaustive `match`, borrow-safe
// string handling, and a clean stdin loop — the idioms small models most often get wrong.

use std::io::{self, BufRead, Write};

#[derive(Debug, Clone, PartialEq)]
enum CalcError {
    UnknownToken(String),
    NotANumber(String),
    StackUnderflow,
    DivideByZero,
    TrailingOperands(usize),
}

impl std::fmt::Display for CalcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Exhaustive: every variant is covered, so adding one is a compile error until handled.
        match self {
            CalcError::UnknownToken(token) => write!(f, "unknown token '{token}'"),
            CalcError::NotANumber(token) => write!(f, "'{token}' is not a number"),
            CalcError::StackUnderflow => write!(f, "not enough operands on the stack"),
            CalcError::DivideByZero => write!(f, "division by zero"),
            CalcError::TrailingOperands(n) => write!(f, "{n} leftover operand(s); expected exactly one result"),
        }
    }
}

fn eval_rpn(line: &str) -> Result<f64, CalcError> {
    let mut stack: Vec<f64> = Vec::new();
    for token in line.split_whitespace() {
        match token {
            "+" | "-" | "*" | "/" => {
                let rhs = stack.pop().ok_or(CalcError::StackUnderflow)?;
                let lhs = stack.pop().ok_or(CalcError::StackUnderflow)?;
                let value = match token {
                    "+" => lhs + rhs,
                    "-" => lhs - rhs,
                    "*" => lhs * rhs,
                    "/" => {
                        if rhs == 0.0 {
                            return Err(CalcError::DivideByZero);
                        }
                        lhs / rhs
                    }
                    // Unreachable given the outer arm, but written so the match is total.
                    other => return Err(CalcError::UnknownToken(other.to_string())),
                };
                stack.push(value);
            }
            number => {
                let parsed = number
                    .parse::<f64>()
                    .map_err(|_| CalcError::NotANumber(number.to_string()))?;
                stack.push(parsed);
            }
        }
    }
    match stack.len() {
        0 => Err(CalcError::StackUnderflow),
        1 => Ok(stack[0]),
        n => Err(CalcError::TrailingOperands(n - 1)),
    }
}

fn main() {
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    println!("RPN calculator. Enter e.g. `3 4 + 5 *`, blank line to quit.");
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(text) => text,
            Err(error) => {
                eprintln!("input error: {error}");
                break;
            }
        };
        if line.trim().is_empty() {
            break;
        }
        match eval_rpn(&line) {
            Ok(result) => {
                let _ = writeln!(stdout, "= {result}");
            }
            Err(error) => {
                let _ = writeln!(stdout, "error: {error}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evaluates_basic_expressions() {
        assert_eq!(eval_rpn("3 4 +"), Ok(7.0));
        assert_eq!(eval_rpn("3 4 + 5 *"), Ok(35.0));
    }

    #[test]
    fn reports_errors() {
        assert_eq!(eval_rpn("3 +"), Err(CalcError::StackUnderflow));
        assert_eq!(eval_rpn("1 0 /"), Err(CalcError::DivideByZero));
        assert_eq!(eval_rpn("3 4"), Err(CalcError::TrailingOperands(1)));
        assert!(matches!(eval_rpn("3 x +"), Err(CalcError::NotANumber(_))));
    }
}
