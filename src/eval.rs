use super::ast::{BinaryOp, CompareOp, Expr, UnaryOp};
use regex::Regex;
use std::cmp::Ordering;

/// A runtime value used during filter evaluation.
#[derive(Debug, Clone)]
pub enum Value<'a> {
    /// Boolean value.
    Bool(bool),
    /// Signed integer (covers i8..i64).
    Int(i64),
    /// Unsigned integer (covers u8..u64).
    Uint(u64),
    /// Floating-point number.
    Float(f64),
    /// Borrowed string reference.
    String(&'a str),
    /// Owned string (for computed values).
    StringOwned(std::string::String),
    /// A list of values (for repeated fields / slices).
    List(Vec<Value<'a>>),
    /// A map of string keys to values.
    Map(Vec<(&'a str, Value<'a>)>),
    /// Absent value.
    Null,
}

impl<'a> Value<'a> {
    /// Returns the string content if this is a string variant.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s),
            Value::StringOwned(s) => Some(s.as_str()),
            _ => None,
        }
    }

    /// Returns true if this value is the zero/default for its type.
    pub fn is_zero(&self) -> bool {
        match self {
            Value::Bool(b) => !b,
            Value::Int(i) => *i == 0,
            Value::Uint(u) => *u == 0,
            Value::Float(f) => *f == 0.0,
            Value::String(s) => s.is_empty(),
            Value::StringOwned(s) => s.is_empty(),
            Value::List(v) => v.is_empty(),
            Value::Map(m) => m.is_empty(),
            Value::Null => true,
        }
    }

    /// Try to compare two values, returning an ordering if possible.
    fn compare(&self, other: &Value<'_>) -> Option<Ordering> {
        match (self, other) {
            (Value::Bool(a), Value::Bool(b)) => Some(a.cmp(b)),
            (Value::Int(a), Value::Int(b)) => Some(a.cmp(b)),
            (Value::Uint(a), Value::Uint(b)) => Some(a.cmp(b)),
            (Value::Float(a), Value::Float(b)) => a.partial_cmp(b),
            (Value::String(a), Value::String(b)) => Some(a.to_lowercase().cmp(&b.to_lowercase())),
            (Value::StringOwned(a), Value::String(b)) => {
                Some(a.to_lowercase().cmp(&b.to_lowercase()))
            }
            (Value::String(a), Value::StringOwned(b)) => {
                Some(a.to_lowercase().cmp(&b.to_lowercase()))
            }
            (Value::StringOwned(a), Value::StringOwned(b)) => {
                Some(a.to_lowercase().cmp(&b.to_lowercase()))
            }
            // Cross-numeric comparisons
            (Value::Int(a), Value::Float(b)) => (*a as f64).partial_cmp(b),
            (Value::Float(a), Value::Int(b)) => a.partial_cmp(&(*b as f64)),
            (Value::Uint(a), Value::Float(b)) => (*a as f64).partial_cmp(b),
            (Value::Float(a), Value::Uint(b)) => a.partial_cmp(&(*b as f64)),
            (Value::Int(a), Value::Uint(b)) => {
                if *a < 0 {
                    Some(Ordering::Less)
                } else {
                    (*a as u64).partial_cmp(b)
                }
            }
            (Value::Uint(a), Value::Int(b)) => {
                if *b < 0 {
                    Some(Ordering::Greater)
                } else {
                    a.partial_cmp(&(*b as u64))
                }
            }
            _ => None,
        }
    }
}

/// Trait that types must implement to be filterable.
///
/// # Example
///
/// ```rust
/// use aip_filter::{Value, Filterable};
///
/// struct Issue {
///     state: String,
///     priority: i64,
/// }
///
/// impl Filterable for Issue {
///     fn field(&self, name: &str) -> Option<Value<'_>> {
///         match name {
///             "state" => Some(Value::String(&self.state)),
///             "priority" => Some(Value::Int(self.priority)),
///             _ => None,
///         }
///     }
/// }
/// ```
pub trait Filterable {
    /// Look up a field by name, returning its value.
    ///
    /// The name matching should ideally support both snake_case and camelCase
    /// forms, though this is up to the implementor.
    ///
    /// Return `None` if the field doesn't exist.
    fn field(&self, name: &str) -> Option<Value<'_>>;

    /// Look up a nested field path like `["a", "b"]` for `a.b`.
    ///
    /// Default implementation walks through `field()` calls for structs.
    /// Override this for custom nested access.
    fn field_path(&self, path: &[&str]) -> Option<Value<'_>> {
        if path.is_empty() {
            return None;
        }
        if path.len() == 1 {
            return self.field(path[0]);
        }
        // Default: can't traverse deeper without knowing the sub-type
        None
    }

    /// Return all field values for "global restriction" matching
    /// (when a bare value appears without a field name).
    ///
    /// Default returns an empty vec (no global matching).
    fn all_field_values(&self) -> Vec<Value<'_>> {
        Vec::new()
    }
}

impl Expr {
    /// Evaluate this expression against a filterable value.
    ///
    /// Returns `true` if the value matches the filter.
    pub fn evaluate<T: Filterable>(&self, val: &T) -> bool {
        match self {
            Expr::Binary { op, left, right, .. } => match op {
                BinaryOp::And => left.evaluate(val) && right.evaluate(val),
                BinaryOp::Or => left.evaluate(val) || right.evaluate(val),
            },

            Expr::Unary { op, expr, .. } => {
                let result = expr.evaluate(val);
                match op {
                    UnaryOp::Minus | UnaryOp::Not => !result,
                }
            }

            Expr::Comparison {
                op, left, right, ..
            } => eval_comparison(*op, left, right, val),

            Expr::Name {
                name, is_string, ..
            } => {
                // A bare name/value: global restriction matching.
                // Check if any field contains or equals the value.
                if let Some(field_val) = val.field(name) {
                    // If it's a boolean field referenced by name, return its value
                    if !*is_string
                        && let Value::Bool(b) = field_val {
                            return b;
                        }
                    return !field_val.is_zero();
                }

                // Try matching against all fields
                let search = name.as_str();
                for field_val in val.all_field_values() {
                    if value_contains_str(&field_val, search) {
                        return true;
                    }
                }
                false
            }

            Expr::Member {
                holder, member, ..
            } => {
                // Member as a standalone expression; expect boolean
                if let Some(val) = resolve_member_value(holder, member, val) {
                    match val {
                        Value::Bool(b) => b,
                        _ => !val.is_zero(),
                    }
                } else {
                    false
                }
            }

            Expr::Function { .. } => {
                // Function evaluation is not yet supported
                false
            }
        }
    }
}

/// Evaluate a comparison expression.
fn eval_comparison<T: Filterable>(
    op: CompareOp,
    left: &Expr,
    right: &Expr,
    val: &T,
) -> bool {
    // Resolve the left side to a Value
    let left_val = resolve_expr_value(left, val);

    let Some(left_val) = left_val else {
        return false;
    };

    eval_comparison_with_value(op, &left_val, right, val)
}

fn eval_comparison_with_value<T: Filterable>(
    op: CompareOp,
    left_val: &Value<'_>,
    right: &Expr,
    val: &T,
) -> bool {
    // Handle the right side, it could be a composite expression (AND/OR)
    match right {
        Expr::Binary {
            op: bin_op,
            left: rleft,
            right: rright,
            ..
        } => {
            let l = eval_comparison_with_value(op, left_val, rleft, val);
            let r = eval_comparison_with_value(op, left_val, rright, val);
            match bin_op {
                BinaryOp::And => l && r,
                BinaryOp::Or => l || r,
            }
        }
        Expr::Unary {
            op: UnaryOp::Minus | UnaryOp::Not,
            expr,
            ..
        } => !eval_comparison_with_value(op, left_val, expr, val),

        _ => {
            let right_str = expr_to_literal_str(right);
            let right_str = right_str.as_deref();

            match (&left_val, op) {
                // Has operator on lists
                (Value::List(items), CompareOp::Has) => {
                    if let Some(rs) = right_str {
                        items.iter().any(|item| value_matches_str(item, rs, op))
                    } else {
                        false
                    }
                }

                // Has operator on maps; check if key exists
                (Value::Map(entries), CompareOp::Has) => {
                    if let Some(rs) = right_str {
                        entries.iter().any(|(k, _)| k.eq_ignore_ascii_case(rs))
                    } else {
                        false
                    }
                }

                // Equals/NotEquals on lists; check if any element matches
                (Value::List(items), CompareOp::Equals) => {
                    if let Some(rs) = right_str {
                        items.iter().any(|item| value_matches_str(item, rs, op))
                    } else {
                        false
                    }
                }
                (Value::List(items), CompareOp::NotEquals) => {
                    if let Some(rs) = right_str {
                        !items.iter().any(|item| value_matches_str(item, rs, CompareOp::Equals))
                    } else {
                        true
                    }
                }

                // Has on strings; substring/word boundary match
                (_, CompareOp::Has) => {
                    if let Some(rs) = right_str {
                        // Wildcard handling
                        if rs == "*" {
                            return !left_val.is_zero();
                        }
                        value_matches_str(left_val, rs, op)
                    } else {
                        false
                    }
                }

                // Regex match
                (_, CompareOp::MatchesRegexp | CompareOp::NotMatchesRegexp) => {
                    let Some(left_s) = left_val.as_str() else {
                        return false;
                    };
                    let Some(rs) = right_str else {
                        return false;
                    };
                    let Ok(re) = Regex::new(rs) else {
                        return false;
                    };
                    let matched = re.is_match(left_s);
                    if op == CompareOp::NotMatchesRegexp {
                        !matched
                    } else {
                        matched
                    }
                }

                // Standard comparison
                _ => {
                    if let Some(rs) = right_str {
                        let right_val = parse_literal_for_value(left_val, rs);
                        compare_values(op, left_val, &right_val)
                    } else {
                        false
                    }
                }
            }
        }
    }
}

/// Resolve an expression to its value given a filterable object.
fn resolve_expr_value<'a, T: Filterable>(expr: &Expr, val: &'a T) -> Option<Value<'a>> {
    match expr {
        Expr::Name {
            name, is_string, ..
        } => {
            if *is_string {
                // A quoted string on the left side of a comparison is just a string literal
                Some(Value::StringOwned(name.clone()))
            } else {
                val.field(name)
            }
        }
        Expr::Member {
            holder, member, ..
        } => resolve_member_value(holder, member, val),
        _ => None,
    }
}

/// Resolve a member expression (e.g., `a.b.c`) to a value.
fn resolve_member_value<'a, T: Filterable>(
    holder: &Expr,
    member: &str,
    val: &'a T,
) -> Option<Value<'a>> {
    // Collect the path segments
    let mut path = vec![member];
    let mut cur = holder;
    loop {
        match cur {
            Expr::Member {
                holder,
                member,
                ..
            } => {
                path.push(member.as_str());
                cur = holder;
            }
            Expr::Name { name, .. } => {
                path.push(name.as_str());
                break;
            }
            _ => return None,
        }
    }
    path.reverse();

    // Try field_path first
    if let Some(v) = val.field_path(&path) {
        return Some(v);
    }

    // Fall back to walking field by field (for nested Filterable values)
    // First field
    let first_val = val.field(path[0])?;

    // Walk remaining path through map keys
    let mut current = first_val;
    for &segment in &path[1..] {
        current = match current {
            Value::Map(entries) => {
                let found = entries
                    .into_iter()
                    .find(|(k, _)| k.eq_ignore_ascii_case(segment));
                match found {
                    Some((_, v)) => v,
                    None => return None,
                }
            }
            _ => return None,
        };
    }
    Some(current)
}

/// Extract the literal string from an expression (name or member chain as dotted string).
fn expr_to_literal_str(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Name { name, .. } => Some(name.clone()),
        Expr::Member { holder, member, .. } => {
            // a.b on the right side is just "a.b" as a string
            let mut parts = vec![member.as_str()];
            let mut cur = holder.as_ref();
            loop {
                match cur {
                    Expr::Member { holder, member, .. } => {
                        parts.push(member.as_str());
                        cur = holder.as_ref();
                    }
                    Expr::Name { name, .. } => {
                        parts.push(name.as_str());
                        break;
                    }
                    _ => return None,
                }
            }
            parts.reverse();
            Some(parts.join("."))
        }
        Expr::Function { .. } => None,
        _ => None,
    }
}

/// Parse a literal string to match the type of a reference value.
fn parse_literal_for_value<'a>(reference: &Value<'_>, literal: &'a str) -> Value<'a> {
    match reference {
        Value::Bool(_) => match literal.to_lowercase().as_str() {
            "true" | "1" => Value::Bool(true),
            "false" | "0" => Value::Bool(false),
            _ => Value::String(literal),
        },
        Value::Int(_) => {
            if let Ok(i) = literal.parse::<i64>() {
                Value::Int(i)
            } else if let Ok(f) = literal.parse::<f64>() {
                Value::Int(f as i64)
            } else {
                Value::String(literal)
            }
        }
        Value::Uint(_) => {
            if let Ok(u) = literal.parse::<u64>() {
                Value::Uint(u)
            } else {
                Value::String(literal)
            }
        }
        Value::Float(_) => {
            if let Ok(f) = literal.parse::<f64>() {
                Value::Float(f)
            } else {
                Value::String(literal)
            }
        }
        Value::String(_) | Value::StringOwned(_) => Value::String(literal),
        _ => Value::String(literal),
    }
}

/// Check if a value matches a string using the given comparison op.
fn value_matches_str(val: &Value<'_>, s: &str, op: CompareOp) -> bool {
    match val {
        Value::String(vs) => string_match(vs, s, op),
        Value::StringOwned(vs) => string_match(vs.as_str(), s, op),
        Value::Int(i) => {
            if let Ok(ri) = s.parse::<i64>() {
                match op {
                    CompareOp::Equals | CompareOp::Has => *i == ri,
                    _ => false,
                }
            } else {
                false
            }
        }
        Value::Uint(u) => {
            if let Ok(ru) = s.parse::<u64>() {
                match op {
                    CompareOp::Equals | CompareOp::Has => *u == ru,
                    _ => false,
                }
            } else {
                false
            }
        }
        Value::Float(f) => {
            if let Ok(rf) = s.parse::<f64>() {
                match op {
                    CompareOp::Equals | CompareOp::Has => (*f - rf).abs() < f64::EPSILON,
                    _ => false,
                }
            } else {
                false
            }
        }
        Value::Bool(b) => {
            let sb = matches!(s.to_lowercase().as_str(), "true" | "1");
            match op {
                CompareOp::Equals | CompareOp::Has => *b == sb,
                _ => false,
            }
        }
        Value::List(items) => items.iter().any(|item| value_matches_str(item, s, op)),
        Value::Map(entries) => entries.iter().any(|(_, v)| value_matches_str(v, s, op)),
        Value::Null => false,
    }
}

/// String matching: case-insensitive equality for `=`, word-boundary match for `:`, wildcard support.
fn string_match(haystack: &str, needle: &str, op: CompareOp) -> bool {
    match op {
        CompareOp::Has => {
            // Wildcard: * prefix
            if let Some(suffix) = needle.strip_prefix('*') {
                return haystack.len() >= suffix.len()
                    && haystack[haystack.len() - suffix.len()..].eq_ignore_ascii_case(suffix);
            }
            // Wildcard: * suffix
            if let Some(prefix) = needle.strip_suffix('*') {
                return haystack.len() >= prefix.len()
                    && haystack[..prefix.len()].eq_ignore_ascii_case(prefix);
            }

            // Word boundary match
            let hl = haystack.to_lowercase();
            let nl = needle.to_lowercase();
            if let Some(idx) = hl.find(&nl) {
                let is_boundary = |b: u8| -> bool {
                    b < 0x80 && !b.is_ascii_alphanumeric()
                };
                let start_ok = idx == 0 || is_boundary(haystack.as_bytes()[idx - 1]);
                let end_ok = idx + nl.len() >= haystack.len()
                    || is_boundary(haystack.as_bytes()[idx + nl.len()]);
                if start_ok && end_ok {
                    return true;
                }
            }

            // Also check individual words
            let words = split_string(haystack);
            if words.len() > 1 {
                for word in &words {
                    if string_match(word, needle, CompareOp::Has) {
                        return true;
                    }
                }
            }

            false
        }
        CompareOp::Equals => {
            // Wildcard: * prefix
            if let Some(suffix) = needle.strip_prefix('*') {
                return haystack.len() >= suffix.len()
                    && haystack[haystack.len() - suffix.len()..].eq_ignore_ascii_case(suffix);
            }
            // Wildcard: * suffix
            if let Some(prefix) = needle.strip_suffix('*') {
                return haystack.len() >= prefix.len()
                    && haystack[..prefix.len()].eq_ignore_ascii_case(prefix);
            }
            haystack.eq_ignore_ascii_case(needle)
        }
        CompareOp::NotEquals => !haystack.eq_ignore_ascii_case(needle),
        _ => {
            // For ordering comparisons on strings, use case-sensitive compare
            let ord = haystack.cmp(needle);
            match op {
                CompareOp::LessThan => ord == Ordering::Less,
                CompareOp::LessThanEquals => ord != Ordering::Greater,
                CompareOp::GreaterThan => ord == Ordering::Greater,
                CompareOp::GreaterThanEquals => ord != Ordering::Less,
                _ => false,
            }
        }
    }
}

/// Check if a value contains a string (for global restriction matching).
fn value_contains_str(val: &Value<'_>, search: &str) -> bool {
    match val {
        Value::String(s) => s.to_lowercase().contains(&search.to_lowercase()),
        Value::StringOwned(s) => s.to_lowercase().contains(&search.to_lowercase()),
        Value::Int(i) => i.to_string() == search,
        Value::Uint(u) => u.to_string() == search,
        Value::Float(f) => f.to_string() == search,
        Value::Bool(b) => {
            let bs = if *b { "true" } else { "false" };
            bs == search.to_lowercase()
        }
        Value::List(items) => items.iter().any(|item| value_contains_str(item, search)),
        Value::Map(entries) => entries.iter().any(|(_, v)| value_contains_str(v, search)),
        Value::Null => false,
    }
}

/// Compare two values using a comparison operator.
fn compare_values(op: CompareOp, left: &Value<'_>, right: &Value<'_>) -> bool {
    match op {
        CompareOp::Equals | CompareOp::Has => {
            if let (Some(ls), Some(rs)) = (left.as_str(), right.as_str()) {
                return string_match(ls, rs, op);
            }
            left.compare(right) == Some(Ordering::Equal)
        }
        CompareOp::NotEquals => {
            if let (Some(ls), Some(rs)) = (left.as_str(), right.as_str()) {
                return !ls.eq_ignore_ascii_case(rs);
            }
            left.compare(right) != Some(Ordering::Equal)
        }
        CompareOp::LessThan => left.compare(right) == Some(Ordering::Less),
        CompareOp::LessThanEquals => matches!(
            left.compare(right),
            Some(Ordering::Less) | Some(Ordering::Equal)
        ),
        CompareOp::GreaterThan => left.compare(right) == Some(Ordering::Greater),
        CompareOp::GreaterThanEquals => matches!(
            left.compare(right),
            Some(Ordering::Greater) | Some(Ordering::Equal)
        ),
        CompareOp::MatchesRegexp | CompareOp::NotMatchesRegexp => {
            // Handled at a higher level
            false
        }
    }
}

/// Split a string into words.
fn split_string(s: &str) -> Vec<&str> {
    let mut result = Vec::new();
    let mut start = None;

    for (i, c) in s.char_indices() {
        if c.is_whitespace() {
            if let Some(s_idx) = start {
                result.push(&s[s_idx..i]);
                start = None;
            }
        } else if is_text_start_char(c) {
            if start.is_none() {
                start = Some(i);
            }
        } else {
            if let Some(s_idx) = start {
                result.push(&s[s_idx..i]);
                start = None;
            }
            // Single char becomes its own word
            let end = i + c.len_utf8();
            result.push(&s[i..end]);
        }
    }

    if let Some(s_idx) = start {
        result.push(&s[s_idx..]);
    }

    result
}

fn is_text_start_char(r: char) -> bool {
    matches!(r,
        '#'..='\'' | '*' | '/' | ';' | '?' | '@' | 'A'..='Z' | '[' | ']' | '^'..='}' | '\\'
    ) || ('\u{00a1}'..='\u{0effff}').contains(&r)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;

    struct TestItem {
        state: String,
        priority: i64,
        score: f64,
        active: bool,
        labels: Vec<String>,
        name: String,
    }

    impl Filterable for TestItem {
        fn field(&self, name: &str) -> Option<Value<'_>> {
            match name {
                "state" => Some(Value::String(&self.state)),
                "priority" => Some(Value::Int(self.priority)),
                "score" => Some(Value::Float(self.score)),
                "active" => Some(Value::Bool(self.active)),
                "labels" => Some(Value::List(
                    self.labels.iter().map(|l| Value::String(l.as_str())).collect(),
                )),
                "name" => Some(Value::String(&self.name)),
                _ => None,
            }
        }

        fn all_field_values(&self) -> Vec<Value<'_>> {
            vec![
                Value::String(&self.state),
                Value::Int(self.priority),
                Value::Float(self.score),
                Value::Bool(self.active),
                Value::String(&self.name),
            ]
        }
    }

    fn item() -> TestItem {
        TestItem {
            state: "open".into(),
            priority: 5,
            score: 9.5,
            active: true,
            labels: vec!["bug".into(), "urgent".into()],
            name: "Fix crash on startup".into(),
        }
    }

    #[test]
    fn test_equals() {
        let expr = parse(r#"state = "open""#).unwrap().unwrap();
        assert!(expr.evaluate(&item()));

        let expr = parse(r#"state = "closed""#).unwrap().unwrap();
        assert!(!expr.evaluate(&item()));
    }

    #[test]
    fn test_not_equals() {
        let expr = parse(r#"state != "closed""#).unwrap().unwrap();
        assert!(expr.evaluate(&item()));
    }

    #[test]
    fn test_numeric_comparison() {
        let expr = parse("priority > 3").unwrap().unwrap();
        assert!(expr.evaluate(&item()));

        let expr = parse("priority < 3").unwrap().unwrap();
        assert!(!expr.evaluate(&item()));

        let expr = parse("priority >= 5").unwrap().unwrap();
        assert!(expr.evaluate(&item()));

        let expr = parse("priority <= 5").unwrap().unwrap();
        assert!(expr.evaluate(&item()));
    }

    #[test]
    fn test_float_comparison() {
        let expr = parse("score > 9.0").unwrap().unwrap();
        assert!(expr.evaluate(&item()));
    }

    #[test]
    fn test_and() {
        let expr = parse(r#"state = "open" AND priority > 3"#).unwrap().unwrap();
        assert!(expr.evaluate(&item()));

        let expr = parse(r#"state = "closed" AND priority > 3"#)
            .unwrap()
            .unwrap();
        assert!(!expr.evaluate(&item()));
    }

    #[test]
    fn test_or() {
        let expr = parse(r#"state = "closed" OR priority > 3"#)
            .unwrap()
            .unwrap();
        assert!(expr.evaluate(&item()));
    }

    #[test]
    fn test_negation() {
        let expr = parse(r#"-state = "closed""#).unwrap().unwrap();
        assert!(expr.evaluate(&item()));

        let expr = parse(r#"NOT state = "open""#).unwrap().unwrap();
        assert!(!expr.evaluate(&item()));
    }

    #[test]
    fn test_has_on_list() {
        let expr = parse(r#"labels : "bug""#).unwrap().unwrap();
        assert!(expr.evaluate(&item()));

        let expr = parse(r#"labels : "feature""#).unwrap().unwrap();
        assert!(!expr.evaluate(&item()));
    }

    #[test]
    fn test_has_on_string() {
        let expr = parse(r#"name : "crash""#).unwrap().unwrap();
        assert!(expr.evaluate(&item()));

        let expr = parse(r#"name : "memory""#).unwrap().unwrap();
        assert!(!expr.evaluate(&item()));
    }

    #[test]
    fn test_boolean_field() {
        let expr = parse("active").unwrap().unwrap();
        assert!(expr.evaluate(&item()));
    }

    #[test]
    fn test_regex_match() {
        let expr = parse(r#"state =~ "^op""#).unwrap().unwrap();
        assert!(expr.evaluate(&item()));

        let expr = parse(r#"state !~ "^cl""#).unwrap().unwrap();
        assert!(expr.evaluate(&item()));
    }

    #[test]
    fn test_wildcard() {
        let expr = parse(r#"state = "op*""#).unwrap().unwrap();
        assert!(expr.evaluate(&item()));

        let expr = parse(r#"state = "*en""#).unwrap().unwrap();
        assert!(expr.evaluate(&item()));
    }

    #[test]
    fn test_case_insensitive() {
        let expr = parse(r#"state = "OPEN""#).unwrap().unwrap();
        assert!(expr.evaluate(&item()));
    }

    #[test]
    fn test_implicit_and() {
        let expr = parse(r#"state = "open" priority > 3"#).unwrap().unwrap();
        assert!(expr.evaluate(&item()));
    }

    #[test]
    fn test_parenthesized() {
        let expr = parse(r#"(state = "open" OR state = "closed") AND priority > 3"#)
            .unwrap()
            .unwrap();
        assert!(expr.evaluate(&item()));
    }

    #[test]
    fn test_has_wildcard_star() {
        let expr = parse("priority : *").unwrap().unwrap();
        assert!(expr.evaluate(&item()));
    }

    #[test]
    fn test_global_restriction() {
        let expr = parse("open").unwrap().unwrap();
        assert!(expr.evaluate(&item()));

        let expr = parse("nonexistent").unwrap().unwrap();
        assert!(!expr.evaluate(&item()));
    }

    #[test]
    fn test_nested_field() {
        struct Outer {
            inner: Inner,
        }
        struct Inner {
            value: i64,
        }

        impl Filterable for Outer {
            fn field(&self, name: &str) -> Option<Value<'_>> {
                match name {
                    "inner" => Some(Value::Map(vec![
                        ("value", Value::Int(self.inner.value)),
                    ])),
                    _ => None,
                }
            }

            fn field_path(&self, path: &[&str]) -> Option<Value<'_>> {
                match path {
                    ["inner", "value"] => Some(Value::Int(self.inner.value)),
                    _ => None,
                }
            }
        }

        let outer = Outer {
            inner: Inner { value: 42 },
        };

        let expr = parse("inner.value > 10").unwrap().unwrap();
        assert!(expr.evaluate(&outer));
    }
}
