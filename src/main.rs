use aip_filter::{parse, CompiledFilter};
use serde_json::Value as Json;
use std::{
    fs::File,
    io::{self, BufRead, BufReader, BufWriter, Write},
    path::Path,
    process,
};

fn filter_lines(
    expr: Option<&CompiledFilter<'_>>,
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
                    if expr.evaluate(&json) {
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
        let filter = expr.as_ref().map(|expr| expr.compile()).transpose()
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
        if args.is_empty() {
            return filter_lines(filter.as_ref(), io::stdin().lock(), &mut out, "<stdin>");
        }
        let mut valid = true;
        for path in &args {
            let file = File::open(Path::new(path))
                .map_err(|e| io::Error::new(e.kind(), format!("{path}: {e}")))?;
            valid &= filter_lines(filter.as_ref(), BufReader::new(file), &mut out, path)
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
