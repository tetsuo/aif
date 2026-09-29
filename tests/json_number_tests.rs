use aif::{Filterable, Value, parse};
use serde_json::{Number, Value as Json};

#[test]
fn large_json_numbers_are_rejected_or_preserved_without_panicking() {
    for text in ["1e400", "-1e400", "1e9999"] {
        let number = match serde_json::from_str::<Json>(text) {
            Ok(number) => number,
            Err(error) => {
                assert!(error.to_string().contains("number out of range"));
                continue;
            }
        };
        let value = Value::from(&number);
        assert!(matches!(value, Value::JsonNumber(_)));
        assert!(!value.is_zero());
        assert!(value.matches_global(&number.to_string()));

        let record = serde_json::json!({"n": number, "msg": "match"});
        for operator in ["=", "!=", "<", ">", "<=", ">=", ":"] {
            let expr = parse(&format!("n {operator} 0")).unwrap().unwrap();
            assert!(
                !expr.compile().unwrap().evaluate(&record),
                "{text} {operator} 0"
            );
        }
        for filter in ["n:*", "msg=match"] {
            assert!(
                parse(filter)
                    .unwrap()
                    .unwrap()
                    .compile()
                    .unwrap()
                    .evaluate(&record)
            );
        }
        let array = serde_json::json!({"n": [number]});
        assert!(
            !parse("n:0")
                .unwrap()
                .unwrap()
                .compile()
                .unwrap()
                .evaluate(&array)
        );
    }
}

#[test]
fn borrowed_native_json_numbers_keep_numeric_behavior() {
    struct Record<'a>(&'a Number);
    impl Filterable for Record<'_> {
        fn field(&self, name: &str) -> Option<Value<'_>> {
            (name == "n").then_some(Value::JsonNumber(self.0))
        }
    }
    for number in [
        Number::from(0),
        Number::from(5),
        Number::from_f64(5.5).unwrap(),
    ] {
        let record = Record(&number);
        let expr = parse(&format!("n = {number}")).unwrap().unwrap();
        assert!(expr.compile().unwrap().evaluate(&record));
        assert_eq!(
            Value::JsonNumber(&number).is_zero(),
            number.as_i64() == Some(0)
        );
        assert!(Value::JsonNumber(&number).matches_global(&number.to_string()));
    }
}
