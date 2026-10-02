//! Parses and evaluates [AIP-160](https://google.aip.dev/160) filter expressions.
//!
//! ## Example
//!
//! ```rust
//! use ele::{parse, Value, Filterable};
//!
//! let expr = parse("state = \"open\" AND priority > 3").unwrap().unwrap();
//! println!("{}", expr);
//! ```
//!
//! ## Evaluating filters
//!
//! Implement [`Filterable`] for your type, then call [`Expr::evaluate`]:
//!
//! ```rust
//! use ele::{parse, Value, Filterable};
//!
//! struct Issue {
//!     state: String,
//!     priority: i64,
//!     labels: Vec<String>,
//! }
//!
//! impl Filterable for Issue {
//!     fn field(&self, name: &str) -> Option<Value<'_>> {
//!         match name {
//!             "state"    => Some(Value::String(&self.state)),
//!             "priority" => Some(Value::Int(self.priority)),
//!             "labels"   => Some(Value::List(
//!                 self.labels.iter().map(|l| Value::String(l)).collect()
//!             )),
//!             _ => None,
//!         }
//!     }
//! }
//!
//! let expr = parse("state = \"open\"").unwrap().unwrap();
//! let issue = Issue { state: "open".into(), priority: 1, labels: vec![] };
//! assert!(expr.evaluate(&issue));
//! ```

pub mod ast;
mod compiled;
pub mod display;
pub mod eval;
pub mod lex;
pub mod parser;

pub use ast::{BinaryOp, CompareOp, Expr, Position, UnaryOp};
pub use compiled::CompiledFilter;
pub use eval::{Filterable, Value};

/// Parse a filter expression string into an [`Expr`] AST.
///
/// Returns `Ok(None)` for empty/whitespace-only input.
/// Returns `Ok(Some(expr))` on success.
/// Returns `Err(message)` on parse errors.
pub fn parse(input: &str) -> Result<Option<Expr>, String> {
    parser::parse_filter(input)
}
