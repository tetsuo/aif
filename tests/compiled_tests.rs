use aif::{Expr, Filterable, Value, parse};
use serde_json::json;
use std::process::{Command, Stdio};

#[test]
fn unsupported_functions_reject_the_entire_filter() {
    let record = json!({"active": true, "value": "open"});
    for input in [
        "fn(x)",
        "NOT fn(x)",
        "active OR fn(x)",
        "-fn(x)",
        "fn(x) = open",
        "value = fn(x)",
        "value:(open OR fn(x))",
    ] {
        let expr = parse(input).unwrap().unwrap();
        assert!(
            expr.compile()
                .unwrap_err()
                .contains("function calls are not supported")
        );
        assert!(!expr.evaluate(&record), "{input}");
        let output = Command::new(env!("CARGO_BIN_EXE_aif"))
            .arg(input)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1), "{input}");
        assert!(output.stdout.is_empty());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("function calls are not supported")
        );
    }
    let output = Command::new(env!("CARGO_BIN_EXE_aif"))
        .args(["--print", "fn(x)"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("call"));
}

#[test]
fn compiled_filters_are_reusable_with_different_record_types() {
    let expr = parse(r#"state =~ ("^op" OR "^cl") AND priority < 5.9"#)
        .unwrap()
        .unwrap();
    let filter = expr.compile().unwrap();
    for _ in 0..100 {
        assert!(filter.evaluate(&json!({"state":"open", "priority":5})));
        assert!(filter.evaluate(&json!({"state":"closed", "priority":1})));
        assert!(!filter.evaluate(&json!({"state":"shut", "priority":1})));
        assert!(!filter.evaluate(&json!({"state":"open", "priority":6})));
        assert!(!filter.evaluate(&json!({"priority":1})));
    }

    struct Record;
    impl Filterable for Record {
        fn field(&self, name: &str) -> Option<Value<'_>> {
            match name {
                "state" => Some(Value::String("closed")),
                "priority" => Some(Value::Int(5)),
                _ => None,
            }
        }
    }
    assert!(filter.evaluate(&Record));
}

#[test]
fn borrowed_and_owned_collections_have_identical_semantics() {
    struct Record<'a>(Value<'a>);
    impl Filterable for Record<'_> {
        fn field(&self, name: &str) -> Option<Value<'_>> {
            (name == "value").then(|| self.0.clone())
        }
    }
    let array = json!(["open", 5, false, {"key": "nested"}]);
    let owned = Value::List(vec![
        Value::StringOwned("open".into()),
        Value::Int(5),
        Value::Bool(false),
        Value::Map(vec![("key", Value::String("nested"))]),
    ]);
    assert!(matches!(Value::from(&array), Value::JsonArray(_)));
    for input in [
        "value:*",
        "value:open",
        "value:5.0",
        "value:false",
        "value:garbage",
        "value:nested",
        "value = open",
        "value != open",
        "value != absent",
    ] {
        let expr = parse(input).unwrap().unwrap();
        let filter = expr.compile().unwrap();
        assert_eq!(
            filter.evaluate(&Record(Value::from(&array))),
            filter.evaluate(&Record(owned.clone())),
            "{input}"
        );
    }
    let object = json!({"key":"nested"});
    assert!(matches!(Value::from(&object), Value::JsonObject(_)));
    for input in ["value:key", "value:nested", "value:*", "value.KEY=nested"] {
        let expr = parse(input).unwrap().unwrap();
        let filter = expr.compile().unwrap();
        assert_eq!(
            filter.evaluate(&Record(Value::from(&object))),
            filter.evaluate(&Record(Value::Map(vec![("key", Value::String("nested"))]))),
            "{input}"
        );
    }
}

#[test]
fn regex_resources_and_manual_trees_are_bounded() {
    let input = std::iter::repeat_n(r#"value =~ "x""#, 33)
        .collect::<Vec<_>>()
        .join(" OR ");
    let expr = parse(&input).unwrap().unwrap();
    assert!(
        expr.compile()
            .unwrap_err()
            .contains("32 regular expressions")
    );
    assert!(parse(r#"value =~ "a{1000000}""#).is_err());

    let mut expr = parse("value").unwrap().unwrap();
    for _ in 0..150 {
        expr = Expr::Unary {
            op: aif::UnaryOp::Not,
            expr: Box::new(expr),
            pos: aif::Position { line: 1, col: 1 },
        };
    }
    assert!(expr.compile().unwrap_err().contains("expression limit"));
}

#[test]
fn phrase_matching_agrees_with_a_simple_reference() {
    struct Text<'a>(&'a str);
    impl Filterable for Text<'_> {
        fn field(&self, name: &str) -> Option<Value<'_>> {
            (name == "value").then_some(Value::String(self.0))
        }
    }
    fn strings(depth: usize) -> Vec<String> {
        let mut result = vec![String::new()];
        let mut level = vec![String::new()];
        for _ in 0..depth {
            level = level
                .iter()
                .flat_map(|prefix| {
                    ["a", "é", "İ", " ", "!"].map(|suffix| format!("{prefix}{suffix}"))
                })
                .collect();
            result.extend(level.iter().cloned());
        }
        result
    }
    let haystacks = strings(4);
    for needle in strings(3) {
        let expr = parse(&format!(
            "value:{}",
            serde_json::to_string(&needle).unwrap()
        ))
        .unwrap()
        .unwrap();
        let filter = expr.compile().unwrap();
        let folded_needle = needle.to_lowercase();
        for haystack in &haystacks {
            let folded = haystack.to_lowercase();
            let boundary =
                |c: char| c.is_whitespace() || (c.is_ascii() && !c.is_ascii_alphanumeric());
            let expected = folded_needle.is_empty()
                || folded.char_indices().any(|(start, _)| {
                    folded[start..].starts_with(&folded_needle)
                        && folded[..start].chars().next_back().is_none_or(boundary)
                        && folded[start + folded_needle.len()..]
                            .chars()
                            .next()
                            .is_none_or(boundary)
                });
            assert_eq!(
                filter.evaluate(&Text(haystack)),
                expected,
                "{haystack:?} : {needle:?}"
            );
        }
    }
}

#[test]
fn long_overlapping_phrases_are_supported() {
    let needle = "aa ".repeat(1000) + "a";
    let expr = parse(&format!("value:\"{needle}\"")).unwrap().unwrap();
    let filter = expr.compile().unwrap();
    let haystack = "aa ".repeat(100_000);
    assert!(!filter.evaluate(&json!({"value": haystack})));
    assert!(filter.evaluate(&json!({"value": haystack + "a"})));
}

#[test]
fn invalid_comparison_operands_report_errors() {
    let expr = parse("value:(other=1)").unwrap().unwrap();
    assert!(expr.compile().is_err());
    assert!(!expr.evaluate(&json!({"value":"other"})));
}
