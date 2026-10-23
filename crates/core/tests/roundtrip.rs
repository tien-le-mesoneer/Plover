use std::path::{Path, PathBuf};

use plover_core::bru::kind::{BlockKind, block_kind};
use plover_core::bru::syntax::{BlockContent, BruDocument};

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
            let rendered = plover_core::bru::render::render_block(&block.name, block.content());
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
    use plover_core::bru::model::{EnvironmentFile, RequestFile, ScopeFile};
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
                Err(plover_core::bru::model::ModelError::MissingHttpBlock) => continue,
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

fn changed_lines(before: &str, after: &str) -> (usize, usize) {
    let diff = similar::TextDiff::from_lines(before, after);
    let mut counts = (0, 0);
    for change in diff.iter_all_changes() {
        match change.tag() {
            similar::ChangeTag::Delete => counts.0 += 1,
            similar::ChangeTag::Insert => counts.1 += 1,
            similar::ChangeTag::Equal => {}
        }
    }
    counts
}

/// Editing one line of any text block (scripts, tests, docs, bodies) changes exactly
/// that line in the file: untouched lines keep their bytes, including blank lines
/// without indentation and tab indentation.
#[test]
fn one_line_text_block_edit_is_a_one_line_diff_in_all_fixtures() {
    let mut checked = 0;
    let mut failures = Vec::new();
    for path in all_fixtures() {
        let src = std::fs::read_to_string(&path).unwrap();
        let doc = BruDocument::parse(&src).unwrap();
        let texts: Vec<(String, String)> = doc
            .blocks()
            .filter_map(|b| match b.content() {
                BlockContent::Text(t) if !t.is_empty() => Some((b.name.clone(), t.clone())),
                _ => None,
            })
            .collect();
        for (name, text) in texts {
            // Skip a later duplicate name: set_block edits the first block with the name.
            if doc.block(&name).and_then(|b| b.content().text()) != Some(text.as_str()) {
                continue;
            }
            let mut lines: Vec<&str> = text.split('\n').collect();
            let Some(i) = lines.iter().position(|l| !l.trim().is_empty()) else {
                continue;
            };
            let edited_line = format!("{} // edited", lines[i]);
            lines[i] = &edited_line;
            let mut edited = doc.clone();
            edited.set_block(&name, BlockContent::Text(lines.join("\n")));
            let out = edited.write();
            checked += 1;
            if changed_lines(&src, &out) != (1, 1) {
                failures.push(format!(
                    "{} [{name}]: {:?}",
                    path.display(),
                    changed_lines(&src, &out)
                ));
            }
        }
    }
    assert!(checked > 100, "only {checked} text blocks checked");
    assert!(
        failures.is_empty(),
        "{} of {checked} edits were not 1/1:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn text_block_edit_keeps_blank_lines_and_tabs() {
    let src = "tests {\n  test('a', () => {\n\n  \tconst x = 1;\n\t// tab only\n  });\n\n  test('b', () => {});\n}\n";
    let doc = BruDocument::parse(src).unwrap();
    let text = doc
        .block("tests")
        .unwrap()
        .content()
        .text()
        .unwrap()
        .to_owned();
    let mut edited = doc.clone();
    edited.set_block(
        "tests",
        BlockContent::Text(text.replace("test('b'", "test('renamed'")),
    );
    let out = edited.write();
    assert_eq!(out, src.replace("test('b'", "test('renamed'"));
    // CRLF files keep CRLF.
    let crlf = src.replace('\n', "\r\n");
    let doc = BruDocument::parse(&crlf).unwrap();
    let mut edited = doc.clone();
    edited.set_block(
        "tests",
        BlockContent::Text(text.replace("test('b'", "test('renamed'")),
    );
    assert_eq!(edited.write(), crlf.replace("test('b'", "test('renamed'"));
    // Inserted lines are indented like Bruno writes them.
    let mut edited = doc.clone();
    edited.set_block("tests", BlockContent::Text(format!("{text}\nnew line")));
    assert!(
        edited
            .write()
            .ends_with("  test('b', () => {});\r\n  new line\r\n}\r\n"),
        "{:?}",
        edited.write()
    );
}
