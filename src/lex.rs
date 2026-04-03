use super::ast::Position;

/// Token kinds produced by the lexer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TokenKind {
    InvalidInput,
    Eof,
    Wsc,                // whitespace or comment
    Dot,                // .
    Has,                // :
    Or,                 // OR
    And,                // AND
    Not,                // NOT
    Lparen,             // (
    Rparen,             // )
    Comma,              // ,
    LessThan,           // <
    GreaterThan,        // >
    GreaterThanEquals,  // >=
    LessThanEquals,     // <=
    NotEquals,          // !=
    MatchesRegexp,      // =~
    NotMatchesRegexp,   // !~
    Equals,             // =
    Minus,              // -
    Plus,               // +
    Tilde,              // ~
    #[allow(dead_code)]
    Backslash,  // backslash
    StringLit,  // quoted string
    Text,       // unquoted text/name
}

/// A lexical token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Token {
    pub kind: TokenKind,
    pub pos: Position,
    pub val: String, // only set for StringLit and Text
}

/// Lexer converts a filter string into tokens.
pub(crate) struct Lexer {
    input: Vec<char>,
    cursor: usize,
    pushed: Option<Token>,
    prev_dot: bool,
    prev_text: bool,
    next_pos: Position,
}

impl Lexer {
    pub fn new(input: &str) -> Self {
        Lexer {
            input: input.chars().collect(),
            cursor: 0,
            pushed: None,
            prev_dot: false,
            prev_text: false,
            next_pos: Position { line: 1, col: 0 },
        }
    }

    pub fn next_token(&mut self) -> Token {
        if let Some(tok) = self.pushed.take() {
            return tok;
        }

        let pos = self.next_pos;
        let prev_dot = self.prev_dot;
        let prev_text = self.prev_text;
        self.prev_dot = false;
        self.prev_text = false;

        let Some(r) = self.peek() else {
            return Token { kind: TokenKind::Eof, pos, val: String::new() };
        };

        self.advance();

        match r {
            _ if is_white(r) => {
                self.skip_white();
                Token { kind: TokenKind::Wsc, pos, val: String::new() }
            }
            '-' => {
                if let Some(rn) = self.peek() {
                    if rn == '-' {
                        self.advance();
                        self.skip_comment();
                        self.skip_white();
                        return Token { kind: TokenKind::Wsc, pos, val: String::new() };
                    }
                    if is_digit(rn) {
                        return self.text(r, prev_dot);
                    }
                    if !prev_text && rn == '.'
                        && let Some(&next_after) = self.input.get(self.cursor + 1)
                            && is_digit(next_after) {
                                return self.text(r, prev_dot);
                            }
                }
                Token { kind: TokenKind::Minus, pos, val: String::new() }
            }
            '.' => {
                if self.peek().is_none() {
                    self.prev_dot = true;
                    return Token { kind: TokenKind::Dot, pos, val: String::new() };
                }
                if !prev_text
                    && let Some(rn) = self.peek()
                        && is_digit(rn) {
                            return self.text(r, prev_dot);
                        }
                self.prev_dot = true;
                Token { kind: TokenKind::Dot, pos, val: String::new() }
            }
            ':' => Token { kind: TokenKind::Has, pos, val: String::new() },
            'O' => {
                if self.remaining() > 0 && self.input[self.cursor] == 'R'
                    && (self.remaining() < 2 || is_white(self.input[self.cursor + 1]))
                {
                    self.advance();
                    return Token { kind: TokenKind::Or, pos, val: String::new() };
                }
                self.text(r, prev_dot)
            }
            'A' => {
                if self.remaining() > 1
                    && self.input[self.cursor] == 'N'
                    && self.input[self.cursor + 1] == 'D'
                    && (self.remaining() < 3 || is_white(self.input[self.cursor + 2]))
                {
                    self.advance();
                    self.advance();
                    return Token { kind: TokenKind::And, pos, val: String::new() };
                }
                self.text(r, prev_dot)
            }
            'N' => {
                if self.remaining() > 1
                    && self.input[self.cursor] == 'O'
                    && self.input[self.cursor + 1] == 'T'
                    && (self.remaining() < 3 || is_white(self.input[self.cursor + 2]))
                {
                    self.advance();
                    self.advance();
                    return Token { kind: TokenKind::Not, pos, val: String::new() };
                }
                self.text(r, prev_dot)
            }
            '(' => Token { kind: TokenKind::Lparen, pos, val: String::new() },
            ')' => Token { kind: TokenKind::Rparen, pos, val: String::new() },
            ',' => Token { kind: TokenKind::Comma, pos, val: String::new() },
            '<' => {
                if self.peek() == Some('=') {
                    self.advance();
                    Token { kind: TokenKind::LessThanEquals, pos, val: String::new() }
                } else {
                    Token { kind: TokenKind::LessThan, pos, val: String::new() }
                }
            }
            '>' => {
                if self.peek() == Some('=') {
                    self.advance();
                    Token { kind: TokenKind::GreaterThanEquals, pos, val: String::new() }
                } else {
                    Token { kind: TokenKind::GreaterThan, pos, val: String::new() }
                }
            }
            '!' => {
                if let Some(rn) = self.peek() {
                    if rn == '=' {
                        self.advance();
                        return Token { kind: TokenKind::NotEquals, pos, val: String::new() };
                    }
                    if rn == '~' {
                        self.advance();
                        return Token { kind: TokenKind::NotMatchesRegexp, pos, val: String::new() };
                    }
                }
                // bare ! is part of a name
                self.text(r, prev_dot)
            }
            '=' => {
                if self.peek() == Some('~') {
                    self.advance();
                    Token { kind: TokenKind::MatchesRegexp, pos, val: String::new() }
                } else {
                    Token { kind: TokenKind::Equals, pos, val: String::new() }
                }
            }
            '+' => Token { kind: TokenKind::Plus, pos, val: String::new() },
            '~' => Token { kind: TokenKind::Tilde, pos, val: String::new() },
            '"' => self.collect_string(),
            _ if is_text_start(r) || is_digit(r) => self.text(r, prev_dot),
            _ => Token { kind: TokenKind::InvalidInput, pos, val: String::new() },
        }
    }

    pub fn push_token(&mut self, tok: Token) {
        assert!(self.pushed.is_none(), "double push_token");
        self.pushed = Some(tok);
    }

    // Helpers.

    fn peek(&self) -> Option<char> {
        self.input.get(self.cursor).copied()
    }

    fn remaining(&self) -> usize {
        self.input.len() - self.cursor
    }

    fn advance(&mut self) -> Option<char> {
        if self.cursor >= self.input.len() {
            return None;
        }
        let r = self.input[self.cursor];
        self.cursor += 1;
        if r == '\n' {
            self.next_pos.line += 1;
            self.next_pos.col = 0;
        } else {
            self.next_pos.col += 1;
        }
        Some(r)
    }

    fn skip_white(&mut self) {
        loop {
            match self.peek() {
                Some(r) if is_white(r) => { self.advance(); }
                Some('-') => {
                    if self.input.get(self.cursor + 1) == Some(&'-') {
                        self.advance();
                        self.advance();
                        self.skip_comment();
                    } else {
                        return;
                    }
                }
                _ => return,
            }
        }
    }

    fn skip_comment(&mut self) {
        while let Some(r) = self.peek() {
            if r == '\t' || (' '..='~').contains(&r) || is_a1_or_higher(r) {
                self.advance();
            } else {
                return;
            }
        }
    }

    fn text(&mut self, first: char, prev_dot: bool) -> Token {
        let pos = self.next_pos;

        let mut sb = String::new();

        if first != '\\' {
            sb.push(first);
        } else if !self.text_esc(&mut sb) {
            return Token { kind: TokenKind::InvalidInput, pos, val: String::new() };
        }

        // Number prefix handling
        if first == '-' || first == '.' || is_digit(first) {
            let mut r = first;
            while let Some(rn) = self.peek() {
                if is_digit(rn) {
                    sb.push(rn);
                    self.advance();
                    if r == '.' {
                        break;
                    }
                } else if !prev_dot && r != '.' && rn == '.' {
                    sb.push(rn);
                    self.advance();
                    break;
                } else {
                    break;
                }
                if r == '-' {
                    r = rn;
                }
            }
        }

        loop {
            let Some(rn) = self.peek() else { break };
            if rn == '\\' {
                self.advance();
                if !self.text_esc(&mut sb) {
                    return Token { kind: TokenKind::InvalidInput, pos, val: String::new() };
                }
            } else if is_text_start(rn) || is_digit(rn) || rn == '+' || rn == '-' {
                sb.push(rn);
                self.advance();
            } else if rn == '!' {
                // ! is part of a name unless followed by = or ~
                if let Some(&next) = self.input.get(self.cursor + 1)
                    && (next == '=' || next == '~') {
                        break;
                    }
                sb.push(rn);
                self.advance();
            } else {
                break;
            }
        }

        self.prev_text = true;
        Token { kind: TokenKind::Text, pos, val: sb }
    }

    fn text_esc(&mut self, sb: &mut String) -> bool {
        let Some(r) = self.peek() else { return false };
        self.handle_text_esc(r, sb)
    }

    fn handle_text_esc(&mut self, r: char, sb: &mut String) -> bool {
        match r {
            ',' | ':' | '=' | '<' | '>' | '+' | '~' | '"' | '\\' | '.' | '*' => {
                self.advance();
                sb.push(r);
                true
            }
            'u' => self.handle_hex(r, sb),
            '0'..='7' => {
                // Octal escapes
                let d0 = r as u32 - '0' as u32;
                if r <= '3' && self.remaining() >= 3 {
                    let n1 = self.input[self.cursor + 1];
                    let n2 = self.input[self.cursor + 2];
                    if is_octal_digit(n1) && is_octal_digit(n2) {
                        self.advance(); // skip r
                        self.advance(); // skip n1
                        self.advance(); // skip n2
                        let val = (d0 << 6)
                            + ((n1 as u32 - '0' as u32) << 3)
                            + (n2 as u32 - '0' as u32);
                        if let Some(c) = char::from_u32(val) {
                            sb.push(c);
                        } else {
                            sb.push(char::REPLACEMENT_CHARACTER);
                        }
                        return true;
                    }
                }
                if self.remaining() >= 2 {
                    let n1 = self.input[self.cursor + 1];
                    if is_octal_digit(n1) {
                        self.advance(); // skip r
                        self.advance(); // skip n1
                        let val = (d0 << 3) + (n1 as u32 - '0' as u32);
                        if let Some(c) = char::from_u32(val) {
                            sb.push(c);
                        }
                        return true;
                    }
                }
                self.advance(); // skip r
                if let Some(c) = char::from_u32(d0) {
                    sb.push(c);
                }
                true
            }
            'x' => self.handle_hex(r, sb),
            _ => false,
        }
    }

    fn handle_hex(&mut self, starter: char, sb: &mut String) -> bool {
        let len = if starter == 'u' { 4 } else { 2 };

        if self.remaining() < len + 1 {
            return false;
        }

        // Check all hex digits (they start at cursor+1)
        for i in 1..=len {
            if !is_hex_digit(self.input[self.cursor + i]) {
                return false;
            }
        }

        // Skip the starter ('u' or 'x')
        self.advance();

        let mut val: u32 = 0;
        for _ in 0..len {
            let r1 = self.input[self.cursor];
            self.advance();
            val <<= 4;
            if ('a'..='f').contains(&r1) {
                val += 10 + r1 as u32 - 'a' as u32;
            } else if ('A'..='F').contains(&r1) {
                val += 10 + r1 as u32 - 'A' as u32;
            } else {
                val += r1 as u32 - '0' as u32;
            }
        }

        if starter == 'u' {
            if let Some(c) = char::from_u32(val) {
                sb.push(c);
            } else {
                sb.push(char::REPLACEMENT_CHARACTER);
            }
        } else {
            // \xHH: treat as the Unicode scalar value with that code point.
            // All values 0x00..=0xFF are valid Unicode scalar values.
            if let Some(c) = char::from_u32(val) {
                sb.push(c);
            } else {
                sb.push(char::REPLACEMENT_CHARACTER);
            }
        }
        true
    }

    fn collect_string(&mut self) -> Token {
        let start_pos = self.next_pos;
        let mut sb = String::new();
        while let Some(r) = self.peek() {
            self.advance();
            match r {
                '"' => return Token { kind: TokenKind::StringLit, pos: start_pos, val: sb },
                '\\' => {
                    let Some(rn) = self.peek() else {
                        return Token { kind: TokenKind::InvalidInput, pos: start_pos, val: String::new() };
                    };
                    if !self.handle_text_esc(rn, &mut sb) {
                        // String-only single-char escape sequences not handled by handle_text_esc
                        match rn {
                            'a' => { self.advance(); sb.push('\x07'); }
                            'b' => { self.advance(); sb.push('\x08'); }
                            'f' => { self.advance(); sb.push('\x0c'); }
                            'n' => { self.advance(); sb.push('\n'); }
                            'r' => { self.advance(); sb.push('\r'); }
                            't' => { self.advance(); sb.push('\t'); }
                            'v' => { self.advance(); sb.push('\x0b'); }
                            // Unrecognised escape: keep the backslash
                            _ => sb.push('\\'),
                        }
                    }
                }
                _ => sb.push(r),
            }
        }
        // Unterminated string
        Token { kind: TokenKind::InvalidInput, pos: start_pos, val: String::new() }
    }
}

fn is_white(r: char) -> bool {
    matches!(r, ' ' | '\t' | '\x0c' | '\u{00a0}' | '\r' | '\n')
}

fn is_digit(r: char) -> bool {
    ('0'..='9').contains(&r)
}

fn is_octal_digit(r: char) -> bool {
    ('0'..='7').contains(&r)
}

fn is_hex_digit(r: char) -> bool {
    is_digit(r) || ('a'..='f').contains(&r) || ('A'..='F').contains(&r)
}

fn is_text_start(r: char) -> bool {
    matches!(r,
        '#'..='\'' | '*' | '/' | ';' | '?' | '@' | 'A'..='Z' | '[' | ']' | '^'..='}' | '\\'
    ) || is_a1_or_higher(r)
}

fn is_a1_or_higher(r: char) -> bool {
    ('\u{00a1}'..='\u{0effff}').contains(&r)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_tokens() {
        let mut lex = Lexer::new("x = 42");
        let t1 = lex.next_token();
        assert_eq!(t1.kind, TokenKind::Text);
        assert_eq!(t1.val, "x");

        let t2 = lex.next_token();
        assert_eq!(t2.kind, TokenKind::Wsc);

        let t3 = lex.next_token();
        assert_eq!(t3.kind, TokenKind::Equals);

        let t4 = lex.next_token();
        assert_eq!(t4.kind, TokenKind::Wsc);

        let t5 = lex.next_token();
        assert_eq!(t5.kind, TokenKind::Text);
        assert_eq!(t5.val, "42");

        let t6 = lex.next_token();
        assert_eq!(t6.kind, TokenKind::Eof);
    }

    #[test]
    fn test_keywords() {
        let mut lex = Lexer::new("AND OR NOT");
        let t = lex.next_token();
        assert_eq!(t.kind, TokenKind::And);
        lex.next_token(); // wsc
        let t = lex.next_token();
        assert_eq!(t.kind, TokenKind::Or);
        lex.next_token(); // wsc
        let t = lex.next_token();
        assert_eq!(t.kind, TokenKind::Not);
    }

    #[test]
    fn test_string() {
        let mut lex = Lexer::new(r#""hello world""#);
        let t = lex.next_token();
        assert_eq!(t.kind, TokenKind::StringLit);
        assert_eq!(t.val, "hello world");
    }

    #[test]
    fn test_comparison_ops() {
        let mut lex = Lexer::new("< <= > >= != =~ !~");
        assert_eq!(lex.next_token().kind, TokenKind::LessThan);
        lex.next_token(); // wsc
        assert_eq!(lex.next_token().kind, TokenKind::LessThanEquals);
        lex.next_token();
        assert_eq!(lex.next_token().kind, TokenKind::GreaterThan);
        lex.next_token();
        assert_eq!(lex.next_token().kind, TokenKind::GreaterThanEquals);
        lex.next_token();
        assert_eq!(lex.next_token().kind, TokenKind::NotEquals);
        lex.next_token();
        assert_eq!(lex.next_token().kind, TokenKind::MatchesRegexp);
        lex.next_token();
        assert_eq!(lex.next_token().kind, TokenKind::NotMatchesRegexp);
    }

    #[test]
    fn test_comment() {
        let mut lex = Lexer::new("x -- this is a comment\n= 42");
        let t = lex.next_token();
        assert_eq!(t.kind, TokenKind::Text);
        assert_eq!(t.val, "x");
        let t = lex.next_token();
        assert_eq!(t.kind, TokenKind::Wsc);
        let t = lex.next_token();
        assert_eq!(t.kind, TokenKind::Equals);
    }

    #[test]
    fn test_push_token() {
        let mut lex = Lexer::new("a b");
        let t = lex.next_token();
        assert_eq!(t.val, "a");
        lex.push_token(t.clone());
        let t2 = lex.next_token();
        assert_eq!(t2.val, "a");
    }
}
