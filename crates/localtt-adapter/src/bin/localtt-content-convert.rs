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
//! `--help` prints the usage and exits 0, an unknown or malformed flag prints
//! the usage to stderr and exits 2, a successful conversion prints the
//! per-kind counts and exits 0, and an import or write failure prints the
//! error and exits 1. The output root and the namespace directory are created
//! when missing.

use std::{env, path::PathBuf, process::ExitCode};

use localtt_adapter::reference::{
    import::{Importer, Open5eImporter},
    write_source,
};
use localtt_core::types::Namespace;

/// The usage block printed for `--help` and for malformed invocations.
const USAGE: &str = "\
Usage: localtt-content-convert --input <dataset-dir> --output <data-root> \
--namespace <ns> --source-revision <rev>

  --input <dataset-dir>      Open5e dataset directory (the pinned checkout's
                             data/WOTC_5e_SRD_v5.1)
  --output <data-root>       data root; <namespace>/ is (re)written under it
  --namespace <ns>           namespace to write, e.g. srd2014
  --source-revision <rev>    pinned upstream revision, recorded verbatim
  --help                     print this usage
";

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

/// Why the command line did not parse.
#[derive(Debug)]
enum ParseError {
    /// `--help` was requested: print the usage and exit 0.
    Help,
    /// A flag was unknown or malformed: print the usage and exit 2.
    Malformed(String),
}

fn main() -> ExitCode {
    let args = match parse_args(env::args().skip(1)) {
        Ok(args) => args,
        Err(ParseError::Help) => {
            print!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Err(ParseError::Malformed(message)) => {
            eprintln!("localtt-content-convert: {message}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("localtt-content-convert: {message}");
            ExitCode::FAILURE
        }
    }
}

/// Runs the conversion and prints the per-kind counts.
fn run(args: &Args) -> Result<(), String> {
    let namespace = Namespace::parse(&args.namespace)
        .map_err(|error| format!("invalid namespace {:?}: {error}", args.namespace))?;
    let importer = Open5eImporter::new(namespace, args.source_revision.clone());
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

/// Parses `--help` and the four required flags; every flag takes one value.
fn parse_args(args: impl Iterator<Item = String>) -> Result<Args, ParseError> {
    let mut args = args;
    let mut input = None;
    let mut output = None;
    let mut namespace = None;
    let mut source_revision = None;
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--help" => return Err(ParseError::Help),
            "--input" => input = Some(PathBuf::from(value(&mut args, &flag)?)),
            "--output" => output = Some(PathBuf::from(value(&mut args, &flag)?)),
            "--namespace" => namespace = Some(value(&mut args, &flag)?),
            "--source-revision" => source_revision = Some(value(&mut args, &flag)?),
            other => return Err(ParseError::Malformed(format!("unknown flag {other:?}"))),
        }
    }
    Ok(Args {
        input: input.ok_or_else(|| missing("--input"))?,
        output: output.ok_or_else(|| missing("--output"))?,
        namespace: namespace.ok_or_else(|| missing("--namespace"))?,
        source_revision: source_revision.ok_or_else(|| missing("--source-revision"))?,
    })
}

/// The value following a flag that needs one.
fn value(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<String, ParseError> {
    args.next()
        .ok_or_else(|| ParseError::Malformed(format!("{flag} needs a value")))
}

/// The malformed-invocation error for a flag that was never given.
fn missing(flag: &str) -> ParseError {
    ParseError::Malformed(format!("{flag} is required"))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{Args, ParseError, parse_args};

    fn parse(values: &[&str]) -> Result<Args, ParseError> {
        parse_args(values.iter().map(|value| (*value).to_owned()))
    }

    fn malformed_error(values: &[&str]) -> String {
        match parse(values) {
            Err(ParseError::Malformed(message)) => message,
            other => panic!("{values:?} must be malformed, got {other:?}"),
        }
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
    fn help_is_requested_with_the_help_flag() {
        assert!(matches!(parse(&["--help"]), Err(ParseError::Help)));
        assert!(matches!(
            parse(&["--input", "dataset", "--help"]),
            Err(ParseError::Help)
        ));
    }

    #[test]
    fn flags_are_required() {
        let error = malformed_error(&["--input", "dataset"]);

        assert!(error.contains("--output"), "{error}");
    }

    #[test]
    fn a_flag_without_a_value_fails() {
        let error = malformed_error(&["--input"]);

        assert!(error.contains("--input"), "{error}");
    }

    #[test]
    fn an_unknown_flag_fails() {
        let error = malformed_error(&["--verbose", "x"]);

        assert!(error.contains("--verbose"), "{error}");
    }
}
