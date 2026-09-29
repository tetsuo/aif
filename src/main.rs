use aif::{CompiledFilter, parse};
use serde_json::Value as Json;
use std::{
    fs::File,
    io::{self, BufRead, BufReader, BufWriter, Read, Write},
    path::Path,
    process,
};

const DEFAULT_MAX_RECORD_BYTES: usize = 8 * 1024 * 1024;
const USAGE: &str = "[-p|--print] [--max-record-bytes N] [--] <expr> [file...]";

struct RecordOutput<W> {
    writer: W,
    needs_separator: bool,
}

impl<W: Write> RecordOutput<W> {
    fn write_record(&mut self, record: &[u8]) -> io::Result<()> {
        if self.needs_separator {
            self.writer.write_all(b"\n")?;
        }
        self.writer.write_all(record)?;
        self.needs_separator = !record.ends_with(b"\n");
        Ok(())
    }
}

fn filter_lines(
    expr: Option<&CompiledFilter<'_>>,
    mut reader: impl BufRead,
    out: &mut RecordOutput<impl Write>,
    source: &str,
    max_record_bytes: usize,
) -> io::Result<bool> {
    let Some(expr) = expr else {
        io::copy(&mut reader, &mut out.writer)?;
        return Ok(true);
    };
    let mut line_buffer = Vec::with_capacity(128);
    let mut lineno = 0;
    let mut valid = true;

    while (&mut reader)
        .take(max_record_bytes as u64 + 1)
        .read_until(b'\n', &mut line_buffer)?
        > 0
    {
        lineno += 1;
        if line_buffer.len() > max_record_bytes {
            eprintln!("{source}:{lineno}: record exceeds {max_record_bytes} bytes");
            valid = false;
            if line_buffer.last() != Some(&b'\n') {
                reader.skip_until(b'\n')?;
            }
        } else if !line_buffer.iter().all(u8::is_ascii_whitespace) {
            match serde_json::from_slice::<Json>(&line_buffer) {
                Ok(json) => {
                    if expr.evaluate(&json) {
                        out.write_record(&line_buffer)?;
                    }
                }
                Err(e) => {
                    eprintln!("{source}:{lineno}: invalid JSON: {e}");
                    valid = false;
                }
            }
        }
        line_buffer.clear();
    }

    Ok(valid)
}

fn usage(program: &str) -> ! {
    eprintln!("usage: {program} {USAGE}");
    process::exit(2);
}

fn print_info(program: &str, message: &str) {
    let mut out = io::stdout().lock();
    if let Err(error) = writeln!(out, "{message}").and_then(|()| out.flush()) {
        eprintln!("{program}: {error}");
        process::exit(1);
    }
}

fn main() {
    let mut args = std::env::args();
    let program = args.next().unwrap();
    let mut print_only = false;
    let mut max_record_bytes = DEFAULT_MAX_RECORD_BYTES;
    let raw_expr = loop {
        let arg = args.next().unwrap_or_else(|| usage(&program));
        match arg.as_str() {
            "-p" | "--print" => print_only = true,
            "--max-record-bytes" => {
                max_record_bytes = args
                    .next()
                    .and_then(|value| value.parse::<usize>().ok())
                    .filter(|value| *value > 0 && value.checked_add(1).is_some())
                    .unwrap_or_else(|| usage(&program));
            }
            "--" => break args.next().unwrap_or_else(|| usage(&program)),
            "-h" | "--help" => {
                return print_info(
                    &program,
                    &format!(
                        "usage: {program} {USAGE}\n\n\
                     -p, --print           Print the parsed expression.\n\
                     --max-record-bytes N  Limit each record, including its newline (default: {DEFAULT_MAX_RECORD_BYTES}).\n\
                     -h, --help            Print help without reading input.\n\
                     -V, --version         Print the version without reading input.\n\
                     --                    End option parsing."
                    ),
                );
            }
            "-V" | "--version" => {
                return print_info(&program, concat!("aif ", env!("CARGO_PKG_VERSION")));
            }
            _ if arg.starts_with("--") => {
                eprintln!("{program}: unknown option: {arg}; use -- before a literal expression");
                usage(&program);
            }
            _ => break arg,
        }
    };
    let mut paths = args.peekable();
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
        let filter = expr
            .as_ref()
            .map(|expr| expr.compile())
            .transpose()
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
        let mut records = RecordOutput {
            writer: &mut out,
            needs_separator: false,
        };
        if paths.peek().is_none() {
            return filter_lines(
                filter.as_ref(),
                io::stdin().lock(),
                &mut records,
                "<stdin>",
                max_record_bytes,
            );
        }
        let mut valid = true;
        for path in paths {
            let file = File::open(Path::new(&path))
                .map_err(|e| io::Error::new(e.kind(), format!("{path}: {e}")))?;
            valid &= filter_lines(
                filter.as_ref(),
                BufReader::new(file),
                &mut records,
                &path,
                max_record_bytes,
            )
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
