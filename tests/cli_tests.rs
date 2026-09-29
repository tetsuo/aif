use std::io::Write;
use std::process::{Command, Output, Stdio};

fn run(expr: &str, input: &str, args: &[&str]) -> Output {
    run_bytes(expr, input.as_bytes(), args)
}

fn run_bytes(expr: &str, input: &[u8], args: &[&str]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_aif"))
        .args(args)
        .arg(expr)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    child.wait_with_output().unwrap()
}

fn matches(expr: &str, record: &str, expected: bool) {
    let input = format!("{record}\n");
    let output = run(expr, &input, &[]);
    assert!(
        output.status.success(),
        "{expr}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        if expected { input } else { String::new() },
        "{expr} against {record}"
    );
}

#[test]
fn bare_literals_search_nested_values() {
    for record in [
        r#"{"msg":"hello foobar"}"#,
        r#"{"nested":{"msg":"hello foobar"}}"#,
        r#"{"events":[{"msg":"hello foobar"}]}"#,
        r#"{"events":[["hello foobar"]]}"#,
        r#"["hello foobar"]"#,
        r#""hello foobar""#,
    ] {
        matches("foobar", record, true);
        matches("FOOBAR", record, true);
        matches("absent", record, false);
    }
    matches("foobar", r#"{"foobar":"unrelated"}"#, false);
    matches("foobar", r#"{"foobar":"","msg":"hello foobar"}"#, true);
    matches("foo.bar", r#"{"msg":"hello foo.bar"}"#, true);
    matches("active", r#"{"active":true}"#, true);
    matches("active", r#"{"active":false}"#, false);
}

#[test]
fn quoted_literals_do_not_resolve_fields() {
    matches(r#""msg""#, r#"{"msg":"unrelated"}"#, false);
    matches(r#""active""#, r#"{"active":false,"msg":"active"}"#, true);
}

#[test]
fn unicode_wildcards_do_not_panic() {
    for expr in [
        r#"name = "x*""#,
        r#"name = "*x""#,
        r#"name : "x*""#,
        r#"name : "*x""#,
    ] {
        matches(expr, r#"{"name":"é"}"#, false);
    }
    matches(r#"name = "é*""#, r#"{"name":"étude"}"#, true);
    matches(r#"name != "op*""#, r#"{"name":"open"}"#, false);
    matches(r#"name != "op*""#, r#"{"name":"closed"}"#, true);
    matches(r#"names != "op*""#, r#"{"names":["open"]}"#, false);
}

#[test]
fn has_checks_later_phrases_and_unicode_boundaries() {
    matches(
        r#"msg:"hello world""#,
        r#"{"msg":"xhello world; hello world"}"#,
        true,
    );
    matches(r#"msg:"a a""#, r#"{"msg":"xa a a"}"#, true);
    matches("msg:a", r#"{"msg":"İa"}"#, false);
    matches("msg:a", r#"{"msg":"İ a"}"#, true);
    matches("msg:crash", r#"{"msg":"crashes"}"#, false);
}

#[test]
fn numeric_comparisons_do_not_truncate_or_round_integers() {
    for (expr, record, expected) in [
        ("n = 5.9", r#"{"n":5}"#, false),
        ("n < 5.9", r#"{"n":5}"#, true),
        ("n > -5.9", r#"{"n":-5}"#, true),
        ("n = -5", r#"{"n":-5}"#, true),
        ("n = 9007199254740992.0", r#"{"n":9007199254740993}"#, false),
        ("n = 9007199254740993", r#"{"n":9007199254740992.0}"#, false),
        ("n > -1", r#"{"n":18446744073709551615}"#, true),
        (
            "n < 18446744073709551616.0",
            r#"{"n":18446744073709551615}"#,
            true,
        ),
        (
            "n < -9223372036854775808.0",
            r#"{"n":-9223372036854775808}"#,
            false,
        ),
        ("n:0.0", r#"{"n":[0.00000000000000001]}"#, false),
        ("n:5.0", r#"{"n":[5]}"#, true),
    ] {
        matches(expr, record, expected);
    }
}

#[test]
fn has_and_boolean_values() {
    matches("flags:garbage", r#"{"flags":[false]}"#, false);
    matches("flags:false", r#"{"flags":[false]}"#, true);
    matches("flags:FALSE", r#"{"flags":[false]}"#, true);
    matches("flags:0", r#"{"flags":[false]}"#, true);
    matches("tags:*", r#"{"tags":[0]}"#, true);
    matches("tags:*", r#"{"tags":[]}"#, false);
    matches("meta:owner", r#"{"meta":{"owner":"alice"}}"#, true);
    matches("meta:*", r#"{"meta":{"owner":"alice"}}"#, true);
    matches("meta:*", r#"{"meta":{}}"#, false);
    matches("meta.owner=alice", r#"{"meta":{"owner":"alice"}}"#, true);
    matches("meta.zero:*", r#"{"meta":{"zero":0}}"#, true);
    matches("r.foo:42", r#"{"r":[{"foo":42}]}"#, true);
    matches("r.foo:42", r#"{"r":[{"foo":0}]}"#, false);
}

#[test]
fn empty_filter_and_print_mode() {
    let input = "{\"msg\":\"foobar\"}\n";
    let output = run("", input, &[]);
    assert!(output.status.success());
    assert_eq!(output.stdout, input.as_bytes());
    let output = Command::new(env!("CARGO_BIN_EXE_aif"))
        .args(["--print", ""])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(output.stdout.is_empty());

    let path = format!("{}/README.md", env!("CARGO_MANIFEST_DIR"));
    let output = Command::new(env!("CARGO_BIN_EXE_aif"))
        .args(["", &path])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, std::fs::read(path).unwrap());
}

#[test]
fn information_flags_succeed_and_unknown_long_options_fail() {
    for flag in ["--help", "-h", "--version", "-V"] {
        let output = Command::new(env!("CARGO_BIN_EXE_aif"))
            .arg(flag)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(0), "{flag}");
        assert!(output.stderr.is_empty());
        let text = String::from_utf8(output.stdout).unwrap();
        if matches!(flag, "--help" | "-h") {
            assert!(text.contains("usage:"));
            assert!(text.contains("--max-record-bytes"));
            assert!(text.contains("--version"));
        } else {
            assert_eq!(text, format!("aif {}\n", env!("CARGO_PKG_VERSION")));
        }
    }
    for flag in ["--verison", "--unknown", "--print=invalid"] {
        let output = Command::new(env!("CARGO_BIN_EXE_aif"))
            .arg(flag)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2), "{flag}");
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains("unknown option"));
    }
    let output = run("-- literal comment", "raw input\n", &["--"]);
    assert!(output.status.success());
    assert_eq!(output.stdout, b"raw input\n");
    let output = run("-p", "\"unrelated\"\n", &["--"]);
    assert!(output.status.success());
    assert_eq!(output.stdout, b"\"unrelated\"\n");
}

#[test]
fn invalid_json_is_reported_and_returns_failure() {
    let output = run("foobar", "not json\n{\"msg\":\"foobar\"}\n", &[]);
    assert!(!output.status.success());
    assert_eq!(output.stdout, b"{\"msg\":\"foobar\"}\n");
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("<stdin>:1: invalid JSON")
    );
}

#[test]
fn record_limits_preserve_boundaries_and_continue_after_errors() {
    let record = "\"match\"\n";
    let limit = record.len().to_string();
    let args = ["--max-record-bytes", limit.as_str()];
    let output = run("match", &(record.repeat(2)), &args);
    assert!(output.status.success());
    assert_eq!(output.stdout, record.repeat(2).as_bytes());

    for oversized in ["\"matchx\"\n", "\"a much longer match\"\n"] {
        let output = run("match", &format!("{oversized}{record}"), &args);
        assert_eq!(output.status.code(), Some(1));
        assert_eq!(output.stdout, record.as_bytes());
        assert!(String::from_utf8_lossy(&output.stderr).contains("<stdin>:1: record exceeds"));
    }
    let output = run("match", "\"an unterminated match", &args);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());

    let output = run("match", "\"match\"", &["--max-record-bytes", "7"]);
    assert!(output.status.success());
    assert_eq!(output.stdout, b"\"match\"");
    let output = run("match", "\"match\"\r\n", &["--max-record-bytes", "9"]);
    assert!(output.status.success());
    assert_eq!(output.stdout, b"\"match\"\r\n");
}

#[test]
fn default_record_limit_is_enforced() {
    let input = "x".repeat(8 * 1024 * 1024) + "\n\"match\"\n";
    let output = run("match", &input, &[]);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(output.stdout, b"\"match\"\n");
    assert!(String::from_utf8_lossy(&output.stderr).contains("record exceeds 8388608 bytes"));
}

#[test]
fn invalid_record_limit_arguments_fail() {
    for limit in ["0", "-1", "invalid", "18446744073709551615"] {
        let output = run("match", "", &["--max-record-bytes", limit]);
        assert_eq!(output.status.code(), Some(2));
    }
}

#[test]
fn invalid_utf8_is_reported_without_losing_later_records() {
    let output = run_bytes("match", b"\"\xff\"\n\"match\"\n", &[]);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(output.stdout, b"\"match\"\n");
    assert!(String::from_utf8_lossy(&output.stderr).contains("<stdin>:1: invalid JSON"));
    let output = run_bytes("", b"\xff\n", &["--max-record-bytes", "1"]);
    assert!(output.status.success());
    assert_eq!(output.stdout, b"\xff\n");
}

#[test]
fn filtering_multiple_files_preserves_successful_output() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join(format!("cli-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let first = dir.join("first.jsonl");
    let second = dir.join("second.jsonl");
    std::fs::write(&first, b"\"match first\"\n").unwrap();
    std::fs::write(&second, b"invalid\n\"match second\"\n").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_aif"))
        .arg("match")
        .arg(&first)
        .arg(&second)
        .output()
        .unwrap();
    std::fs::remove_dir_all(dir).unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(output.stdout, b"\"match first\"\n\"match second\"\n");
    assert!(String::from_utf8_lossy(&output.stderr).contains("second.jsonl:1: invalid JSON"));
}

#[test]
fn multiple_files_keep_record_boundaries_without_forcing_a_final_newline() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join(format!("cli-boundaries-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let first = dir.join("first.jsonl");
    let middle = dir.join("middle.jsonl");
    let last = dir.join("last.jsonl");
    std::fs::write(&middle, b"\n\"unrelated\"\n").unwrap();
    for ending in ["", "\n", "\r\n"] {
        std::fs::write(&first, format!("\"match first\"{ending}")).unwrap();
        std::fs::write(&last, b"\"match last\"").unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_aif"))
            .arg("match")
            .args([&first, &middle, &last])
            .output()
            .unwrap();
        assert!(output.status.success());
        let separator = if ending.is_empty() { "\n" } else { ending };
        assert_eq!(
            output.stdout,
            format!("\"match first\"{separator}\"match last\"").as_bytes()
        );
        let values: Vec<serde_json::Value> = serde_json::Deserializer::from_slice(&output.stdout)
            .into_iter()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(values.len(), 2);
    }
    std::fs::write(&first, b"raw").unwrap();
    std::fs::write(&last, b"bytes").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_aif"))
        .arg("")
        .args([&first, &last])
        .output()
        .unwrap();
    std::fs::remove_dir_all(dir).unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, b"rawbytes");
}

#[cfg(unix)]
#[test]
fn output_flush_errors_are_reported() {
    use std::os::fd::OwnedFd;
    use std::os::unix::net::UnixStream;
    let (writer, reader) = UnixStream::pair().unwrap();
    drop(reader);
    let mut child = Command::new(env!("CARGO_BIN_EXE_aif"))
        .arg("foobar")
        .stdin(Stdio::piped())
        .stdout(Stdio::from(OwnedFd::from(writer)))
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"{\"msg\":\"foobar\"}\n")
        .unwrap();
    assert!(!child.wait_with_output().unwrap().status.success());
    for flag in ["--help", "--version"] {
        let (writer, reader) = UnixStream::pair().unwrap();
        drop(reader);
        let output = Command::new(env!("CARGO_BIN_EXE_aif"))
            .arg(flag)
            .stdin(Stdio::null())
            .stdout(Stdio::from(OwnedFd::from(writer)))
            .stderr(Stdio::piped())
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1));
    }
}
