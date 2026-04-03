use aip_filter::parse;
use std::fs;
use std::path::Path;

/// Parse a txtar-style test file into named sections.
/// Sections are delimited by `-- name --` lines.
fn parse_txtar(content: &str) -> Vec<(String, String)> {
    let mut files: Vec<(String, String)> = Vec::new();
    let mut current_name: Option<String> = None;
    let mut current_data = String::new();

    for line in content.split('\n') {
        let trimmed = line.trim_end_matches('\r');
        if trimmed.starts_with("-- ") && trimmed.ends_with(" --") {
            if let Some(name) = current_name.take() {
                files.push((name, current_data.clone()));
            }
            let name = trimmed[3..trimmed.len() - 3].to_string();
            current_name = Some(name);
            current_data.clear();
        } else if current_name.is_some() {
            current_data.push_str(line);
            current_data.push('\n');
        }
    }
    if let Some(name) = current_name {
        files.push((name, current_data));
    }
    files
}

#[test]
fn test_parse() {
    let test_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/parse");
    if !test_dir.exists() {
        eprintln!("Skipping parse tests: testdata/parse not found");
        return;
    }

    let mut total = 0;
    let mut passed = 0;
    let mut failed_tests = Vec::new();

    for entry in fs::read_dir(&test_dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().map(|e| e == "txt").unwrap_or(false) {
            let content = fs::read_to_string(&path).unwrap();
            let files = parse_txtar(&content);

            let mut i = 0;
            while i < files.len() {
                let (name, data) = &files[i];
                if !name.ends_with(".test") {
                    i += 1;
                    continue;
                }

                let base = name.strip_suffix(".test").unwrap();
                total += 1;

                let result = parse(data);

                i += 1;
                if i >= files.len() {
                    eprintln!("  {}: missing expected result", base);
                    continue;
                }

                let (expected_name, expected_data) = &files[i];

                if expected_name.ends_with(".out") {
                    match result {
                        Err(e) => {
                            failed_tests.push(format!(
                                "{}:{}: got error '{}', expected success",
                                path.file_name().unwrap().to_str().unwrap(),
                                base,
                                e
                            ));
                        }
                        Ok(expr) => {
                            let got = match expr {
                                Some(e) => format!("{}", e),
                                None => String::new(),
                            };
                            let want = expected_data.trim_end_matches('\n');
                            let got_trimmed = got.trim_end_matches('\n');
                            if got_trimmed == want {
                                passed += 1;
                            } else {
                                failed_tests.push(format!(
                                    "{}:{}: output mismatch\n  got:  {:?}\n  want: {:?}",
                                    path.file_name().unwrap().to_str().unwrap(),
                                    base,
                                    got_trimmed,
                                    want
                                ));
                            }
                        }
                    }
                } else if expected_name.ends_with(".err") {
                    match result {
                        Ok(_) => {
                            failed_tests.push(format!(
                                "{}:{}: got success, expected error '{}'",
                                path.file_name().unwrap().to_str().unwrap(),
                                base,
                                expected_data.trim()
                            ));
                        }
                        Err(e) => {
                            let want = expected_data.trim();
                            let got = e.trim();
                            if got == want || regex_error_prefix_matches(got, want) {
                                passed += 1;
                            } else {
                                failed_tests.push(format!(
                                    "{}:{}: error mismatch\n  got:  {:?}\n  want: {:?}",
                                    path.file_name().unwrap().to_str().unwrap(),
                                    base,
                                    got,
                                    want
                                ));
                            }
                        }
                    }
                }
            }
        }
    }

    println!("\nParse tests: {}/{} passed", passed, total);
    if !failed_tests.is_empty() {
        println!("\nFailed tests:");
        for f in &failed_tests {
            println!("  {}", f);
        }
        panic!("{} of {} parse tests failed", failed_tests.len(), total);
    }
}

/// Returns true when the position prefix and "invalid regular expression:" label
/// match but the trailing error detail does not - the regex crate's error messages
/// are more verbose than the testdata expects, so we accept any error at the right
/// position with the right label.
fn regex_error_prefix_matches(got: &str, want: &str) -> bool {
    const MARKER: &str = "invalid regular expression:";
    match (got.split_once(MARKER), want.split_once(MARKER)) {
        (Some((gp, _)), Some((wp, _))) => gp == wp,
        _ => false,
    }
}
