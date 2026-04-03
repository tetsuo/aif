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
            _ => vec![],
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
        // Nested objects are accessible via field_path; skip at top level.
        Json::Object(_) => None,
    }
}

fn filter_lines(
    expr: &Expr,
    mut reader: impl BufRead,
    out: &mut impl Write,
    source: &str,
) -> io::Result<()> {
    let mut line_buffer = String::with_capacity(128);
    let mut lineno = 0;

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
                }
            }
        }

        // Clear the buffer for the next line,
        // which keeps the memory allocated but resets the length to 0.
        line_buffer.clear();
    }

    Ok(())
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
        Ok(Some(e)) => e,
        Ok(None) => {
            // Empty expression matches everything; just cat the input.
            let stdin = io::stdin();
            let mut out = BufWriter::new(io::stdout().lock());
            for line in stdin.lock().lines() {
                writeln!(out, "{}", line.unwrap_or_default()).ok();
            }
            return;
        }
        Err(e) => {
            eprintln!("{program}: {e}");
            process::exit(1);
        }
    };

    if print_only {
        println!("{}", expr);
        return;
    }

    let mut out = BufWriter::new(io::stdout().lock());

    if args.is_empty() {
        // Read from stdin.
        let stdin = io::stdin();
        if let Err(e) = filter_lines(&expr, stdin.lock(), &mut out, "<stdin>") {
            eprintln!("{program}: {e}");
            process::exit(1);
        }
    } else {
        for path in &args {
            let source = path.as_str();
            let file = match File::open(Path::new(path)) {
                Ok(f) => f,
                Err(e) => {
                    eprintln!("{program}: {path}: {e}");
                    process::exit(1);
                }
            };
            if let Err(e) = filter_lines(&expr, BufReader::new(file), &mut out, source) {
                eprintln!("{program}: {path}: {e}");
                process::exit(1);
            }
        }
    }
}
