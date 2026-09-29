use std::io::Write;
use std::process::{Command, Output, Stdio};

fn run(expr: &str, input: &str, args: &[&str]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_aip-filter"))
        .args(args)
        .arg(expr)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
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
}

#[test]
fn empty_filter_and_print_mode() {
    let input = "{\"msg\":\"foobar\"}\n";
    let output = run("", input, &[]);
    assert!(output.status.success());
    assert_eq!(output.stdout, input.as_bytes());
    let output = run("", input, &["--print"]);
    assert!(output.status.success());
    assert!(output.stdout.is_empty());

    let path = format!("{}/README.md", env!("CARGO_MANIFEST_DIR"));
    let output = Command::new(env!("CARGO_BIN_EXE_aip-filter"))
        .args(["", &path])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, std::fs::read(path).unwrap());
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

#[cfg(unix)]
#[test]
fn output_flush_errors_are_reported() {
    use std::os::fd::OwnedFd;
    use std::os::unix::net::UnixStream;
    let (writer, reader) = UnixStream::pair().unwrap();
    drop(reader);
    let mut child = Command::new(env!("CARGO_BIN_EXE_aip-filter"))
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
}
