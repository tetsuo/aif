# aip-filter

Parses and evaluates [AIP-160](https://google.aip.dev/160) filter expressions.

## Install

Clone the repository and install it with Cargo:

```bash
cargo install --path .
```

## CLI usage

```bash
usage: aip-filter [-p|--print] <expr> [file...]
```

* `--print` prints the parsed expression instead of evaluating it.
* If no files are given, reads from stdin.
* Input is newline-delimited JSON, with one value per line. Matching lines are preserved.
* An empty filter copies the selected input unchanged; `--print` with an empty filter prints nothing.
* Invalid JSON lines are reported and skipped. Any invalid JSON or input/output error causes a nonzero exit status.

### Matching rules

* Bare literals search values recursively, including nested objects and arrays, but not object keys. String searches are case-insensitive substrings.
* An unquoted boolean field name evaluates that field. A dotted name resolves a field path first and otherwise becomes a global literal. Quote a search term to prevent field lookup, for example `'"active"'` or `'"example.com"'`.
* String `=` and `!=` ignore ASCII case and support one leading or trailing `*`. The two operators are opposites.
* String `:` matches words or phrases bounded by whitespace or ASCII punctuation, or a leading/trailing wildcard. Lists match any element; maps match keys. `field:*` tests for a non-default value, including a nonempty list or map.
* Functions can be parsed and printed, but evaluation rejects the entire filter if any function is present, including inside a negation or unused branch.
* The CLI compiles filters before reading records. Regexes, numeric literals, and field paths are reused across records. Filters can contain at most 32 regexes; each regex has a 1 MiB compiled-size limit and a 256 KiB DFA cache limit.
* Filters are limited to 64 KiB, 512 non-whitespace tokens, 64 recursive parser calls, and an expression tree of at most 128 levels and 512 nodes. Excessive filters return errors instead of recursing without a bound.

### Examples

```bash
# Match if price is between 10 and 100, and status is active
echo '{"price": 93, "status": "active"}' | \
  aip-filter "price > 10 AND price < 100 AND status = active"

# Match if 'foobar' appears in any field (bare-literal match)
echo '{"id": 1, "msg": "hello foobar"}' | aip-filter "foobar"

# Match if 'tags' contains 'beta'
echo '{"tags": ["stable", "beta"]}' | aip-filter "tags:beta"

# Nested field access
echo '{"user": {"settings": {"theme": "dark"}}}' | \
  aip-filter "user.settings.theme = dark"
```

## API

Implement `Filterable` for your type, compile the expression once with `Expr::compile`, and reuse `CompiledFilter::evaluate`. Compilation returns errors for unsupported expressions before any records are read. `serde_json::Value` implements `Filterable` directly and borrows arrays and objects without copying them.

`Expr::evaluate` is a convenience method for one record: it compiles on every call and returns false if compilation fails. Use `Expr::compile` when errors must be reported.

Bare-literal matching also requires overriding `all_field_values` or `matches_global`; the default searches no values. Override `matches_global` to search borrowed data without building a list of values. `Value::matches_global` provides recursive matching for individual values.

```rust
use aip_filter::{parse, Value, Filterable};

struct Issue {
    state: String,
    priority: i64,
    labels: Vec<String>,
}

impl Filterable for Issue {
    fn field(&self, name: &str) -> Option<Value<'_>> {
        match name {
            "state"    => Some(Value::String(&self.state)),
            "priority" => Some(Value::Int(self.priority)),
            "labels"   => Some(Value::List(
                self.labels.iter().map(|l| Value::String(l)).collect()
            )),
            _ => None,
        }
    }
}

fn main() {
    let expr = parse("state = \"open\"").unwrap().unwrap();
    let filter = expr.compile().unwrap();
    let issue = Issue { state: "open".into(), priority: 1, labels: vec![] };

    if filter.evaluate(&issue) {
        println!("got match");
    } else {
        println!("no match");
    }

    assert!(filter.evaluate(&issue));
}
```
