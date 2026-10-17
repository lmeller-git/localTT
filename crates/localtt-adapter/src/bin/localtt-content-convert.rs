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
//! success prints the per-kind counts, failure exits non-zero.

fn main() {
    todo!()
}
