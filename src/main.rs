use aip_filter::{parse, Expr, Filterable, Value};
use serde_json::Value as Json;
use std::{
    fs::File,
    io::{self, BufRead, BufReader, BufWriter, Write},
    path::Path,
    process,
};

struct JsonRecord<'j>(&'j Json);

impl<'j> Filterable for JsonRecord<'j> {
    fn field(&self, name: &str) -> Option<Value<'_>> {
        json_to_value(self.0.get(name)?)
    }

    fn field_path(&self, path: &[&str]) -> Option<Value<'_>> {
        let mut cur = self.0;
        for &segment in path {
            cur = cur.get(segment)?;
        }
        json_to_value(cur)
    }

    fn all_field_values(&self) -> Vec<Value<'_>> {
        match self.0 {
            Json::Object(map) => map.values().filter_map(json_to_value).collect(),
            _ => json_to_value(self.0).into_iter().collect(),
        }
    }

    fn matches_global(&self, search: &str) -> bool {
        match self.0 {
            Json::Object(map) => map
                .values()
                .any(|value| JsonRecord(value).matches_global(search)),
            Json::Array(items) => items
                .iter()
                .any(|value| JsonRecord(value).matches_global(search)),
            _ => json_to_value(self.0).is_some_and(|value| value.matches_global(search)),
        }
    }
}

fn json_to_value(v: &Json) -> Option<Value<'_>> {
    match v {
        Json::String(s)  => Some(Value::String(s.as_str())),
        Json::Bool(b)    => Some(Value::Bool(*b)),
        Json::Null       => Some(Value::Null),
        Json::Number(n)  => {
            if let Some(i) = n.as_i64() { return Some(Value::Int(i)); }
            if let Some(u) = n.as_u64() { return Some(Value::Uint(u)); }
            n.as_f64().map(Value::Float)
        }
        Json::Array(arr) => Some(Value::List(
            arr.iter().filter_map(json_to_value).collect(),
        )),
        Json::Object(map) => Some(Value::Map(
            map.iter()
                .filter_map(|(key, value)| {
                    json_to_value(value).map(|value| (key.as_str(), value))
                })
                .collect(),
        )),
    }
}

fn filter_lines(
    expr: Option<&Expr>,
    mut reader: impl BufRead,
    out: &mut impl Write,
    source: &str,
) -> io::Result<bool> {
    let Some(expr) = expr else {
        io::copy(&mut reader, out)?;
        return Ok(true);
    };
    let mut line_buffer = String::with_capacity(128);
    let mut lineno = 0;
    let mut valid = true;

    while reader.read_line(&mut line_buffer)? > 0 {
        lineno += 1;

        // Skip empty lines
        if !line_buffer.trim().is_empty() {
            match serde_json::from_str::<Json>(&line_buffer) {
                Ok(json) => {
                    if expr.evaluate(&JsonRecord(&json)) {
                        // Write directly to stdout
                        out.write_all(line_buffer.as_bytes())?;
                    }
                }
                Err(e) => {
                    eprintln!("{}:{}: invalid JSON: {}", source, lineno, e);
                    valid = false;
                }
            }
        }

        // Clear the buffer for the next line,
        // which keeps the memory allocated but resets the length to 0.
        line_buffer.clear();
    }

    Ok(valid)
}

fn usage(program: &str) -> ! {
    eprintln!("usage: {program} [-p|--print] <expr> [file...]");
    process::exit(2);
}

fn main() {
    let mut args: Vec<String> = std::env::args().collect();
    let program = args.remove(0);

    if args.is_empty() || args[0] == "-h" || args[0] == "--help" {
        usage(&program);
    }

    // Check for print flag
    let mut print_only = false;
    if args[0] == "-p" || args[0] == "--print" {
        print_only = true;
        args.remove(0);
        if args.is_empty() { usage(&program); }
    }

    let raw_expr = args.remove(0);
    let expr = match parse(&raw_expr) {
        Ok(expr) => expr,
        Err(e) => {
            eprintln!("{program}: {e}");
            process::exit(1);
        }
    };

    let mut out = BufWriter::new(io::stdout().lock());
    let result = (|| -> io::Result<bool> {
        if print_only {
            if let Some(expr) = &expr {
                writeln!(out, "{expr}")?;
            }
            return Ok(true);
        }
        if args.is_empty() {
            return filter_lines(expr.as_ref(), io::stdin().lock(), &mut out, "<stdin>");
        }
        let mut valid = true;
        for path in &args {
            let file = File::open(Path::new(path))
                .map_err(|e| io::Error::new(e.kind(), format!("{path}: {e}")))?;
            valid &= filter_lines(expr.as_ref(), BufReader::new(file), &mut out, path)
                .map_err(|e| io::Error::new(e.kind(), format!("{path}: {e}")))?;
        }
        Ok(valid)
    })();
    let flushed = out.flush();
    match result.and_then(|valid| flushed.map(|()| valid)) {
        Ok(true) => {}
        Ok(false) => process::exit(1),
        Err(e) => {
            eprintln!("{program}: {e}");
            process::exit(1);
        }
    }
}
