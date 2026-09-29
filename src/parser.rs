use super::ast::{BinaryOp, CompareOp, Expr, Position, UnaryOp};
use super::lex::{Lexer, Token, TokenKind};

pub const MAX_FILTER_BYTES: usize = 64 * 1024;
pub const MAX_FILTER_TOKENS: usize = 512;
const MAX_PARSE_DEPTH: usize = 64;

struct Parser {
    tokens: std::vec::IntoIter<Token>,
    pushed: Option<Token>,
    eof: Token,
    depth: usize,
}

impl Parser {
    fn new(filter: &str) -> Result<Self, String> {
        if filter.len() > MAX_FILTER_BYTES {
            return Err(format!("filter exceeds {MAX_FILTER_BYTES} bytes"));
        }
        let mut lexer = Lexer::new(filter);
        let mut tokens = Vec::new();
        let mut count = 0;
        loop {
            let token = lexer.next_token();
            if token.kind == TokenKind::Eof {
                return Ok(Self {
                    tokens: tokens.into_iter(),
                    pushed: None,
                    eof: token,
                    depth: 0,
                });
            }
            if token.kind != TokenKind::Wsc {
                count += 1;
                if count > MAX_FILTER_TOKENS {
                    return Err(format!("{}: filter exceeds {MAX_FILTER_TOKENS} tokens", token.pos));
                }
            }
            tokens.push(token);
        }
    }

    fn next_token(&mut self) -> Token {
        self.pushed
            .take()
            .or_else(|| self.tokens.next())
            .unwrap_or_else(|| self.eof.clone())
    }

    fn push_token(&mut self, token: Token) {
        assert!(self.pushed.is_none(), "double push_token");
        self.pushed = Some(token);
    }

    fn nested<T>(&mut self, parse: impl FnOnce(&mut Self) -> Result<T, String>) -> Result<T, String> {
        if self.depth >= MAX_PARSE_DEPTH {
            return Err(format!("filter exceeds parser nesting limit of {MAX_PARSE_DEPTH}"));
        }
        self.depth += 1;
        let result = parse(self);
        self.depth -= 1;
        result
    }
}

/// Parse a filter expression string into an [`Expr`].
///
/// Returns `Ok(None)` for empty input, `Ok(Some(expr))` on success,
/// `Err(msg)` on parse error.
pub fn parse_filter(filter: &str) -> Result<Option<Expr>, String> {
    let mut lex = Parser::new(filter)?;

    let mut tok = lex.next_token();
    if tok.kind == TokenKind::Wsc {
        tok = lex.next_token();
    }
    if tok.kind == TokenKind::Eof {
        return Ok(None);
    }
    lex.push_token(tok);

    let expr = parse_expression(&mut lex)?;

    let mut tok = lex.next_token();
    if tok.kind == TokenKind::Wsc {
        tok = lex.next_token();
    }
    if tok.kind != TokenKind::Eof {
        return Err(format!("{}: unexpected tokens after filter expression", tok.pos));
    }

    expr.check_limits()?;
    Ok(Some(expr))
}

/// expression = sequence, { "AND", sequence } ;
fn parse_expression(lex: &mut Parser) -> Result<Expr, String> {
    parse_junction(lex, TokenKind::And, parse_sequence)
}

/// sequence = factor, { factor } ;
fn parse_sequence(lex: &mut Parser) -> Result<Expr, String> {
    let mut factor = parse_factor(lex)?;

    loop {
        let mut tok = lex.next_token();
        if tok.kind == TokenKind::Wsc {
            tok = lex.next_token();
        }
        lex.push_token(tok.clone());

        if tok.kind == TokenKind::Eof || tok.kind == TokenKind::Rparen {
            return Ok(factor);
        }

        let pos = tok.pos;

        if tok.kind == TokenKind::And {
            return Ok(factor);
        }

        let rfactor = parse_factor(lex)?;

        factor = Expr::Binary {
            op: BinaryOp::And,
            left: Box::new(factor),
            right: Box::new(rfactor),
            pos,
        };
    }
}

/// factor = term, { "OR", term } ;
fn parse_factor(lex: &mut Parser) -> Result<Expr, String> {
    parse_junction(lex, TokenKind::Or, parse_term)
}

/// Parse an AND or OR sequence.
fn parse_junction(
    lex: &mut Parser,
    kind: TokenKind,
    parse_element: fn(&mut Parser) -> Result<Expr, String>,
) -> Result<Expr, String> {
    let mut sub = parse_element(lex)?;

    loop {
        let mut tok = lex.next_token();
        if tok.kind == TokenKind::Wsc {
            tok = lex.next_token();
        }

        if tok.kind != kind {
            lex.push_token(tok);
            return Ok(sub);
        }

        let pos = tok.pos;

        tok = lex.next_token();
        if tok.kind != TokenKind::Wsc {
            lex.push_token(tok);
            let name = if kind == TokenKind::And { "AND" } else { "OR" };
            return Err(format!("{}: missing whitespace after {}", pos, name));
        }

        let rsub = parse_element(lex)?;

        let op = if kind == TokenKind::And {
            BinaryOp::And
        } else {
            BinaryOp::Or
        };
        sub = Expr::Binary {
            op,
            left: Box::new(sub),
            right: Box::new(rsub),
            pos,
        };
    }
}

/// term = [ "-" | "NOT" ], primitive ;
fn parse_term(lex: &mut Parser) -> Result<Expr, String> {
    lex.nested(parse_term_inner)
}

fn parse_term_inner(lex: &mut Parser) -> Result<Expr, String> {
    let tok = lex.next_token();
    let is_neg = tok.kind == TokenKind::Minus || tok.kind == TokenKind::Not;
    let neg_pos = tok.pos;
    let neg_kind = tok.kind;

    if !is_neg {
        lex.push_token(tok);
    }

    if neg_kind == TokenKind::Not {
        let stok = lex.next_token();
        if stok.kind != TokenKind::Wsc {
            lex.push_token(stok);
            return Err(format!("{}: missing whitespace after NOT", neg_pos));
        }
    }

    let ntok = lex.next_token();
    lex.push_token(ntok.clone());
    let prim = if ntok.kind == TokenKind::Minus || ntok.kind == TokenKind::Not {
        parse_term(lex)?
    } else {
        parse_primitive(lex)?
    };

    if is_neg {
        let op = if neg_kind == TokenKind::Minus {
            UnaryOp::Minus
        } else {
            UnaryOp::Not
        };
        Ok(Expr::Unary {
            op,
            expr: Box::new(prim),
            pos: neg_pos,
        })
    } else {
        Ok(prim)
    }
}

/// Parse a primitive (comparison, function call, parenthesized expr, or bare value).
fn parse_primitive(lex: &mut Parser) -> Result<Expr, String> {
    let mut tok = lex.next_token();
    if tok.kind == TokenKind::Wsc {
        tok = lex.next_token();
    }

    if tok.kind == TokenKind::Lparen {
        return parse_parenthesized_expression(lex);
    }

    let left = parse_comparable(lex, tok)?;

    let mut tok = lex.next_token();
    if tok.kind == TokenKind::Wsc {
        tok = lex.next_token();
    }

    let cmp_op = match tok.kind {
        TokenKind::LessThanEquals => Some(CompareOp::LessThanEquals),
        TokenKind::LessThan => Some(CompareOp::LessThan),
        TokenKind::GreaterThanEquals => Some(CompareOp::GreaterThanEquals),
        TokenKind::GreaterThan => Some(CompareOp::GreaterThan),
        TokenKind::NotEquals => Some(CompareOp::NotEquals),
        TokenKind::Equals => Some(CompareOp::Equals),
        TokenKind::Has => Some(CompareOp::Has),
        TokenKind::MatchesRegexp => Some(CompareOp::MatchesRegexp),
        TokenKind::NotMatchesRegexp => Some(CompareOp::NotMatchesRegexp),
        _ => None,
    };

    if let Some(op) = cmp_op {
        let pos = tok.pos;

        let ntok = lex.next_token();
        if ntok.kind != TokenKind::Wsc {
            lex.push_token(ntok);
        }

        let right = parse_arg(lex)?;

        // Validate regex on the right-hand side
        if op == CompareOp::MatchesRegexp || op == CompareOp::NotMatchesRegexp {
            validate_regex(&right)?;
        }

        Ok(Expr::Comparison {
            op,
            left: Box::new(left),
            right: Box::new(right),
            pos,
        })
    } else {
        lex.push_token(tok);
        Ok(left)
    }
}

/// Validate that a regex RHS only contains quoted strings.
fn validate_regex(expr: &Expr) -> Result<(), String> {
    match expr {
        Expr::Binary { left, right, .. } => {
            validate_regex(left)?;
            validate_regex(right)
        }
        Expr::Unary { expr, .. } => validate_regex(expr),
        Expr::Name { is_string, pos, name, .. } => {
            if !is_string {
                return Err(format!("{}: regular expression is not a quoted string", pos));
            }
            // Validate the regex syntax
            if let Err(e) = crate::compiled::compile_regex(name) {
                return Err(format!("{}: invalid regular expression: {}", pos, e));
            }
            Ok(())
        }
        Expr::Comparison { pos, .. } | Expr::Member { pos, .. } => {
            Err(format!("{}: regular expression is not a quoted string", pos))
        }
        Expr::Function { func, .. } => {
            // Use the function name position
            let pos = func_pos(func);
            Err(format!("{}: regular expression is not a quoted string", pos))
        }
    }
}

fn func_pos(expr: &Expr) -> Position {
    match expr {
        Expr::Name { pos, .. } | Expr::Member { pos, .. } => *pos,
        _ => Position { line: 0, col: 0 },
    }
}

/// Parse a parenthesized expression. We have already consumed the `(`.
fn parse_parenthesized_expression(lex: &mut Parser) -> Result<Expr, String> {
    lex.nested(parse_parenthesized_inner)
}

fn parse_parenthesized_inner(lex: &mut Parser) -> Result<Expr, String> {
    let tok = lex.next_token();
    if tok.kind != TokenKind::Wsc {
        lex.push_token(tok);
    }

    let mut ret = parse_expression(lex)?;

    // Mark all name exprs as composite
    mark_composite(&mut ret);

    let tok = lex.next_token();
    if tok.kind != TokenKind::Rparen {
        lex.push_token(tok.clone());
        return Err(format!(
            "{}: expected right parenthesis after expression",
            tok.pos
        ));
    }

    Ok(ret)
}

fn mark_composite(expr: &mut Expr) {
    match expr {
        Expr::Binary { left, right, .. } => {
            mark_composite(left);
            mark_composite(right);
        }
        Expr::Unary { expr, .. } => {
            mark_composite(expr);
        }
        Expr::Comparison { left, right, .. } => {
            mark_composite(left);
            mark_composite(right);
        }
        Expr::Name { is_composite, .. } => {
            *is_composite = true;
        }
        Expr::Member { holder, .. } => {
            mark_composite(holder);
        }
        Expr::Function { .. } => {}
    }
}

/// Parse a comparable (name, member, function).
fn parse_comparable(lex: &mut Parser, tok: Token) -> Result<Expr, String> {
    let mut saw_white = false;
    let mut ntok = lex.next_token();
    if ntok.kind == TokenKind::Wsc {
        saw_white = true;
        ntok = lex.next_token();
    }
    lex.push_token(ntok.clone());

    let tok = match tok.kind {
        TokenKind::StringLit | TokenKind::Text => tok,
        TokenKind::And | TokenKind::Or | TokenKind::Not => {
            if ntok.kind != TokenKind::Dot {
                return Err(format!("{}: misplaced AND/OR/NOT", tok.pos));
            }
            keyword_to_text(tok)
        }
        _ => return Err(format!("{}: expected identifier or value", tok.pos)),
    };

    match ntok.kind {
        TokenKind::Dot => {
            lex.next_token(); // consume dot
            let dot_pos = ntok.pos;
            let e = parse_member(lex, &tok, dot_pos)?;

            // Check if member is used as a function call
            let ftok = lex.next_token();
            if ftok.kind != TokenKind::Lparen || saw_white {
                lex.push_token(ftok);
                return Ok(e);
            }
            parse_function(lex, e)
        }
        TokenKind::Lparen => {
            let fn_expr = Expr::Name {
                name: tok.val.clone(),
                pos: tok.pos,
                is_string: false,
                is_composite: false,
            };

            if saw_white || tok.val.is_empty() || tok.val.starts_with(|c: char| c.is_ascii_digit())
            {
                return Ok(fn_expr);
            }

            lex.next_token(); // consume lparen
            parse_function(lex, fn_expr)
        }
        _ => Ok(Expr::Name {
            name: tok.val,
            pos: tok.pos,
            is_string: tok.kind == TokenKind::StringLit,
            is_composite: false,
        }),
    }
}

/// Parse member access: `identifier { "." identifier }`.
fn parse_member(lex: &mut Parser, tok: &Token, pos: Position) -> Result<Expr, String> {
    let mut ret: Expr = Expr::Name {
        name: tok.val.clone(),
        pos: tok.pos,
        is_string: false,
        is_composite: false,
    };

    let mut dot_pos = pos;

    loop {
        let ntok = lex.next_token();
        let ntok = match ntok.kind {
            TokenKind::StringLit | TokenKind::Text => ntok,
            TokenKind::And | TokenKind::Or | TokenKind::Not => keyword_to_text(ntok),
            _ => return Err(format!("{}: expected identifier", ntok.pos)),
        };

        ret = Expr::Member {
            holder: Box::new(ret),
            member: ntok.val,
            pos: dot_pos,
        };

        let next = lex.next_token();
        if next.kind != TokenKind::Dot {
            lex.push_token(next);
            return Ok(ret);
        }
        dot_pos = next.pos;
    }
}

/// Parse a function call. We've consumed the `(`.
fn parse_function(lex: &mut Parser, func: Expr) -> Result<Expr, String> {
    lex.nested(|lex| parse_function_inner(lex, func))
}

fn parse_function_inner(lex: &mut Parser, func: Expr) -> Result<Expr, String> {
    let tok = lex.next_token();
    if tok.kind == TokenKind::Rparen {
        return Ok(Expr::Function {
            func: Box::new(func),
            args: vec![],
        });
    }
    lex.push_token(tok);

    let mut args = Vec::new();
    loop {
        let arg = parse_arg(lex)?;
        args.push(arg);

        let mut tok = lex.next_token();
        if tok.kind == TokenKind::Rparen {
            return Ok(Expr::Function {
                func: Box::new(func),
                args,
            });
        }
        if tok.kind == TokenKind::Wsc {
            tok = lex.next_token();
        }
        if tok.kind != TokenKind::Comma {
            return Err(format!(
                "{}: expected comma after function argument",
                tok.pos
            ));
        }

        tok = lex.next_token();
        if tok.kind != TokenKind::Wsc {
            lex.push_token(tok);
        }
    }
}

/// Parse an argument (comparable, value, or parenthesized expression).
fn parse_arg(lex: &mut Parser) -> Result<Expr, String> {
    let tok = lex.next_token();
    if tok.kind == TokenKind::Lparen {
        return parse_parenthesized_expression(lex);
    }
    parse_comparable(lex, tok)
}

/// Convert a keyword token (AND/OR/NOT) to a text token.
fn keyword_to_text(tok: Token) -> Token {
    let val = match tok.kind {
        TokenKind::And => "AND",
        TokenKind::Or => "OR",
        TokenKind::Not => "NOT",
        _ => unreachable!(),
    };
    Token {
        kind: TokenKind::Text,
        pos: tok.pos,
        val: val.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_push_token() {
        let mut parser = Parser::new("a b").unwrap();
        let token = parser.next_token();
        assert_eq!(token.val, "a");
        parser.push_token(token);
        assert_eq!(parser.next_token().val, "a");
    }

    #[test]
    fn test_empty() {
        assert!(parse_filter("").unwrap().is_none());
        assert!(parse_filter("  ").unwrap().is_none());
    }

    #[test]
    fn test_basic_comparison() {
        let expr = parse_filter("x = 42").unwrap().unwrap();
        match expr {
            Expr::Comparison { op, left, right, .. } => {
                assert_eq!(op, CompareOp::Equals);
                match *left {
                    Expr::Name { ref name, .. } => assert_eq!(name, "x"),
                    _ => panic!("expected name"),
                }
                match *right {
                    Expr::Name { ref name, .. } => assert_eq!(name, "42"),
                    _ => panic!("expected name"),
                }
            }
            _ => panic!("expected comparison"),
        }
    }

    #[test]
    fn test_conjunction() {
        let expr = parse_filter("a : b c = 17").unwrap().unwrap();
        match expr {
            Expr::Binary { op: BinaryOp::And, .. } => {}
            _ => panic!("expected conjunction"),
        }
    }

    #[test]
    fn test_negation() {
        let expr = parse_filter("-a < 1").unwrap().unwrap();
        match expr {
            Expr::Unary { op: UnaryOp::Minus, .. } => {}
            _ => panic!("expected unary minus"),
        }
    }

    #[test]
    fn test_member() {
        let expr = parse_filter("a.b > 17").unwrap().unwrap();
        match expr {
            Expr::Comparison { left, .. } => match *left {
                Expr::Member { ref member, .. } => assert_eq!(member, "b"),
                _ => panic!("expected member"),
            },
            _ => panic!("expected comparison"),
        }
    }

    #[test]
    fn test_function() {
        let expr = parse_filter("func(val)").unwrap().unwrap();
        match expr {
            Expr::Function { func, args } => {
                match *func {
                    Expr::Name { ref name, .. } => assert_eq!(name, "func"),
                    _ => panic!("expected name"),
                }
                assert_eq!(args.len(), 1);
            }
            _ => panic!("expected function"),
        }
    }

    #[test]
    fn test_error_double_equals() {
        let err = parse_filter("x == 42").unwrap_err();
        assert!(err.contains("expected identifier or value"), "got: {err}");
    }

    #[test]
    fn test_error_misplaced_keyword() {
        let err = parse_filter("a : AND b  c = 17").unwrap_err();
        assert!(err.contains("misplaced AND/OR/NOT"), "got: {err}");
    }
}
