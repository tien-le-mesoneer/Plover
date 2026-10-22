//! Which interpretation each block name gets, following Bruno's grammar.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockKind {
    Dict,
    AssertDict,
    Text,
    List,
    Opaque,
}

const TEXT_BLOCKS: &[&str] = &[
    "body",
    "body:json",
    "body:text",
    "body:xml",
    "body:sparql",
    "body:graphql",
    "body:graphql:vars",
    "tests",
    "docs",
];

const DICT_BLOCKS: &[&str] = &[
    "meta",
    "get",
    "post",
    "put",
    "delete",
    "patch",
    "options",
    "head",
    "connect",
    "trace",
    "http",
    "grpc",
    "ws",
    "query",
    "params:query",
    "params:path",
    "headers",
    "metadata",
    "auth",
    "body:form-urlencoded",
    "body:multipart-form",
    "body:file",
    "body:grpc",
    "body:ws",
    "vars",
    "vars:pre-request",
    "vars:post-response",
    "settings",
    "app",
];

/// `raw_opener` is the character that opened the block: `{`, `[` or `:`.
pub fn block_kind(name: &str, raw_opener: char) -> BlockKind {
    match raw_opener {
        '[' => return BlockKind::List,
        '{' => {}
        _ => return BlockKind::Opaque,
    }
    if name == "assert" {
        BlockKind::AssertDict
    } else if TEXT_BLOCKS.contains(&name) || name.starts_with("script:") {
        BlockKind::Text
    } else if DICT_BLOCKS.contains(&name)
        || name.starts_with("auth:")
        || name.starts_with("vars:externalsecrets:")
    {
        BlockKind::Dict
    } else {
        BlockKind::Opaque
    }
}
