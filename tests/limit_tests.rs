use ele::{parse, parser::MAX_FILTER_BYTES};
use std::process::Command;

#[test]
fn excessive_filters_report_errors_without_crashing() {
    let filters = [
        format!("{}x{}", "(".repeat(4096), ")".repeat(4096)),
        format!("{}x{}", "(".repeat(100), ")".repeat(100)),
        format!("{}x", "NOT ".repeat(100)),
        format!("{}x{}", "f(".repeat(100), ")".repeat(100)),
        "x ".repeat(300),
        "x.".repeat(300) + "x",
        "x AND ".repeat(300) + "x",
        // Three-byte UTF-8 characters keep the argument below Windows' UTF-16
        // command-line limit while exceeding the filter byte limit.
        "界".repeat(MAX_FILTER_BYTES / "界".len() + 1),
    ];
    for filter in filters {
        let output = Command::new(env!("CARGO_BIN_EXE_ele"))
            .args(["--print", &filter])
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(1),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains("exceeds"));
    }
}

#[test]
fn filter_byte_limit_counts_utf8_bytes() {
    for character in ["x", "界"] {
        let within_limit = character.repeat(MAX_FILTER_BYTES / character.len());
        assert!(parse(&within_limit).is_ok());
        assert_eq!(
            parse(&(within_limit + character)).unwrap_err(),
            format!("filter exceeds {MAX_FILTER_BYTES} bytes")
        );
    }
}

#[test]
fn parser_limits_work_on_a_normal_thread_stack() {
    std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(|| {
            assert!(parse(&format!("{}x{}", "(".repeat(100), ")".repeat(100))).is_err());
            assert!(parse(&("x ".repeat(300))).is_err());
            assert!(parse(&format!("{}x", "NOT ".repeat(100))).is_err());
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn ordinary_nested_filters_still_parse() {
    assert!(
        parse(&format!("{}x{}", "(".repeat(16), ")".repeat(16)))
            .unwrap()
            .is_some()
    );
    assert!(parse(&("x AND ".repeat(20) + "x")).unwrap().is_some());
    assert!(parse(&("x.".repeat(20) + "x = 1")).unwrap().is_some());
    assert!(
        parse(&format!("\"{}\"", "x".repeat(65534)))
            .unwrap()
            .is_some()
    );
}
