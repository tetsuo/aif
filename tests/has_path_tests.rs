use ele::{Filterable, Value, parse};
use serde_json::json;

fn matches(record: &impl Filterable, text: &str) -> bool {
    parse(text)
        .unwrap()
        .unwrap()
        .compile()
        .unwrap()
        .evaluate(record)
}

#[test]
fn map_presence_does_not_depend_on_the_value() {
    for value in [
        json!(0),
        json!(false),
        json!(""),
        json!(null),
        json!([]),
        json!({}),
    ] {
        let record = json!({"m": {"foo": value}});
        assert!(matches(&record, "m:foo"));
        assert!(matches(&record, "m.foo:*"), "{record}");
        assert!(matches(&record, "m.foo:(* OR absent)"));
        assert!(!matches(&record, "m.foo:(NOT *)"));
        assert!(!matches(&record, "m.missing:*"));
        assert!(!matches(&record, "m.missing:(NOT *)"));
    }
    assert!(!matches(&json!({"m": {}}), "m:*"));
    assert!(!matches(&json!({"r": []}), "r:*"));
    assert!(!matches(&json!({"n": 0}), "n:*"));
    assert!(matches(&json!({"m": {"foo": 0}}), "m.foo:(0 AND *)"));
    assert!(matches(&json!({"m": {"FOO": 1, "foo": 0}}), "m.foo:0"));
}

#[test]
fn has_traverses_repeated_values_without_array_indices() {
    let record = json!({"r": [{"foo": 42}, {"foo": 43}, {"other": 0}, null]});
    assert!(matches(&record, "r.foo:42"));
    assert!(matches(&record, "r.foo:43"));
    assert!(matches(&record, "r.foo:*"));
    assert!(matches(&record, "r.foo:42 AND r.foo:43"));
    assert!(!matches(&record, "r.foo:(42 AND 43)"));
    assert!(!matches(&record, "r.foo:99"));
    assert!(!matches(&record, "r.0.foo:42"));
    assert!(!matches(&record, "r.foo = 42"));
    assert!(!matches(&record, "r.foo != 42"));
    assert!(!matches(&record, "r.foo > 0"));
    assert!(!matches(&json!({"r": []}), "r.foo:*"));
    assert!(!matches(&json!({"r": [{"other": 0}]}), "r.foo:(NOT *)"));
    assert!(matches(&json!({"r": [{"foo": 0}]}), "r.foo:*"));
    assert!(matches(
        &json!({"r": [{"foo": [{"bar": 42}]}]}),
        "r.foo.bar:42"
    ));
}

#[test]
fn owned_collections_follow_the_same_has_rules() {
    struct Record;
    impl Filterable for Record {
        fn field(&self, name: &str) -> Option<Value<'_>> {
            match name {
                "m" => Some(Value::Map(vec![("foo", Value::Bool(false))])),
                "r" => Some(Value::List(vec![Value::Map(vec![("foo", Value::Int(42))])])),
                _ => None,
            }
        }
    }
    assert!(matches(&Record, "m.foo:*"));
    assert!(matches(&Record, "r.foo:42"));
    assert!(!matches(&Record, "r.foo = 42"));
}

#[test]
fn custom_message_paths_keep_non_default_presence_rules() {
    struct Message(i64);
    impl Filterable for Message {
        fn field(&self, _: &str) -> Option<Value<'_>> {
            None
        }
        fn field_path(&self, path: &[&str]) -> Option<Value<'_>> {
            match path {
                ["inner", "value"] => Some(Value::Int(self.0)),
                ["inner", "map"] => Some(Value::Map(vec![("value", Value::Int(self.0))])),
                ["inner", "map", "value"] => Some(Value::Int(self.0)),
                _ => None,
            }
        }
    }
    assert!(!matches(&Message(0), "inner.value:*"));
    assert!(matches(&Message(42), "inner.value:*"));
    assert!(matches(&Message(42), "inner.value:42"));
    assert!(matches(&Message(0), "inner.map.value:*"));
    assert!(matches(&Message(0), "inner.map.value:0"));
}
