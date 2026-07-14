#[derive(Debug, Clone, PartialEq)]
enum Token {
    Number(f64),
    Plus,
    Minus,
    Star,
    Slash,
    LParen,
    RParen,
}

#[derive(Debug, Clone, PartialEq)]
enum Expr {
    Number(f64),
    Neg(Box<Expr>),
    BinOp { op: char, lhs: Box<Expr>, rhs: Box<Expr> },
}

fn lex(input: &str) -> Result<Vec<Token>, String> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            ' ' | '\t' | '\r' | '\n' => i += 1,
            '+' => { tokens.push(Token::Plus); i += 1; }
            '-' => { tokens.push(Token::Minus); i += 1; }
            '*' => { tokens.push(Token::Star); i += 1; }
            '/' => { tokens.push(Token::Slash); i += 1; }
            '(' => { tokens.push(Token::LParen); i += 1; }
            ')' => { tokens.push(Token::RParen); i += 1; }
            '0'..='9' | '.' => {
                let start = i;
                while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                    i += 1;
                }
                let text: String = chars[start..i].iter().collect();
                let value = text.parse::<f64>().map_err(|_| format!("bad number '{text}'"))?;
                tokens.push(Token::Number(value));
            }
            other => return Err(format!("unexpected character '{other}'")),
        }
    }
    Ok(tokens)
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        Parser { tokens, pos: 0 }
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn next(&mut self) -> Option<Token> {
        let token = self.tokens.get(self.pos).cloned();
        if token.is_some() {
            self.pos += 1;
        }
        token
    }

    // expr := term (('+' | '-') term)*
    fn parse_expr(&mut self) -> Result<Expr, String> {
        let mut node = self.parse_term()?;
        while let Some(token) = self.peek() {
            let op = match token {
                Token::Plus => '+',
                Token::Minus => '-',
                _ => break,
            };
            self.pos += 1;
            let rhs = self.parse_term()?;
            node = Expr::BinOp { op, lhs: Box::new(node), rhs: Box::new(rhs) };
        }
        Ok(node)
    }

    // term := factor (('*' | '/') factor)*
    fn parse_term(&mut self) -> Result<Expr, String> {
        let mut node = self.parse_factor()?;
        while let Some(token) = self.peek() {
            let op = match token {
                Token::Star => '*',
                Token::Slash => '/',
                _ => break,
            };
            self.pos += 1;
            let rhs = self.parse_factor()?;
            node = Expr::BinOp { op, lhs: Box::new(node), rhs: Box::new(rhs) };
        }
        Ok(node)
    }

    // factor := '-' factor | '(' expr ')' | number
    fn parse_factor(&mut self) -> Result<Expr, String> {
        match self.next() {
            Some(Token::Minus) => Ok(Expr::Neg(Box::new(self.parse_factor()?))),
            Some(Token::Number(value)) => Ok(Expr::Number(value)),
            Some(Token::LParen) => {
                let inner = self.parse_expr()?;
                match self.next() {
                    Some(Token::RParen) => Ok(inner),
                    _ => Err("expected ')'".to_string()),
                }
            }
            other => Err(format!("unexpected token {other:?}")),
        }
    }
}

fn eval(expr: &Expr) -> Result<f64, String> {
    match expr {
        Expr::Number(value) => Ok(*value),
        Expr::Neg(inner) => Ok(-eval(inner)?),
        Expr::BinOp { op, lhs, rhs } => {
            let l = eval(lhs)?;
            let r = eval(rhs)?;
            match op {
                '+' => Ok(l + r),
                '-' => Ok(l - r),
                '*' => Ok(l * r),
                '/' if r == 0.0 => Err("division by zero".to_string()),
                '/' => Ok(l / r),
                other => Err(format!("unknown operator '{other}'")),
            }
        }
    }
}

fn evaluate(input: &str) -> Result<f64, String> {
    let tokens = lex(input)?;
    let mut parser = Parser::new(tokens);
    let ast = parser.parse_expr()?;
    if parser.pos != parser.tokens.len() {
        return Err("trailing tokens after expression".to_string());
    }
    eval(&ast)
}

fn main() {
    for expr in ["2 + 3 * (4 - 1)", "-(2 + 3) * 4", "10 / 4"] {
        match evaluate(expr) {
            Ok(value) => println!("{expr} = {value}"),
            Err(error) => println!("{expr} -> error: {error}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn respects_precedence_and_parens() {
        assert_eq!(evaluate("2 + 3 * 4"), Ok(14.0));
        assert_eq!(evaluate("2 + 3 * (4 - 1)"), Ok(11.0));
        assert_eq!(evaluate("-(2 + 3) * 4"), Ok(-20.0));
    }

    #[test]
    fn reports_errors() {
        assert!(evaluate("1 / 0").is_err());
        assert!(evaluate("2 +").is_err());
        assert!(evaluate("(1 + 2").is_err());
    }
}
