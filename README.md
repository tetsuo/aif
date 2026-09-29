# aip-filter

Parses and evaluates [AIP-160](https://google.aip.dev/160) filter expressions.

## Install

Clone the repository and install it with Cargo:

```bash
cargo install --path .
```

## CLI usage

```bash
usage: aip-filter [-p|--print] [--max-record-bytes N] [--] <expr> [file...]
```

* `--print` prints the parsed expression instead of evaluating it.
* If no files are given, reads from stdin.
* Input is newline-delimited JSON, with one value per line. Matching lines are preserved, with a newline inserted between records when a preceding input file has no trailing newline. A final unterminated record is otherwise left unchanged.
* An empty filter copies the selected input unchanged; `--print` with an empty filter prints nothing.
* Invalid JSON lines are reported and skipped. Any invalid JSON or input/output error causes a nonzero exit status.
* Records are limited to 8 MiB by default, including the newline. `--max-record-bytes N` sets a positive byte limit. Oversized records are reported and skipped without buffering the rest of the line; later records are still processed and the exit status indicates failure. The limit does not apply to an empty filter, which copies bytes directly.
* Memory depends on the largest allowed record and its parsed JSON values, not on the total number of records. The record limit is not a process-memory limit.

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

## Performance

Release builds optimize for speed rather than minimum executable size. The evaluator reuses compiled regexes and literals, borrows JSON collections, and searches overlapping phrases in linear time. JSON parsing still allocates memory for each record.

To compare two release binaries, use Python 3:

```bash
python3 bench/benchmark.py ./target/baseline ./target/release/aip-filter \
  --records 100000 --runs 5
```

The benchmark generates JSON lines under `target/benchmarks`, checks that both binaries produce identical output, and reports median elapsed times for seven workloads. It alternates execution order and discards output during timing. Results depend on the machine and data.

For a smaller binary, Cargo also supports a size-oriented build without changing the configuration:

```bash
CARGO_PROFILE_RELEASE_OPT_LEVEL=z cargo build --release --locked
```

## API

Implement `Filterable` for your type, compile the expression once with `Expr::compile`, and reuse `CompiledFilter::evaluate`. Compilation returns errors for unsupported expressions before any records are read. `serde_json::Value` implements `Filterable` directly and borrows arrays and objects without copying them.

`Expr::evaluate` is a convenience method for one record: it compiles on every call and returns false if compilation fails. Use `Expr::compile` when errors must be reported.

JSON numbers use `i64`, `u64`, or finite `f64` when representable. If another crate enables `serde_json/arbitrary_precision`, larger values are retained as `Value::JsonNumber` without panicking or converting to infinity. Numeric comparisons against an out-of-range value return false, including `!=`; presence checks and exact bare-literal matching of its JSON number text remain available. This does not provide arbitrary-precision arithmetic.

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
