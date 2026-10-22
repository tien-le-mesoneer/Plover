//! Canonical Bruno rendering of a single block (mirrors `jsonToBru.js`).

use super::syntax::{Annotation, BlockContent, Pair, PairValue};

/// Renders `name` with `content` in Bruno's canonical form, without a trailing newline.
pub fn render_block(name: &str, content: &BlockContent) -> String {
    match content {
        BlockContent::Dict(pairs) => {
            let quote_keys = name != "assert";
            let mut out = format!("{name} {{\n");
            for pair in pairs {
                render_pair(&mut out, pair, quote_keys);
            }
            out.push('}');
            out
        }
        BlockContent::Text(text) if text.is_empty() => format!("{name} {{\n\n}}"),
        BlockContent::Text(text) => format!("{name} {{\n{}\n}}", indent(text, "  ")),
        BlockContent::List(items) if items.is_empty() => format!("{name} [\n]"),
        BlockContent::List(items) => {
            let body: Vec<String> = items.iter().map(|i| format!("  {i}")).collect();
            format!("{name} [\n{}\n]", body.join(",\n"))
        }
        BlockContent::Opaque => format!("{name} {{\n}}"),
    }
}

fn render_pair(out: &mut String, pair: &Pair, quote_keys: bool) {
    for annotation in &pair.annotations {
        out.push_str("  ");
        out.push_str(&render_annotation(annotation));
        out.push('\n');
    }
    out.push_str("  ");
    if !pair.enabled {
        out.push('~');
    }
    out.push_str(&if quote_keys {
        key_string(&pair.key)
    } else {
        pair.key.clone()
    });
    out.push_str(": ");
    match &pair.value {
        PairValue::Single(v) if v.contains('\n') => render_multiline(out, v, None),
        PairValue::Single(v) => out.push_str(v),
        PairValue::Multiline { text, content_type } => {
            render_multiline(out, text, content_type.as_deref())
        }
        PairValue::List(items) => {
            out.push_str("[\n");
            for item in items {
                out.push_str(&format!("    {item}\n"));
            }
            if items.is_empty() {
                out.push('\n');
            }
            out.push_str("  ]");
        }
    }
    out.push('\n');
}

fn render_multiline(out: &mut String, text: &str, content_type: Option<&str>) {
    out.push_str("'''\n");
    out.push_str(&indent(text, "    "));
    out.push_str("\n  '''");
    if let Some(ct) = content_type {
        out.push_str(&format!(" @contentType({ct})"));
    }
}

/// Bruno `indentString`: every line, including empty ones, gets the prefix.
fn indent(text: &str, prefix: &str) -> String {
    text.split('\n')
        .map(|l| format!("{prefix}{l}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Bruno `getKeyString`, plus quoting a leading `~` so it is not read as "disabled".
fn key_string(key: &str) -> String {
    if key.contains([':', '"', '{', '}', ' ', '\t']) || key.starts_with('~') {
        format!("\"{}\"", key.replace('"', "\\\""))
    } else {
        key.to_owned()
    }
}

/// Bruno `serializeAnnotations` for one annotation.
fn render_annotation(a: &Annotation) -> String {
    match &a.arg {
        None => format!("@{}", a.name),
        Some(arg) if arg.contains('\n') => {
            format!("@{}('''\n{}\n  ''')", a.name, indent(arg, "    "))
        }
        Some(arg) if arg.contains('\'') => {
            format!(
                "@{}(\"{}\")",
                a.name,
                arg.replace('\\', "\\\\").replace('"', "\\\"")
            )
        }
        Some(arg) => format!("@{}('{arg}')", a.name),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bru::syntax::BruDocument;

    const REQUEST: &str = include_str!("../../../../fixtures/bruno-lang/request.bru");

    fn line_changes(a: &str, b: &str) -> (usize, usize) {
        let diff = similar::TextDiff::from_lines(a, b);
        let mut removed = 0;
        let mut added = 0;
        for change in diff.iter_all_changes() {
            match change.tag() {
                similar::ChangeTag::Delete => removed += 1,
                similar::ChangeTag::Insert => added += 1,
                similar::ChangeTag::Equal => {}
            }
        }
        (removed, added)
    }

    #[test]
    fn unchanged_set_block_is_noop() {
        let mut doc = BruDocument::parse(REQUEST).unwrap();
        let headers = doc.block("headers").unwrap().content().clone();
        doc.set_block("headers", headers);
        assert_eq!(doc.write(), REQUEST);
    }

    #[test]
    fn edit_one_header_minimal_diff() {
        let mut doc = BruDocument::parse(REQUEST).unwrap();
        let mut headers = doc.block("headers").unwrap().content().clone();
        if let BlockContent::Dict(pairs) = &mut headers {
            let ct = pairs.iter_mut().find(|p| p.key == "content-type").unwrap();
            ct.value = PairValue::Single("text/plain".into());
        }
        doc.set_block("headers", headers);
        assert_eq!(line_changes(REQUEST, &doc.write()), (1, 1));
    }

    #[test]
    fn append_new_block() {
        let mut doc = BruDocument::parse("meta {\n  name: a\n}\n").unwrap();
        doc.set_block("docs", BlockContent::Text("hi".into()));
        assert_eq!(doc.write(), "meta {\n  name: a\n}\n\ndocs {\n  hi\n}\n");
    }

    #[test]
    fn append_keeps_crlf() {
        let mut doc = BruDocument::parse("meta {\r\n  name: a\r\n}\r\n").unwrap();
        doc.set_block("docs", BlockContent::Text("hi".into()));
        assert_eq!(
            doc.write(),
            "meta {\r\n  name: a\r\n}\r\n\r\ndocs {\r\n  hi\r\n}\r\n"
        );
    }

    #[test]
    fn remove_block_drops_separator() {
        let mut doc = BruDocument::parse("meta {\n  name: a\n}\n\ndocs {\n  hi\n}\n").unwrap();
        doc.remove_block("docs");
        assert_eq!(doc.write(), "meta {\n  name: a\n}\n");
        let mut doc = BruDocument::parse("meta {\n  name: a\n}\n\ndocs {\n  hi\n}\n").unwrap();
        doc.remove_block("meta");
        assert_eq!(doc.write(), "docs {\n  hi\n}\n");
    }

    #[test]
    fn renders_quoted_disabled_annotated_and_multiline() {
        let content = BlockContent::Dict(vec![
            Pair {
                key: "a:b".into(),
                value: PairValue::Single("1".into()),
                enabled: false,
                annotations: vec![Annotation {
                    name: "description".into(),
                    arg: Some("x\ny".into()),
                }],
            },
            Pair {
                key: "obj".into(),
                value: PairValue::Multiline {
                    text: "{\n  \"a\": 1\n}".into(),
                    content_type: Some("application/json".into()),
                },
                enabled: true,
                annotations: vec![],
            },
        ]);
        let text = render_block("vars", &content);
        let doc = BruDocument::parse(&text).unwrap();
        assert_eq!(*doc.block("vars").unwrap().content(), content, "{text}");
    }
}
