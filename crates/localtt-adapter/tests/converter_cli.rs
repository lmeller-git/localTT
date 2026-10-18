//! Process-level checks of the converter's CLI contract
//! (`docs/rfcs/content.md` section 4.3): `--help` prints the usage and exits
//! 0, a malformed flag exits 2 with the usage on stderr, a failed import exits
//! 1, a successful conversion prints the counts and exits 0.

use std::{fs, path::Path, process::Command};

use tempfile::TempDir;

/// The binary Cargo built for this package.
const BIN: &str = env!("CARGO_BIN_EXE_localtt-content-convert");

/// The eleven dataset files the converter requires, all empty arrays.
fn empty_dataset() -> TempDir {
    let dir = TempDir::new().expect("a temp dir");
    for file in [
        "spells",
        "monsters",
        "magicitems",
        "weapons",
        "armor",
        "conditions",
        "classes",
        "races",
        "backgrounds",
        "feats",
        "sections",
    ] {
        fs::write(dir.path().join(format!("{file}.json")), b"[]").expect("a dataset file");
    }
    dir
}

fn run(args: &[&str]) -> std::process::Output {
    Command::new(BIN)
        .args(args)
        .output()
        .expect("the converter runs")
}

/// The four conversion flags for one input/output pair.
fn conversion_args(input: &Path, output: &Path) -> Vec<String> {
    vec![
        "--input".to_owned(),
        input.display().to_string(),
        "--output".to_owned(),
        output.display().to_string(),
        "--namespace".to_owned(),
        "srd2014".to_owned(),
        "--source-revision".to_owned(),
        "rev-1".to_owned(),
    ]
}

#[test]
fn help_prints_usage_and_exits_zero() {
    let output = run(&["--help"]);

    assert!(output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("Usage:"),
        "usage belongs on stdout"
    );
}

#[test]
fn malformed_invocations_exit_two_with_usage_on_stderr() {
    for args in [
        vec!["--verbose", "x"],
        vec!["--input"],
        vec!["--input", "dataset"],
        vec![],
    ] {
        let output = run(&args);

        assert_eq!(output.status.code(), Some(2), "{args:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("Usage:"), "{args:?}: {stderr}");
    }
}

#[test]
fn a_failed_import_exits_one() {
    let input = TempDir::new().expect("a temp dir");
    let output = TempDir::new().expect("a temp dir");
    let args = conversion_args(input.path(), output.path());
    let args: Vec<&str> = args.iter().map(String::as_str).collect();

    let result = run(&args);

    assert_eq!(result.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(stderr.contains("spells.json"), "{stderr}");
}

#[test]
fn a_successful_conversion_prints_counts_and_exits_zero() {
    let input = empty_dataset();
    let output = TempDir::new().expect("a temp dir");
    let args = conversion_args(input.path(), output.path());
    let args: Vec<&str> = args.iter().map(String::as_str).collect();

    let result = run(&args);

    assert!(result.status.success());
    let stdout = String::from_utf8_lossy(&result.stdout);
    assert!(stdout.contains("wrote 0 entries"), "{stdout}");
    assert!(
        output.path().join("srd2014").join("index.json").exists(),
        "the namespace snapshot is written"
    );
}
