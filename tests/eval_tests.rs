use ele::{Filterable, Value, parse};

struct Record<'a> {
    value: Value<'a>,
}

impl Filterable for Record<'_> {
    fn field(&self, name: &str) -> Option<Value<'_>> {
        (name == "value").then(|| self.value.clone())
    }

    fn all_field_values(&self) -> Vec<Value<'_>> {
        vec![self.value.clone()]
    }
}

fn matches(value: Value<'_>, filter: &str) -> bool {
    parse(filter).unwrap().unwrap().evaluate(&Record { value })
}

#[test]
fn global_matching_for_library_records() {
    let value = Value::Map(vec![
        ("ignored_key", Value::Null),
        (
            "events",
            Value::List(vec![Value::StringOwned("FOObar".into())]),
        ),
    ]);
    assert!(matches(value.clone(), "foobar"));
    assert!(!matches(value.clone(), "ignored_key"));
    assert!(value.matches_global("foobar"));
    assert!(Value::String("ÉTUDE").matches_global("étude"));
    assert!(Value::String("").matches_global(""));
    assert!(!Value::Null.matches_global("null"));
}

#[test]
fn global_matching_can_avoid_collecting_values() {
    struct Direct;
    impl Filterable for Direct {
        fn field(&self, _: &str) -> Option<Value<'_>> {
            None
        }

        fn all_field_values(&self) -> Vec<Value<'_>> {
            panic!("global matching must use the override");
        }

        fn matches_global(&self, search: &str) -> bool {
            Value::String("hello foobar").matches_global(search)
        }
    }
    assert!(parse("foobar").unwrap().unwrap().evaluate(&Direct));
}

#[test]
fn integer_float_boundaries() {
    for (integer, float, order) in [
        (Value::Int(i64::MIN), i64::MIN as f64, "="),
        (Value::Int(i64::MAX), i64::MAX as f64, "<"),
        (Value::Uint(u64::MAX), u64::MAX as f64, "<"),
        (Value::Int(0), -0.0, "="),
        (Value::Uint(0), -0.5, ">"),
        (Value::Int(-5), -5.9, ">"),
        (Value::Int(5), 5.9, "<"),
        (Value::Int(1), f64::INFINITY, "<"),
        (Value::Uint(1), f64::NEG_INFINITY, ">"),
    ] {
        assert!(matches(integer, &format!("value {order} \"{float:?}\"")));
    }
    assert!(!matches(Value::Int(0), "value = NaN"));
    assert!(!matches(Value::Float(f64::NAN), "value > 0"));
    assert!(matches(Value::Float(f64::INFINITY), "value = inf"));
    assert!(matches(Value::Float(f64::NEG_INFINITY), "value < -1"));
}

#[test]
fn composite_comparisons_and_negation() {
    for (filter, expected) in [
        ("value:(open OR closed)", true),
        ("value:(open AND closed)", false),
        ("value:(NOT closed)", true),
        (r#"value =~ ("^op" OR "^cl")"#, true),
        (r#"value !~ ("^cl" AND "^sh")"#, true),
        (r#"value != "op*""#, false),
    ] {
        assert_eq!(
            matches(Value::StringOwned("open".into()), filter),
            expected,
            "{filter}"
        );
    }
}
