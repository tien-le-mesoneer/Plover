use std::path::{Path, PathBuf};

use brs_core::bru::kind::{BlockKind, block_kind};
use brs_core::bru::syntax::{BlockContent, BruDocument};

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

fn bru_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("read fixtures dir") {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            bru_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "bru") {
            out.push(path);
        }
    }
}

fn all_fixtures() -> Vec<PathBuf> {
    let mut files = Vec::new();
    bru_files(&fixtures_dir(), &mut files);
    files.sort();
    files
}

#[test]
fn all_fixtures_roundtrip_byte_identical() {
    let files = all_fixtures();
    let mut failures = Vec::new();
    for path in &files {
        let src = std::fs::read_to_string(path).expect("read fixture");
        match BruDocument::parse(&src) {
            Ok(doc) if doc.write() == src => {}
            Ok(_) => failures.push(format!("{}: output differs", path.display())),
            Err(e) => failures.push(format!("{}: {e}", path.display())),
        }
    }
    assert!(
        failures.is_empty(),
        "round-trip failures:\n{}",
        failures.join("\n")
    );
    assert!(
        files.len() >= 280,
        "only {} fixture files checked",
        files.len()
    );
}

#[test]
fn all_fixtures_have_no_unexpected_opaque() {
    let mut offenders = Vec::new();
    for path in all_fixtures() {
        let src = std::fs::read_to_string(&path).expect("read fixture");
        let doc = BruDocument::parse(&src).expect("parse");
        for block in doc.blocks() {
            let opener = if block.raw().trim_end().ends_with(']')
                && !block.raw().trim_end().ends_with('}')
            {
                '['
            } else {
                '{'
            };
            let known = block_kind(&block.name, opener) != BlockKind::Opaque;
            if known && *block.content() == BlockContent::Opaque {
                offenders.push(format!("{}: {}", path.display(), block.name));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "unparsed known blocks:\n{}",
        offenders.join("\n")
    );
}
