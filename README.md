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

Implement `Filterable` for your type, then call `Expr::evaluate`:

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
    let issue = Issue { state: "open".into(), priority: 1, labels: vec![] };

    if expr.evaluate(&issue) {
        println!("got match");
    } else {
        println!("no match");
    }

    assert!(expr.evaluate(&issue));
}
```
