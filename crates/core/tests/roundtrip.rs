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
            let known = !matches!(block_kind(&block.name, opener), Ok(BlockKind::Opaque));
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

#[test]
fn render_parse_is_identity_on_content() {
    let mut offenders = Vec::new();
    for path in all_fixtures() {
        let src = std::fs::read_to_string(&path).expect("read fixture");
        let doc = BruDocument::parse(&src).expect("parse");
        for block in doc.blocks() {
            if *block.content() == BlockContent::Opaque {
                continue;
            }
            let rendered = brs_core::bru::render::render_block(&block.name, block.content());
            let reparsed = BruDocument::parse(&rendered);
            let same = reparsed
                .as_ref()
                .ok()
                .and_then(|d| d.block(&block.name))
                .is_some_and(|b| b.content() == block.content());
            if !same {
                offenders.push(format!("{}: {}\n{rendered}", path.display(), block.name));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "render/parse mismatch:\n{}",
        offenders.join("\n---\n")
    );
}

#[test]
fn typed_files_roundtrip_unchanged() {
    use brs_core::bru::model::{EnvironmentFile, RequestFile, ScopeFile};
    let mut failures = Vec::new();
    for path in all_fixtures() {
        let src = std::fs::read_to_string(&path).expect("read fixture");
        let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let in_env_dir = path
            .parent()
            .and_then(|p| p.file_name())
            .is_some_and(|n| n == "environments");
        let out = if in_env_dir {
            EnvironmentFile::parse(&src).map(|f| f.to_bru())
        } else if file_name == "collection.bru" || file_name == "folder.bru" {
            ScopeFile::parse(&src).map(|f| f.to_bru())
        } else {
            match RequestFile::parse(&src) {
                Ok(f) => Ok(f.to_bru()),
                Err(brs_core::bru::model::ModelError::MissingHttpBlock) => continue,
                Err(e) => Err(e),
            }
        };
        match out {
            Ok(out) if out == src => {}
            Ok(_) => failures.push(format!("{}: output differs", path.display())),
            Err(e) => failures.push(format!("{}: {e}", path.display())),
        }
    }
    assert!(
        failures.is_empty(),
        "typed round-trip failures:\n{}",
        failures.join("\n")
    );
}
