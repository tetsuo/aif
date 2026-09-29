use crate::ast::{BinaryOp, CompareOp, Expr};
use crate::eval::{Filterable, Literal, Value, eval_literal};
use regex::{Regex, RegexBuilder};
use std::borrow::Cow;

/// A validated filter with reusable regexes, literals, and field paths.
///
/// ```
/// use aip_filter::parse;
/// use serde_json::json;
///
/// let expr = parse(r#"state =~ "^op" AND priority < 6"#).unwrap().unwrap();
/// let filter = expr.compile().unwrap();
/// assert!(filter.evaluate(&json!({"state": "open", "priority": 5})));
/// assert!(!filter.evaluate(&json!({"state": "closed", "priority": 5})));
/// ```
#[derive(Debug)]
pub struct CompiledFilter<'a> {
    root: Node<'a>,
}

#[derive(Debug)]
enum Node<'a> {
    Binary(BinaryOp, Box<Node<'a>>, Box<Node<'a>>),
    Not(Box<Node<'a>>),
    Global {
        text: Cow<'a, str>,
        field: Option<Source<'a>>,
        boolean_only: bool,
    },
    Comparison(Source<'a>, Predicate<'a>),
}

#[derive(Debug)]
enum Source<'a> {
    Field(Vec<&'a str>),
    String(&'a str),
}

#[derive(Debug)]
enum Predicate<'a> {
    Binary(BinaryOp, Box<Predicate<'a>>, Box<Predicate<'a>>),
    Not(Box<Predicate<'a>>),
    Literal(CompareOp, Literal<'a>),
    Regex { regex: Regex, negated: bool },
}

impl Expr {
    /// Validates the entire expression and prepares it for repeated evaluation.
    /// Unsupported functions and invalid comparison operands return an error.
    pub fn compile(&self) -> Result<CompiledFilter<'_>, String> {
        self.check_limits()?;
        Ok(CompiledFilter {
            root: Node::compile(self, &mut 32)?,
        })
    }
}

impl CompiledFilter<'_> {
    pub fn evaluate<T: Filterable>(&self, record: &T) -> bool {
        self.root.evaluate(record)
    }
}

impl<'a> Node<'a> {
    fn compile(expr: &'a Expr, regex_budget: &mut usize) -> Result<Self, String> {
        Ok(match expr {
            Expr::Binary {
                op, left, right, ..
            } => Self::Binary(
                *op,
                Box::new(Self::compile(left, regex_budget)?),
                Box::new(Self::compile(right, regex_budget)?),
            ),
            Expr::Unary { expr, .. } => Self::Not(Box::new(Self::compile(expr, regex_budget)?)),
            Expr::Comparison {
                op, left, right, ..
            } => Self::Comparison(
                Source::compile(left)?,
                Predicate::compile(*op, right, regex_budget)?,
            ),
            Expr::Name {
                name, is_string, ..
            } => Self::Global {
                text: Cow::Borrowed(name),
                field: (!is_string).then(|| Source::Field(vec![name])),
                boolean_only: true,
            },
            Expr::Member { .. } => Self::Global {
                text: literal_text(expr)?,
                field: Some(Source::compile(expr)?),
                boolean_only: false,
            },
            Expr::Function { .. } => {
                return Err("function calls are not supported during evaluation".into());
            }
        })
    }

    fn evaluate<T: Filterable>(&self, record: &T) -> bool {
        match self {
            Self::Binary(BinaryOp::And, left, right) => {
                left.evaluate(record) && right.evaluate(record)
            }
            Self::Binary(BinaryOp::Or, left, right) => {
                left.evaluate(record) || right.evaluate(record)
            }
            Self::Not(expr) => !expr.evaluate(record),
            Self::Global {
                text,
                field,
                boolean_only,
            } => {
                if let Some(value) = field.as_ref().and_then(|field| field.resolve(record)) {
                    if let Value::Bool(value) = value {
                        return value;
                    }
                    if !boolean_only {
                        return !value.is_zero();
                    }
                }
                record.matches_global(text)
            }
            Self::Comparison(source, predicate) => source
                .resolve(record)
                .is_some_and(|value| predicate.evaluate(&value)),
        }
    }
}

impl<'a> Source<'a> {
    fn compile(expr: &'a Expr) -> Result<Self, String> {
        match expr {
            Expr::Name {
                name,
                is_string: true,
                ..
            } => Ok(Self::String(name)),
            Expr::Name { name, .. } => Ok(Self::Field(vec![name])),
            Expr::Member { .. } => Ok(Self::Field(field_path(expr)?)),
            Expr::Function { .. } => {
                Err("function calls are not supported during evaluation".into())
            }
            _ => Err("left operand must be a field or string literal".into()),
        }
    }

    fn resolve<'s, T: Filterable>(&'s self, record: &'s T) -> Option<Value<'s>> {
        let path = match self {
            Self::String(text) => return Some(Value::String(text)),
            Self::Field(path) => path,
        };
        if path.len() == 1 {
            return record.field(path[0]);
        }
        if let Some(value) = record.field_path(path) {
            return Some(value);
        }
        let mut value = record.field(path[0])?;
        for segment in &path[1..] {
            value = match value {
                Value::Map(entries) => {
                    entries
                        .into_iter()
                        .find(|(key, _)| key.eq_ignore_ascii_case(segment))?
                        .1
                }
                Value::JsonObject(entries) => entries
                    .iter()
                    .find(|(key, _)| key.eq_ignore_ascii_case(segment))
                    .map(|(_, value)| Value::from(value))?,
                _ => return None,
            };
        }
        Some(value)
    }
}

impl<'a> Predicate<'a> {
    fn compile(op: CompareOp, expr: &'a Expr, regex_budget: &mut usize) -> Result<Self, String> {
        Ok(match expr {
            Expr::Binary {
                op: binary,
                left,
                right,
                ..
            } => Self::Binary(
                *binary,
                Box::new(Self::compile(op, left, regex_budget)?),
                Box::new(Self::compile(op, right, regex_budget)?),
            ),
            Expr::Unary { expr, .. } => Self::Not(Box::new(Self::compile(op, expr, regex_budget)?)),
            Expr::Function { .. } => {
                return Err("function calls are not supported during evaluation".into());
            }
            _ if matches!(op, CompareOp::MatchesRegexp | CompareOp::NotMatchesRegexp) => {
                let Expr::Name {
                    name,
                    is_string: true,
                    ..
                } = expr
                else {
                    return Err("regular expression is not a quoted string".into());
                };
                *regex_budget = regex_budget
                    .checked_sub(1)
                    .ok_or("filter exceeds limit of 32 regular expressions")?;
                Self::Regex {
                    regex: compile_regex(name)
                        .map_err(|error| format!("invalid regular expression: {error}"))?,
                    negated: op == CompareOp::NotMatchesRegexp,
                }
            }
            _ => Self::Literal(op, Literal::new(literal_text(expr)?, op)),
        })
    }

    fn evaluate(&self, value: &Value<'_>) -> bool {
        match self {
            Self::Binary(BinaryOp::And, left, right) => {
                left.evaluate(value) && right.evaluate(value)
            }
            Self::Binary(BinaryOp::Or, left, right) => {
                left.evaluate(value) || right.evaluate(value)
            }
            Self::Not(expr) => !expr.evaluate(value),
            Self::Literal(op, literal) => eval_literal(*op, value, literal),
            Self::Regex { regex, negated } => value
                .as_str()
                .is_some_and(|text| regex.is_match(text) != *negated),
        }
    }
}

fn field_path(expr: &Expr) -> Result<Vec<&str>, String> {
    let mut path = Vec::new();
    let mut current = expr;
    loop {
        match current {
            Expr::Member { holder, member, .. } => {
                path.push(member.as_str());
                current = holder;
            }
            Expr::Name { name, .. } => {
                path.push(name.as_str());
                path.reverse();
                return Ok(path);
            }
            Expr::Function { .. } => {
                return Err("function calls are not supported during evaluation".into());
            }
            _ => return Err("invalid field path".into()),
        }
    }
}

fn literal_text(expr: &Expr) -> Result<Cow<'_, str>, String> {
    match expr {
        Expr::Name { name, .. } => Ok(Cow::Borrowed(name)),
        Expr::Member { .. } => Ok(Cow::Owned(field_path(expr)?.join("."))),
        Expr::Function { .. } => Err("function calls are not supported during evaluation".into()),
        _ => Err("right operand must be a literal or a combination of literals".into()),
    }
}

pub(crate) fn compile_regex(pattern: &str) -> Result<Regex, regex::Error> {
    RegexBuilder::new(pattern)
        .size_limit(1024 * 1024)
        .dfa_size_limit(256 * 1024)
        .build()
}
