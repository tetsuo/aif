# aif

Parses and evaluates an [AIP-160](https://google.aip.dev/160)-style filter language for JSON and Rust values.

## Install

Install [Rust](https://rustup.rs/), then run this from a repository checkout:

```bash
rustup update stable
cargo +stable install --path . --locked
```

## CLI usage

The CLI reads one JSON value per line from files or standard input and prints matching records without reformatting.

```text
aif [-p|--print] [--max-record-bytes N] [--] <expr> [file...]
```

```bash
echo '{"price":93,"status":"active"}' | aif 'price > 10 AND status = active'

aif '"timeout"' events.jsonl
aif 'tags:beta' events.jsonl
aif 'user.settings.theme = dark' events.jsonl
```

* `--print` prints the parsed expression instead of filtering input.
* `--help` and `--version` print information and exit. `--` ends option parsing.
* Records are limited to 8 MiB, including the newline. `--max-record-bytes N` changes this limit.
* An empty filter copies input unchanged without applying the record limit.

Invalid or oversized records are reported and skipped. Exit codes are `0` for success, including no matches; `1` for filter, data, or I/O errors; and `2` for invalid arguments.

### Matching rules

* Bare literals search nested values, not object keys. String searches are case-insensitive substrings. Quote a search term to prevent field lookup.
* String `=` and `!=` ignore ASCII case and support a leading or trailing `*` wildcard.
* Regex matching uses `=~` and `!~`.
* String `:` matches whole words or phrases. On lists, it matches any element; on maps, it matches keys.
* `field:*` tests for a non-default value. `map.key:*` tests whether the key exists, even when its value is zero, false, empty, or null.
* `r.foo:42` searches the `foo` fields of repeated elements. Ordinary comparisons do not traverse arrays, and numeric path segments are not array indices.

Function calls are not supported during evaluation.

## Rust API

Compile an expression once and reuse the filter. `serde_json::Value` is supported directly; implement `Filterable` for other types.

```rust
use aif::parse;
use serde_json::json;

let expr = parse("state = open AND priority > 3").unwrap().unwrap();
let filter = expr.compile().unwrap();
let record = json!({"state": "open", "priority": 5});

assert!(filter.evaluate(&record));
```

For a custom type, expose its fields through `Filterable`:

```rust
use aif::{Filterable, Value, parse};

struct Issue {
    state: String,
    priority: i64,
}

impl Filterable for Issue {
    fn field(&self, name: &str) -> Option<Value<'_>> {
        match name {
            "state" => Some(Value::String(&self.state)),
            "priority" => Some(Value::Int(self.priority)),
            _ => None,
        }
    }
}

let expr = parse("state = open AND priority > 3").unwrap().unwrap();
let filter = expr.compile().unwrap();
let issue = Issue { state: "open".into(), priority: 5 };

assert!(filter.evaluate(&issue));
```

## License

Licensed under the [MIT License](LICENSE).
