/// Position in the filter string (line and column, 1-indexed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Position {
    pub line: usize,
    pub col: usize,
}

impl std::fmt::Display for Position {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.line, self.col)
    }
}

/// Binary operators: AND / OR.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    And,
    Or,
}

/// Unary operators: negation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Minus,
    Not,
}

/// Comparison operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompareOp {
    Equals,
    NotEquals,
    LessThan,
    LessThanEquals,
    GreaterThan,
    GreaterThanEquals,
    Has,
    MatchesRegexp,
    NotMatchesRegexp,
}

impl CompareOp {
    pub fn as_str(&self) -> &'static str {
        match self {
            CompareOp::Equals => "=",
            CompareOp::NotEquals => "!=",
            CompareOp::LessThan => "<",
            CompareOp::LessThanEquals => "<=",
            CompareOp::GreaterThan => ">",
            CompareOp::GreaterThanEquals => ">=",
            CompareOp::Has => ":",
            CompareOp::MatchesRegexp => "=~",
            CompareOp::NotMatchesRegexp => "!~",
        }
    }
}

/// The AST of a parsed AIP-160 filter expression.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    /// Binary expression: `left AND right` or `left OR right`.
    Binary {
        op: BinaryOp,
        left: Box<Expr>,
        right: Box<Expr>,
        pos: Position,
    },

    /// Unary expression: `-expr` or `NOT expr`.
    Unary {
        op: UnaryOp,
        expr: Box<Expr>,
        pos: Position,
    },

    /// Comparison expression: `left op right`.
    Comparison {
        op: CompareOp,
        left: Box<Expr>,
        right: Box<Expr>,
        pos: Position,
    },

    /// A simple name or literal value.
    Name {
        name: String,
        pos: Position,
        /// `true` if this was a quoted string literal.
        is_string: bool,
        /// `true` if inside a parenthesized (composite) expression.
        is_composite: bool,
    },

    /// A member access expression: `holder.member`.
    Member {
        holder: Box<Expr>,
        member: String,
        pos: Position,
    },

    /// A function call expression: `fn(args...)`.
    Function {
        func: Box<Expr>,
        args: Vec<Expr>,
    },
}
