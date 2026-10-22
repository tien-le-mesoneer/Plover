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

/// Bracket blocks Bruno parses as arrays of names (envToJson.js `secretvars`).
const LIST_BLOCKS: &[&str] = &["vars:secret"];

/// How a block is interpreted. `raw_opener` is the character that opened it:
/// `{`, `[` or `:`. `Err` means a block name Bruno knows, written in a form its
/// grammar rejects; Bruno fails the whole file in that case.
pub fn block_kind(name: &str, raw_opener: char) -> Result<BlockKind, String> {
    let known = if name == "assert" {
        Some(('{', BlockKind::AssertDict))
    } else if TEXT_BLOCKS.contains(&name) || name.starts_with("script:") {
        Some(('{', BlockKind::Text))
    } else if DICT_BLOCKS.contains(&name)
        || name.starts_with("auth:")
        || name.starts_with("vars:externalsecrets:")
    {
        Some(('{', BlockKind::Dict))
    } else if LIST_BLOCKS.contains(&name) {
        Some(('[', BlockKind::List))
    } else {
        None
    };
    match known {
        Some((opener, kind)) if opener == raw_opener => Ok(kind),
        Some((opener, _)) => Err(format!("block `{name}` must be opened with `{opener}`")),
        None => Ok(BlockKind::Opaque),
    }
}
