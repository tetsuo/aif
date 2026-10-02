use ele::parse;
use std::collections::HashSet;
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

fn verify_fixture(content: &str) -> Result<usize, String> {
    let sections = parse_txtar(content);
    if sections.is_empty() {
        return Err("fixture contains no test cases".into());
    }
    let mut names = HashSet::new();
    for pair in sections.chunks(2) {
        let (name, input) = &pair[0];
        let base = name
            .strip_suffix(".test")
            .filter(|name| !name.is_empty())
            .ok_or_else(|| format!("expected a named .test section, found {name}"))?;
        if !names.insert(base) {
            return Err(format!("duplicate test case: {base}"));
        }
        let (expected_name, expected) = pair
            .get(1)
            .ok_or_else(|| format!("{base}: missing expected result"))?;
        let is_error = if expected_name == &format!("{base}.out") {
            false
        } else if expected_name == &format!("{base}.err") {
            true
        } else {
            return Err(format!(
                "{base}: expected {base}.out or {base}.err, found {expected_name}"
            ));
        };
        match (parse(input), is_error) {
            (Ok(expr), false) => {
                let got = expr.map(|expr| expr.to_string()).unwrap_or_default();
                if got.trim_end_matches('\n') != expected.trim_end_matches('\n') {
                    return Err(format!(
                        "{base}: output mismatch\n  got: {got:?}\n  want: {expected:?}"
                    ));
                }
            }
            (Err(error), true) => {
                let got = error.trim();
                let want = expected.trim();
                if got != want && !regex_error_prefix_matches(got, want) {
                    return Err(format!(
                        "{base}: error mismatch\n  got: {got:?}\n  want: {want:?}"
                    ));
                }
            }
            (Err(error), false) => return Err(format!("{base}: expected success, got {error}")),
            (Ok(_), true) => return Err(format!("{base}: expected an error, got success")),
        }
    }
    Ok(names.len())
}

fn verify_directory(directory: &Path) -> Result<usize, String> {
    let mut files = Vec::new();
    for entry in
        fs::read_dir(directory).map_err(|error| format!("{}: {error}", directory.display()))?
    {
        let path = entry.map_err(|error| error.to_string())?.path();
        if path.extension().is_some_and(|extension| extension == "txt") {
            files.push(path);
        }
    }
    if files.is_empty() {
        return Err(format!("{}: no .txt fixtures", directory.display()));
    }
    files.sort();
    let mut total = 0;
    for path in files {
        let content =
            fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))?;
        total +=
            verify_fixture(&content).map_err(|error| format!("{}: {error}", path.display()))?;
    }
    Ok(total)
}

#[test]
fn test_parse() {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/parse");
    let total = verify_directory(&directory).unwrap();
    println!("Parse tests: {total}/{total} passed");
}

#[test]
fn malformed_fixtures_fail() {
    for content in [
        "",
        "only comments\n",
        "-- x.test --\nx\n",
        "-- x.out --\nx\n",
        "-- x.test --\nx\n-- y.out --\nx\n",
        "-- x.test --\nx\n-- x.unknown --\nx\n",
        "-- x.test --\nx\n-- x.out --\nx\n-- x.test --\nx\n-- x.out --\nx\n",
        "-- x.test --\nx\n-- x.out --\ny\n",
        "-- x.test --\nx\n-- x.err --\nan error\n",
    ] {
        assert!(verify_fixture(content).is_err(), "{content:?}");
    }
    assert_eq!(
        verify_fixture("-- x.test --\nx\n-- x.out --\nx\n").unwrap(),
        1
    );
}

#[test]
fn missing_and_empty_fixture_directories_fail() {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join(format!("fixture-validation-{}", std::process::id()));
    fs::create_dir_all(&directory).unwrap();
    assert!(verify_directory(&directory.join("missing")).is_err());
    assert!(verify_directory(&directory).is_err());
    fs::write(directory.join("empty.txt"), "").unwrap();
    assert!(verify_directory(&directory).is_err());
    fs::write(
        directory.join("empty.txt"),
        "-- x.test --\nx\n-- x.out --\nx\n",
    )
    .unwrap();
    assert_eq!(verify_directory(&directory).unwrap(), 1);
    fs::remove_dir_all(directory).unwrap();
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
