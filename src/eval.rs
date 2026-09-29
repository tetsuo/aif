use super::ast::{CompareOp, Expr};
use serde_json::Value as Json;
use std::{borrow::Cow, cmp::Ordering};

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
    /// Borrowed JSON number, including values outside the native numeric range.
    JsonNumber(&'a serde_json::Number),
    /// Borrowed string reference.
    String(&'a str),
    /// Owned string (for computed values).
    StringOwned(std::string::String),
    /// A list of values (for repeated fields / slices).
    List(Vec<Value<'a>>),
    /// A map of string keys to values.
    Map(Vec<(&'a str, Value<'a>)>),
    /// Borrowed JSON array; its elements are converted only when inspected.
    JsonArray(&'a [Json]),
    /// Borrowed JSON object; its values are converted only when inspected.
    JsonObject(&'a serde_json::Map<String, Json>),
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

    /// Matches a global literal against this value and any nested values.
    pub fn matches_global(&self, search: &str) -> bool {
        value_contains_str(self, search)
    }

    /// Returns true if this value is the zero/default for its type.
    pub fn is_zero(&self) -> bool {
        match self {
            Value::Bool(b) => !b,
            Value::Int(i) => *i == 0,
            Value::Uint(u) => *u == 0,
            Value::Float(f) => *f == 0.0,
            Value::JsonNumber(number) => native_number(number).is_some_and(|value| value.is_zero()),
            Value::String(s) => s.is_empty(),
            Value::StringOwned(s) => s.is_empty(),
            Value::List(v) => v.is_empty(),
            Value::Map(m) => m.is_empty(),
            Value::JsonArray(items) => items.is_empty(),
            Value::JsonObject(entries) => entries.is_empty(),
            Value::Null => true,
        }
    }

    fn compare_number(&self, other: &Value<'_>) -> Option<Ordering> {
        match (self, other) {
            (Value::Int(a), Value::Int(b)) => Some(a.cmp(b)),
            (Value::Uint(a), Value::Uint(b)) => Some(a.cmp(b)),
            (Value::Float(a), Value::Float(b)) => a.partial_cmp(b),
            // Cross-numeric comparisons
            (Value::Int(a), Value::Float(b)) => compare_integer_float(i128::from(*a), *b),
            (Value::Float(a), Value::Int(b)) => {
                compare_integer_float(i128::from(*b), *a).map(Ordering::reverse)
            }
            (Value::Uint(a), Value::Float(b)) => compare_integer_float(i128::from(*a), *b),
            (Value::Float(a), Value::Uint(b)) => {
                compare_integer_float(i128::from(*b), *a).map(Ordering::reverse)
            }
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

// The integer is an i64 or u64. Comparing its exact value avoids f64 rounding above 2^53.
fn compare_integer_float(integer: i128, float: f64) -> Option<Ordering> {
    if float.is_nan() {
        return None;
    }
    if float < i64::MIN as f64 {
        return Some(Ordering::Greater);
    }
    if float >= u64::MAX as f64 {
        return Some(Ordering::Less);
    }
    match integer.cmp(&(float as i128)) {
        Ordering::Equal => 0.0_f64.partial_cmp(&float.fract()),
        order => Some(order),
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

    /// Matches a bare literal across field values. Override to search without collecting values.
    fn matches_global(&self, search: &str) -> bool {
        self.all_field_values()
            .iter()
            .any(|value| value.matches_global(search))
    }
}

fn native_number(number: &serde_json::Number) -> Option<Value<'static>> {
    if let Some(value) = number.as_i64() {
        Some(Value::Int(value))
    } else if let Some(value) = number.as_u64() {
        Some(Value::Uint(value))
    } else {
        number
            .as_f64()
            .filter(|value| value.is_finite())
            .map(Value::Float)
    }
}

impl<'a> From<&'a Json> for Value<'a> {
    fn from(value: &'a Json) -> Self {
        match value {
            Json::Null => Self::Null,
            Json::Bool(value) => Self::Bool(*value),
            Json::String(value) => Self::String(value),
            Json::Number(value) => native_number(value).unwrap_or(Self::JsonNumber(value)),
            Json::Array(values) => Self::JsonArray(values),
            Json::Object(values) => Self::JsonObject(values),
        }
    }
}

impl Filterable for Json {
    fn field(&self, name: &str) -> Option<Value<'_>> {
        self.get(name).map(Value::from)
    }

    fn field_path(&self, path: &[&str]) -> Option<Value<'_>> {
        let mut current = self;
        for segment in path {
            current = current.get(*segment)?;
        }
        Some(Value::from(current))
    }

    fn all_field_values(&self) -> Vec<Value<'_>> {
        match self {
            Json::Object(values) => values.values().map(Value::from).collect(),
            _ => vec![Value::from(self)],
        }
    }

    fn matches_global(&self, search: &str) -> bool {
        Value::from(self).matches_global(search)
    }
}

impl Expr {
    /// Evaluates once, returning false if compilation fails, including under negation.
    /// Use `compile()` to report errors and reuse the filter across records.
    pub fn evaluate<T: Filterable>(&self, value: &T) -> bool {
        self.compile().is_ok_and(|filter| filter.evaluate(value))
    }
}

#[derive(Debug)]
pub(crate) struct Literal<'a> {
    text: Cow<'a, str>,
    folded: Option<String>,
    number: Option<Value<'static>>,
    boolean: Option<bool>,
    word_prefix: Vec<usize>,
}

impl<'a> Literal<'a> {
    pub(crate) fn new(text: Cow<'a, str>, op: CompareOp) -> Self {
        let number = if let Ok(value) = text.parse::<i64>() {
            Some(Value::Int(value))
        } else if let Ok(value) = text.parse::<u64>() {
            Some(Value::Uint(value))
        } else {
            text.parse::<f64>().ok().map(Value::Float)
        };
        let boolean = if text.eq_ignore_ascii_case("true") || text == "1" {
            Some(true)
        } else if text.eq_ignore_ascii_case("false") || text == "0" {
            Some(false)
        } else {
            None
        };
        let folded = match lowercase(&text) {
            Cow::Borrowed(_) => None,
            Cow::Owned(text) => Some(text),
        };
        let word_prefix = if op == CompareOp::Has && !text.starts_with('*') && !text.ends_with('*')
        {
            prefix_lengths(folded.as_deref().unwrap_or(&text).as_bytes())
        } else {
            Vec::new()
        };
        Self {
            text,
            folded,
            number,
            boolean,
            word_prefix,
        }
    }

    pub(crate) fn is_presence(&self) -> bool {
        self.text == "*"
    }

    fn folded(&self) -> &str {
        self.folded.as_deref().unwrap_or(&self.text)
    }
}

pub(crate) fn eval_literal(op: CompareOp, value: &Value<'_>, literal: &Literal<'_>) -> bool {
    if op == CompareOp::Has && literal.text == "*" {
        return !value.is_zero();
    }
    if matches!(
        op,
        CompareOp::Equals | CompareOp::NotEquals | CompareOp::Has
    ) {
        let element_op = if op == CompareOp::NotEquals {
            CompareOp::Equals
        } else {
            op
        };
        let found = match value {
            Value::List(items) => Some(
                items
                    .iter()
                    .any(|item| value_matches_literal(item, literal, element_op)),
            ),
            Value::JsonArray(items) => Some(
                items
                    .iter()
                    .any(|item| value_matches_literal(&Value::from(item), literal, element_op)),
            ),
            _ => None,
        };
        if let Some(found) = found {
            return found != (op == CompareOp::NotEquals);
        }
    }
    if op == CompareOp::Has {
        match value {
            Value::Map(entries) => {
                return entries
                    .iter()
                    .any(|(key, _)| key.eq_ignore_ascii_case(&literal.text));
            }
            Value::JsonObject(entries) => {
                return entries
                    .keys()
                    .any(|key| key.eq_ignore_ascii_case(&literal.text));
            }
            _ => {}
        }
    }
    compare_literal(op, value, literal)
}

fn value_matches_literal(value: &Value<'_>, literal: &Literal<'_>, op: CompareOp) -> bool {
    match value {
        Value::List(items) => items
            .iter()
            .any(|item| value_matches_literal(item, literal, op)),
        Value::Map(entries) => entries
            .iter()
            .any(|(_, value)| value_matches_literal(value, literal, op)),
        Value::JsonArray(items) => items
            .iter()
            .any(|value| value_matches_literal(&Value::from(value), literal, op)),
        Value::JsonObject(entries) => entries
            .values()
            .any(|value| value_matches_literal(&Value::from(value), literal, op)),
        _ => compare_literal(op, value, literal),
    }
}

fn compare_literal(op: CompareOp, value: &Value<'_>, literal: &Literal<'_>) -> bool {
    if let Value::JsonNumber(number) = value {
        return native_number(number).is_some_and(|value| compare_literal(op, &value, literal));
    }
    if let Some(text) = value.as_str() {
        return string_match(text, literal, op);
    }
    let order = match value {
        Value::Bool(value) => literal.boolean.map(|right| value.cmp(&right)),
        Value::Int(_) | Value::Uint(_) | Value::Float(_) => literal
            .number
            .as_ref()
            .and_then(|right| value.compare_number(right)),
        _ => None,
    };
    compare_order(op, order)
}

/// Equality and `:` support a leading or trailing wildcard.
/// Word boundaries for `:` are whitespace or ASCII punctuation.
fn string_match(haystack: &str, literal: &Literal<'_>, op: CompareOp) -> bool {
    if op == CompareOp::NotEquals {
        return !string_match(haystack, literal, CompareOp::Equals);
    }
    let needle = literal.text.as_ref();
    if matches!(op, CompareOp::Equals | CompareOp::Has) {
        if let Some(suffix) = needle.strip_prefix('*') {
            return haystack
                .len()
                .checked_sub(suffix.len())
                .and_then(|start| haystack.get(start..))
                .is_some_and(|tail| tail.eq_ignore_ascii_case(suffix));
        }
        if let Some(prefix) = needle.strip_suffix('*') {
            return haystack
                .get(..prefix.len())
                .is_some_and(|head| head.eq_ignore_ascii_case(prefix));
        }
    }
    match op {
        CompareOp::Equals => haystack.eq_ignore_ascii_case(needle),
        CompareOp::Has => {
            let haystack = lowercase(haystack);
            let needle = literal.folded();
            if needle.is_empty() {
                return true;
            }
            let boundary =
                |c: char| c.is_whitespace() || (c.is_ascii() && !c.is_ascii_alphanumeric());
            let bytes = needle.as_bytes();
            let mut matched = 0;
            for (index, byte) in haystack.bytes().enumerate() {
                while matched > 0 && byte != bytes[matched] {
                    matched = literal.word_prefix[matched - 1];
                }
                if byte == bytes[matched] {
                    matched += 1;
                }
                if matched == bytes.len() {
                    // A complete UTF-8 needle matches only at character boundaries.
                    let end = index + 1;
                    let start = end - bytes.len();
                    if haystack[..start].chars().next_back().is_none_or(boundary)
                        && haystack[end..].chars().next().is_none_or(boundary)
                    {
                        return true;
                    }
                    matched = literal.word_prefix[matched - 1];
                }
            }
            false
        }
        _ => compare_order(op, Some(lowercase(haystack).as_ref().cmp(literal.folded()))),
    }
}

// Prefix lengths let the KMP search reuse overlapping matches in linear time.
fn prefix_lengths(needle: &[u8]) -> Vec<usize> {
    let mut prefix = vec![0; needle.len()];
    for index in 1..needle.len() {
        let mut matched = prefix[index - 1];
        while matched > 0 && needle[index] != needle[matched] {
            matched = prefix[matched - 1];
        }
        if needle[index] == needle[matched] {
            matched += 1;
        }
        prefix[index] = matched;
    }
    prefix
}

fn lowercase(s: &str) -> Cow<'_, str> {
    if s.is_ascii() && !s.bytes().any(|b| b.is_ascii_uppercase()) {
        Cow::Borrowed(s)
    } else {
        Cow::Owned(s.to_lowercase())
    }
}

fn contains_ignore_case(haystack: &str, needle: &str) -> bool {
    lowercase(haystack).contains(lowercase(needle).as_ref())
}

/// Check if a value contains a string (for global restriction matching).
fn value_contains_str(val: &Value<'_>, search: &str) -> bool {
    match val {
        Value::String(s) => contains_ignore_case(s, search),
        Value::StringOwned(s) => contains_ignore_case(s, search),
        Value::Int(i) => i.to_string() == search,
        Value::Uint(u) => u.to_string() == search,
        Value::Float(f) => f.to_string() == search,
        Value::JsonNumber(number) => match native_number(number) {
            Some(value) => value.matches_global(search),
            None => number.to_string() == search,
        },
        Value::Bool(b) => {
            let bs = if *b { "true" } else { "false" };
            bs.eq_ignore_ascii_case(search)
        }
        Value::List(items) => items.iter().any(|item| value_contains_str(item, search)),
        Value::Map(entries) => entries.iter().any(|(_, v)| value_contains_str(v, search)),
        Value::JsonArray(items) => items
            .iter()
            .any(|value| Value::from(value).matches_global(search)),
        Value::JsonObject(entries) => entries
            .values()
            .any(|value| Value::from(value).matches_global(search)),
        Value::Null => false,
    }
}

fn compare_order(op: CompareOp, order: Option<Ordering>) -> bool {
    match op {
        CompareOp::Equals | CompareOp::Has => order == Some(Ordering::Equal),
        CompareOp::NotEquals => order != Some(Ordering::Equal),
        CompareOp::LessThan => order == Some(Ordering::Less),
        CompareOp::LessThanEquals => matches!(order, Some(Ordering::Less | Ordering::Equal)),
        CompareOp::GreaterThan => order == Some(Ordering::Greater),
        CompareOp::GreaterThanEquals => matches!(order, Some(Ordering::Greater | Ordering::Equal)),
        CompareOp::MatchesRegexp | CompareOp::NotMatchesRegexp => false,
    }
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
                    self.labels
                        .iter()
                        .map(|l| Value::String(l.as_str()))
                        .collect(),
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
        let expr = parse(r#"state = "open" AND priority > 3"#)
            .unwrap()
            .unwrap();
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
                    "inner" => Some(Value::Map(vec![("value", Value::Int(self.inner.value))])),
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
