//! Lossless syntax layer: a `.bru` file as a sequence of blocks and the
//! whitespace between them. Every block keeps its exact source text, so
//! writing an unmodified document reproduces the input byte for byte.

use pest::Parser;
use pest::iterators::Pair as PestPair;
use pest_derive::Parser;

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
    Multiline { text: String, content_type: Option<String> },
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
                Rule::bom | Rule::trivia => segments.push(Segment::Trivia(part.as_str().to_owned())),
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
}

fn block_from_pest(block: PestPair<'_, Rule>) -> Block {
    let raw = block.as_str().to_owned();
    let inner = block.into_inner().next();
    let name = inner
        .and_then(|b| b.into_inner().next())
        .map(|n| n.as_str().to_owned())
        .unwrap_or_default();
    Block { name, raw, content: BlockContent::Opaque }
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
    SyntaxError { line, column, message }
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
}
