//! The Open5e -> localtt snapshot conversion tool
//! (`docs/rfcs/content.md` sections 4.3 and 6.4).
//!
//! Usage:
//!
//! ```text
//! localtt-content-convert --input <dataset-dir> --output <data-root> \
//!     --namespace <ns> --source-revision <rev>
//! ```
//!
//! `--input` is the Open5e dataset directory (for the pinned checkout,
//! `data/WOTC_5e_SRD_v5.1/`); `--output` is a data root whose `<namespace>/`
//! directory is (re)written by `write_source`. All four flags are required;
//! success prints the per-kind counts, failure exits non-zero. The output root
//! and the namespace directory are created when missing.

use std::{env, path::PathBuf, process::ExitCode};

use localtt_adapter::reference::{
    import::{Importer, Open5eImporter},
    write_source,
};
use localtt_core::types::Namespace;

/// The converter's command line.
#[derive(Debug)]
struct Args {
    /// Dataset directory holding the eleven `*.json` files.
    input: PathBuf,
    /// Data root to write the namespace directory under.
    output: PathBuf,
    /// Namespace to write.
    namespace: String,
    /// Pinned source revision, recorded verbatim.
    source_revision: String,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("localtt-content-convert: {message}");
            ExitCode::FAILURE
        }
    }
}

/// Runs the conversion and prints the per-kind counts.
fn run() -> Result<(), String> {
    let args = parse_args(env::args().skip(1))?;
    let namespace = Namespace::parse(&args.namespace)
        .map_err(|error| format!("invalid namespace {:?}: {error}", args.namespace))?;
    let importer = Open5eImporter::new(namespace, args.source_revision);
    let snapshot = importer
        .import(&args.input)
        .map_err(|error| error.to_string())?;
    write_source(&args.output, &snapshot).map_err(|error| error.to_string())?;

    println!(
        "wrote {} entries under {}/",
        snapshot.entries.len(),
        args.output.display()
    );
    for (kind, count) in &snapshot.report.counts {
        println!("  {}: {count}", kind.as_str());
    }
    Ok(())
}

/// Parses `--input`, `--output`, `--namespace` and `--source-revision`; every
/// flag is required and takes one value.
fn parse_args(args: impl Iterator<Item = String>) -> Result<Args, String> {
    let mut args = args;
    let mut input = None;
    let mut output = None;
    let mut namespace = None;
    let mut source_revision = None;
    while let Some(flag) = args.next() {
        let value = args.next().ok_or_else(|| format!("{flag} needs a value"))?;
        match flag.as_str() {
            "--input" => input = Some(PathBuf::from(value)),
            "--output" => output = Some(PathBuf::from(value)),
            "--namespace" => namespace = Some(value),
            "--source-revision" => source_revision = Some(value),
            other => return Err(format!("unknown flag {other:?}")),
        }
    }
    Ok(Args {
        input: input.ok_or("--input is required")?,
        output: output.ok_or("--output is required")?,
        namespace: namespace.ok_or("--namespace is required")?,
        source_revision: source_revision.ok_or("--source-revision is required")?,
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{Args, parse_args};

    fn parse(values: &[&str]) -> Result<Args, String> {
        parse_args(values.iter().map(|value| (*value).to_owned()))
    }

    #[test]
    fn parses_the_four_required_flags() {
        let args = parse(&[
            "--input",
            "dataset",
            "--output",
            "data",
            "--namespace",
            "srd2014",
            "--source-revision",
            "rev-1",
        ])
        .expect("the flags parse");

        assert_eq!(args.input, PathBuf::from("dataset"));
        assert_eq!(args.output, PathBuf::from("data"));
        assert_eq!(args.namespace, "srd2014");
        assert_eq!(args.source_revision, "rev-1");
    }

    #[test]
    fn flags_are_required() {
        let error = parse(&["--input", "dataset"]).expect_err("incomplete flags must fail");

        assert!(error.contains("--output"), "{error}");
    }

    #[test]
    fn a_flag_without_a_value_fails() {
        let error = parse(&["--input"]).expect_err("a dangling flag must fail");

        assert!(error.contains("--input"), "{error}");
    }

    #[test]
    fn an_unknown_flag_fails() {
        let error = parse(&["--verbose"]).expect_err("an unknown flag must fail");

        assert!(error.contains("--verbose"), "{error}");
    }
}
