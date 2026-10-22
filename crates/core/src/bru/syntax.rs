//! Lossless syntax layer: a `.bru` file as a sequence of blocks and the
//! whitespace between them. Every block keeps its exact source text, so
//! writing an unmodified document reproduces the input byte for byte.

use pest::Parser;
use pest::iterators::Pair as PestPair;
use pest_derive::Parser;

use super::kind::{BlockKind, block_kind};
use super::render::render_block;

#[derive(Parser)]
#[grammar = "bru/grammar.pest"]
pub(crate) struct BruParser;

/// A parsed `.bru` file.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct BruDocument {
    segments: Vec<Segment>,
}

#[derive(Debug, Clone, PartialEq)]
enum Segment {
    Trivia(String),
    Block(Block),
}

/// One top-level block such as `meta { ... }` or `body:json { ... }`.
#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub name: String,
    raw: String,
    content: BlockContent,
}

/// Interpreted block content. `Opaque` blocks are preserved but not modelled.
#[derive(Debug, Clone, PartialEq)]
pub enum BlockContent {
    Dict(Vec<Pair>),
    Text(String),
    List(Vec<String>),
    Opaque,
}

/// A `key: value` entry of a dictionary block.
#[derive(Debug, Clone, PartialEq)]
pub struct Pair {
    pub key: String,
    pub value: PairValue,
    pub enabled: bool,
    pub annotations: Vec<Annotation>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PairValue {
    Single(String),
    Multiline {
        text: String,
        content_type: Option<String>,
    },
    List(Vec<String>),
}

/// `@name` or `@name(arg)` written on the line(s) before a pair.
#[derive(Debug, Clone, PartialEq)]
pub struct Annotation {
    pub name: String,
    pub arg: Option<String>,
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[error("{line}:{column}: {message}")]
pub struct SyntaxError {
    pub line: usize,
    pub column: usize,
    pub message: String,
}

impl Block {
    /// The block's exact source text.
    pub fn raw(&self) -> &str {
        &self.raw
    }

    pub fn content(&self) -> &BlockContent {
        &self.content
    }
}

impl BlockContent {
    /// Dictionary pairs; empty for non-dictionary content.
    pub fn pairs(&self) -> &[Pair] {
        match self {
            BlockContent::Dict(pairs) => pairs,
            _ => &[],
        }
    }

    pub fn text(&self) -> Option<&str> {
        match self {
            BlockContent::Text(t) => Some(t),
            _ => None,
        }
    }
}

impl PairValue {
    /// The value as text; list values are joined with `\n`.
    pub fn as_text(&self) -> String {
        match self {
            PairValue::Single(s) => s.clone(),
            PairValue::Multiline { text, .. } => text.clone(),
            PairValue::List(items) => items.join("\n"),
        }
    }
}

impl BruDocument {
    pub fn new() -> BruDocument {
        BruDocument::default()
    }

    pub fn parse(src: &str) -> Result<BruDocument, SyntaxError> {
        let mut pairs = BruParser::parse(Rule::file, src).map_err(|e| syntax_error(src, e))?;
        let file = pairs.next().ok_or_else(|| SyntaxError {
            line: 1,
            column: 1,
            message: "empty parse".into(),
        })?;
        let mut segments = Vec::new();
        for part in file.into_inner() {
            match part.as_rule() {
                Rule::bom | Rule::trivia => {
                    segments.push(Segment::Trivia(part.as_str().to_owned()))
                }
                Rule::block => segments.push(Segment::Block(block_from_pest(part))),
                _ => {}
            }
        }
        Ok(BruDocument { segments })
    }

    pub fn write(&self) -> String {
        let mut out = String::new();
        for segment in &self.segments {
            match segment {
                Segment::Trivia(t) => out.push_str(t),
                Segment::Block(b) => out.push_str(&b.raw),
            }
        }
        out
    }

    pub fn blocks(&self) -> impl Iterator<Item = &Block> {
        self.segments.iter().filter_map(|s| match s {
            Segment::Block(b) => Some(b),
            Segment::Trivia(_) => None,
        })
    }

    /// First block with this name.
    pub fn block(&self, name: &str) -> Option<&Block> {
        self.blocks().find(|b| b.name == name)
    }

    /// Sets the first block named `name`. A block whose content already
    /// equals `content` keeps its source text; a changed block is rendered
    /// canonically; a missing block is appended after the last block.
    pub fn set_block(&mut self, name: &str, content: BlockContent) {
        let crlf = self.uses_crlf();
        let render = |content: &BlockContent| {
            let text = render_block(name, content);
            if crlf {
                text.replace('\n', "\r\n")
            } else {
                text
            }
        };
        if let Some(block) = self.block_mut(name) {
            if block.content != content {
                block.raw = render(&content);
                block.content = content;
            }
            return;
        }
        let new = Segment::Block(Block {
            name: name.to_owned(),
            raw: render(&content),
            content,
        });
        let newline = if crlf { "\r\n" } else { "\n" };
        match self
            .segments
            .iter()
            .rposition(|s| matches!(s, Segment::Block(_)))
        {
            Some(last) => {
                self.segments
                    .insert(last + 1, Segment::Trivia(newline.repeat(2)));
                self.segments.insert(last + 2, new);
            }
            None => {
                self.segments.push(new);
                self.segments.push(Segment::Trivia(newline.to_owned()));
            }
        }
    }

    /// Removes the first block named `name` and the whitespace separating it from its neighbour.
    pub fn remove_block(&mut self, name: &str) {
        let Some(index) = self
            .segments
            .iter()
            .position(|s| matches!(s, Segment::Block(b) if b.name == name))
        else {
            return;
        };
        let before = index
            .checked_sub(1)
            .filter(|i| matches!(self.segments[*i], Segment::Trivia(_)));
        let after =
            Some(index + 1).filter(|i| matches!(self.segments.get(*i), Some(Segment::Trivia(_))));
        self.segments.remove(index);
        if let Some(i) = before.filter(|i| *i > 0).or(after.map(|i| i - 1)) {
            self.segments.remove(i);
        }
    }

    /// Renames the first block named `old`, editing only the name in its source text.
    pub fn rename_block(&mut self, old: &str, new: &str) {
        if let Some(block) = self.block_mut(old) {
            block.raw = format!("{new}{}", &block.raw[old.len()..]);
            block.name = new.to_owned();
        }
    }

    fn block_mut(&mut self, name: &str) -> Option<&mut Block> {
        self.segments.iter_mut().find_map(|s| match s {
            Segment::Block(b) if b.name == name => Some(b),
            _ => None,
        })
    }

    fn uses_crlf(&self) -> bool {
        let text = self.write();
        text.find('\n')
            .is_some_and(|i| i > 0 && text.as_bytes()[i - 1] == b'\r')
    }
}

fn block_from_pest(block: PestPair<'_, Rule>) -> Block {
    let raw = block.as_str().to_owned();
    let Some(form) = block.into_inner().next() else {
        return Block {
            name: String::new(),
            raw,
            content: BlockContent::Opaque,
        };
    };
    let opener = match form.as_rule() {
        Rule::brace_block => '{',
        Rule::bracket_block => '[',
        _ => ':',
    };
    let mut parts = form.into_inner();
    let name = parts
        .next()
        .map(|n| n.as_str().to_owned())
        .unwrap_or_default();
    let body = parts.next().map(|b| b.as_str()).unwrap_or("");
    let content = parse_content(&name, opener, body);
    Block { name, raw, content }
}

pub(crate) fn parse_content(name: &str, opener: char, body: &str) -> BlockContent {
    let parsed = match block_kind(name, opener) {
        BlockKind::Dict => parse_dict(body, Rule::dict),
        BlockKind::AssertDict => parse_dict(body, Rule::assert_dict),
        BlockKind::Text => Some(BlockContent::Text(parse_text(body))),
        BlockKind::List => parse_list(body),
        BlockKind::Opaque => Some(BlockContent::Opaque),
    };
    parsed.unwrap_or_else(|| {
        tracing::warn!(
            block = name,
            "block content not understood; preserving it verbatim"
        );
        BlockContent::Opaque
    })
}

fn parse_dict(body: &str, rule: Rule) -> Option<BlockContent> {
    let dict = BruParser::parse(rule, body).ok()?.next()?;
    let pairs = dict
        .into_inner()
        .filter(|p| matches!(p.as_rule(), Rule::pair | Rule::assert_pair))
        .map(pair_from_pest)
        .collect();
    Some(BlockContent::Dict(pairs))
}

fn pair_from_pest(pair: PestPair<'_, Rule>) -> Pair {
    let mut key = String::new();
    let mut enabled = true;
    let mut annotations = Vec::new();
    let mut value = PairValue::Single(String::new());
    for part in pair.into_inner() {
        match part.as_rule() {
            Rule::annotations => {
                annotations = part.into_inner().map(annotation_from_pest).collect()
            }
            Rule::disabled => enabled = false,
            Rule::quoted_key => {
                let inner = part.into_inner().next().map(|i| i.as_str()).unwrap_or("");
                key = inner.replace("\\\"", "\"");
            }
            Rule::plain_key | Rule::assert_key => key = part.as_str().trim().to_owned(),
            Rule::list_value => {
                value = PairValue::List(part.into_inner().map(|w| w.as_str().to_owned()).collect());
            }
            Rule::multiline_value => {
                let mut text = String::new();
                let mut content_type = None;
                for m in part.into_inner() {
                    match m.as_rule() {
                        Rule::ml_content => text = parse_multiline_value(m.as_str()),
                        Rule::content_type => {
                            content_type = m.into_inner().next().map(|c| c.as_str().to_owned());
                        }
                        _ => {}
                    }
                }
                value = PairValue::Multiline { text, content_type };
            }
            Rule::single_value => value = PairValue::Single(part.as_str().trim().to_owned()),
            _ => {}
        }
    }
    Pair {
        key,
        value,
        enabled,
        annotations,
    }
}

fn annotation_from_pest(annotation: PestPair<'_, Rule>) -> Annotation {
    let mut parts = annotation.into_inner();
    let name = parts
        .next()
        .map(|n| n.as_str().to_owned())
        .unwrap_or_default();
    let arg = parts.next().map(|arg| {
        let inner = arg
            .clone()
            .into_inner()
            .next()
            .map(|i| i.as_str())
            .unwrap_or("");
        match arg.as_rule() {
            Rule::ann_multi => dedent_annotation_block(inner),
            Rule::ann_sq => inner.to_owned(),
            Rule::ann_dq => unescape_double_quoted(inner),
            _ => arg.as_str().to_owned(),
        }
    });
    Annotation { name, arg }
}

/// Bruno `multilinetextblock`: drop the first 4 characters of every line, then trim.
fn parse_multiline_value(content: &str) -> String {
    let lines: Vec<String> = content
        .split('\n')
        .map(|line| {
            line.strip_suffix('\r')
                .unwrap_or(line)
                .chars()
                .skip(4)
                .collect()
        })
        .collect();
    lines.join("\n").trim().to_owned()
}

/// Bruno `parseAnnotationMultilineTextBlock`: drop the blank first/last line, remove common indent.
fn dedent_annotation_block(content: &str) -> String {
    if !content.contains(['\n', '\r']) {
        return content.to_owned();
    }
    let mut lines: Vec<&str> = content
        .split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .collect();
    if lines.first() == Some(&"") {
        lines.remove(0);
    }
    if lines.last().is_some_and(|l| l.trim().is_empty()) {
        lines.pop();
    }
    let indent = lines
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start_matches([' ', '\t']).len())
        .min()
        .unwrap_or(0);
    lines
        .iter()
        .map(|l| {
            if l.trim().is_empty() {
                ""
            } else {
                &l[indent..]
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn unescape_double_quoted(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('r') => out.push('\r'),
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some(other @ ('"' | '\\')) => out.push(other),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// Bruno text blocks: leading newlines dropped, then two spaces of indent removed per line.
fn parse_text(body: &str) -> String {
    body.trim_start_matches(['\r', '\n'])
        .split('\n')
        .map(|line| {
            let line = line.strip_suffix('\r').unwrap_or(line);
            line.strip_prefix("  ").unwrap_or(line)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// `[ a, b ]` or one item per line. Annotated or quoted items are not modelled.
fn parse_list(body: &str) -> Option<BlockContent> {
    if body.contains(['@', '"']) {
        return None;
    }
    let items = body
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect();
    Some(BlockContent::List(items))
}

fn syntax_error(src: &str, e: pest::error::Error<Rule>) -> SyntaxError {
    let (line, column) = match e.line_col {
        pest::error::LineColLocation::Pos(p) | pest::error::LineColLocation::Span(p, _) => p,
    };
    let offset = match e.location {
        pest::error::InputLocation::Pos(p) | pest::error::InputLocation::Span((p, _)) => p,
    };
    let rest = src.get(offset..).unwrap_or("");
    let name: String = rest
        .chars()
        .take_while(|c| !c.is_whitespace() && *c != '{' && *c != '[')
        .collect();
    let message = if name.is_empty() {
        "expected a block".to_owned()
    } else {
        format!("unterminated or malformed block `{name}`")
    };
    SyntaxError {
        line,
        column,
        message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(doc: &BruDocument) -> Vec<&str> {
        doc.blocks().map(|b| b.name.as_str()).collect()
    }

    #[test]
    fn roundtrip_simple() {
        let src = "meta {\n  name: a\n  seq: 1\n}\n\nget {\n  url: http://x\n}\n";
        let doc = BruDocument::parse(src).unwrap();
        assert_eq!(doc.write(), src);
        assert_eq!(names(&doc), ["meta", "get"]);
    }

    #[test]
    fn crlf_bom_and_no_trailing_newline() {
        let src = "\u{feff}meta {\r\n  name: a\r\n}";
        let doc = BruDocument::parse(src).unwrap();
        assert_eq!(doc.write(), src);
        assert_eq!(names(&doc), ["meta"]);
    }

    #[test]
    fn unknown_block_preserved() {
        let src = "grpc {\n  url: x\n}\n\nfuture:thing {\n  anything goes\n}\n";
        let doc = BruDocument::parse(src).unwrap();
        assert_eq!(doc.write(), src);
        assert_eq!(names(&doc), ["grpc", "future:thing"]);
    }

    #[test]
    fn text_block_with_braces() {
        let src = "body:json {\n  {\n    \"a\": {}\n  }\n}\n";
        let doc = BruDocument::parse(src).unwrap();
        assert_eq!(names(&doc), ["body:json"]);
        assert_eq!(doc.write(), src);
    }

    #[test]
    fn list_block() {
        let src = "vars:secret [\n  token\n]\n";
        let doc = BruDocument::parse(src).unwrap();
        assert_eq!(names(&doc), ["vars:secret"]);
        assert_eq!(doc.write(), src);
    }

    #[test]
    fn env_line_entries() {
        let src = "color: #ff0000\nvars {\n  a: 1\n}\n";
        let doc = BruDocument::parse(src).unwrap();
        assert_eq!(names(&doc), ["color", "vars"]);
        assert_eq!(doc.write(), src);
    }

    #[test]
    fn syntax_error_has_position() {
        let err = BruDocument::parse("meta {\n  name: a\n").unwrap_err();
        assert!(err.line >= 1 && err.column >= 1);
        assert!(err.message.contains("meta"), "{}", err.message);
    }

    const REQUEST: &str = include_str!("../../../../fixtures/bruno-lang/request.bru");
    const COLLECTION: &str =
        include_str!("../../../../fixtures/bruno-tests/collection/collection.bru");

    fn kv(p: &Pair) -> (&str, String, bool) {
        (p.key.as_str(), p.value.as_text(), p.enabled)
    }

    #[test]
    fn query_params_quoted_and_disabled() {
        let doc = BruDocument::parse(REQUEST).unwrap();
        let pairs: Vec<_> = doc
            .block("params:query")
            .unwrap()
            .content()
            .pairs()
            .iter()
            .map(kv)
            .collect();
        assert!(pairs.contains(&("key with spaces", "is allowed".into(), true)));
        assert!(pairs.contains(&("nested escaped \"quote\"", "is allowed".into(), true)));
        assert!(pairs.contains(&("disabled:colon:parameter", "is allowed".into(), false)));
        assert!(pairs.contains(&("message", "hello".into(), false)));
    }

    #[test]
    fn meta_tags_list() {
        let doc = BruDocument::parse(REQUEST).unwrap();
        let tags = doc
            .block("meta")
            .unwrap()
            .content()
            .pairs()
            .iter()
            .find(|p| p.key == "tags")
            .unwrap();
        assert_eq!(
            tags.value,
            PairValue::List(vec!["foo".into(), "bar".into()])
        );
    }

    #[test]
    fn annotations_on_headers() {
        let doc = BruDocument::parse(COLLECTION).unwrap();
        let first = &doc.block("headers").unwrap().content().pairs()[0];
        assert_eq!(first.key, "check");
        assert_eq!(
            first.annotations,
            [Annotation {
                name: "description".into(),
                arg: Some("asd".into())
            }]
        );
        let token = &doc.block("headers").unwrap().content().pairs()[1];
        assert_eq!(token.annotations[0].arg.as_deref(), Some("asd\nasd\nasd"));
        let vars = doc.block("vars:pre-request").unwrap().content().pairs();
        let num = vars.iter().find(|p| p.key == "coll_num").unwrap();
        assert_eq!(
            num.annotations,
            [Annotation {
                name: "number".into(),
                arg: None
            }]
        );
    }

    #[test]
    fn multiline_value() {
        let doc = BruDocument::parse(COLLECTION).unwrap();
        let vars = doc.block("vars:pre-request").unwrap().content().pairs();
        let obj = vars.iter().find(|p| p.key == "coll_obj").unwrap();
        assert_eq!(
            obj.value,
            PairValue::Multiline {
                text: "{\"scope\":\"collection\"}".into(),
                content_type: None
            }
        );
    }

    #[test]
    fn json_body_outdented() {
        let doc = BruDocument::parse("body:json {\n  {\n    \"a\": 1\n  }\n}\n").unwrap();
        assert_eq!(
            doc.block("body:json").unwrap().content().text(),
            Some("{\n  \"a\": 1\n}")
        );
    }

    #[test]
    fn assert_keys_with_spaces() {
        let doc =
            BruDocument::parse("assert {\n  res.status: eq 200\n  res.body.length: gt 0\n}\n")
                .unwrap();
        let pairs: Vec<_> = doc
            .block("assert")
            .unwrap()
            .content()
            .pairs()
            .iter()
            .map(kv)
            .collect();
        assert_eq!(
            pairs,
            [
                ("res.status", "eq 200".into(), true),
                ("res.body.length", "gt 0".into(), true)
            ]
        );
    }

    #[test]
    fn secret_list_block() {
        let doc = BruDocument::parse("vars:secret [\n  token,\n  other\n]\n").unwrap();
        assert_eq!(
            *doc.block("vars:secret").unwrap().content(),
            BlockContent::List(vec!["token".into(), "other".into()])
        );
    }

    #[test]
    fn empty_dict_block() {
        let doc = BruDocument::parse("headers {\n}\n").unwrap();
        assert_eq!(
            *doc.block("headers").unwrap().content(),
            BlockContent::Dict(vec![])
        );
    }
}
