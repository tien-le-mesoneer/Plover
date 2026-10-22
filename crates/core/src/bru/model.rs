//! Typed layer: maps `.bru` blocks to the request model and writes model
//! changes back by touching only the blocks whose meaning changed.

use super::syntax::{BlockContent, BruDocument, Pair, PairValue, SyntaxError};
use crate::request::{
    Auth, Body, HttpRequest, KeyValue, Method, MultipartField, MultipartValue, RequestSettings,
};

#[derive(Debug, thiserror::Error)]
pub enum ModelError {
    #[error(transparent)]
    Syntax(#[from] SyntaxError),
    #[error("no HTTP method block (get, post, ...) found")]
    MissingHttpBlock,
}

const HTTP_BLOCKS: &[&str] = &[
    "get", "post", "put", "delete", "patch", "options", "head", "connect", "trace", "http",
];

/// A request `.bru` file: its document plus the typed request.
#[derive(Debug, Clone)]
pub struct RequestFile {
    pub doc: BruDocument,
    pub request: HttpRequest,
}

/// `collection.bru` or `folder.bru`: settings shared by the requests below it.
#[derive(Debug, Clone, PartialEq)]
pub struct ScopeFile {
    pub doc: BruDocument,
    pub name: Option<String>,
    pub seq: Option<f64>,
    pub headers: Vec<KeyValue>,
    pub auth: Auth,
    pub vars_pre: Vec<KeyValue>,
    pub vars_post: Vec<KeyValue>,
    pub script_pre: Option<String>,
    pub script_post: Option<String>,
    pub tests: Option<String>,
}

/// `environments/<name>.bru`.
#[derive(Debug, Clone, PartialEq)]
pub struct EnvironmentFile {
    pub doc: BruDocument,
    pub vars: Vec<KeyValue>,
    /// Names from `vars:secret`; values live outside the file.
    pub secret_names: Vec<String>,
}

impl RequestFile {
    pub fn parse(src: &str) -> Result<RequestFile, ModelError> {
        let doc = BruDocument::parse(src)?;
        let request = read_request(&doc)?;
        Ok(RequestFile { doc, request })
    }

    pub fn to_bru(&self) -> String {
        let mut doc = self.doc.clone();
        write_request(&mut doc, &self.request);
        doc.write()
    }
}

impl ScopeFile {
    pub fn parse(src: &str) -> Result<ScopeFile, ModelError> {
        let doc = BruDocument::parse(src)?;
        Ok(read_scope(doc))
    }

    pub fn to_bru(&self) -> String {
        let mut doc = self.doc.clone();
        let old = read_scope(self.doc.clone());
        if old.name != self.name {
            set_dict_key(&mut doc, "meta", "name", self.name.as_deref());
        }
        if old.seq != self.seq {
            set_dict_key(&mut doc, "meta", "seq", self.seq.map(format_seq).as_deref());
        }
        if old.headers != self.headers {
            write_kv_block(&mut doc, "headers", &self.headers);
        }
        if old.auth != self.auth {
            set_dict_key(&mut doc, "auth", "mode", Some(&auth_mode(&self.auth)));
            write_auth_blocks(&mut doc, &self.auth);
        }
        if old.vars_pre != self.vars_pre {
            write_kv_block(&mut doc, "vars:pre-request", &self.vars_pre);
        }
        if old.vars_post != self.vars_post {
            write_kv_block(&mut doc, "vars:post-response", &self.vars_post);
        }
        write_text_if_changed(
            &mut doc,
            "script:pre-request",
            &old.script_pre,
            &self.script_pre,
        );
        write_text_if_changed(
            &mut doc,
            "script:post-response",
            &old.script_post,
            &self.script_post,
        );
        write_text_if_changed(&mut doc, "tests", &old.tests, &self.tests);
        doc.write()
    }
}

impl EnvironmentFile {
    pub fn parse(src: &str) -> Result<EnvironmentFile, ModelError> {
        let doc = BruDocument::parse(src)?;
        Ok(read_environment(doc))
    }

    pub fn to_bru(&self) -> String {
        let mut doc = self.doc.clone();
        let old = read_environment(self.doc.clone());
        if old.vars != self.vars {
            write_kv_block(&mut doc, "vars", &self.vars);
        }
        if old.secret_names != self.secret_names {
            if self.secret_names.is_empty() {
                doc.remove_block("vars:secret");
            } else {
                doc.set_block("vars:secret", BlockContent::List(self.secret_names.clone()));
            }
        }
        doc.write()
    }
}

// ---- reading

fn read_request(doc: &BruDocument) -> Result<HttpRequest, ModelError> {
    let kind = dict_value(doc, "meta", "type").unwrap_or_else(|| "http".to_owned());
    let http = doc
        .blocks()
        .find(|b| HTTP_BLOCKS.contains(&b.name.as_str()));
    if http.is_none() && kind == "http" {
        return Err(ModelError::MissingHttpBlock);
    }
    let http_name = http.map(|b| b.name.as_str()).unwrap_or("get");
    let method = Method::from_block_name(http_name).unwrap_or_else(|| {
        Method::Custom(dict_value(doc, http_name, "method").unwrap_or_default())
    });
    let body_mode = dict_value(doc, http_name, "body").unwrap_or_else(|| "none".to_owned());
    let auth_mode = dict_value(doc, http_name, "auth").unwrap_or_else(|| "none".to_owned());
    Ok(HttpRequest {
        name: dict_value(doc, "meta", "name").unwrap_or_default(),
        seq: parse_seq(dict_value(doc, "meta", "seq").as_deref()).unwrap_or(1.0),
        kind,
        method,
        url: dict_value(doc, http_name, "url").unwrap_or_default(),
        query: kv_list(doc, "params:query"),
        path_params: kv_list(doc, "params:path"),
        headers: kv_list(doc, "headers"),
        body: read_body(doc, &body_mode),
        auth: read_auth(doc, &auth_mode),
        vars_pre: kv_list(doc, "vars:pre-request"),
        vars_post: kv_list(doc, "vars:post-response"),
        script_pre: text(doc, "script:pre-request"),
        script_post: text(doc, "script:post-response"),
        tests: text(doc, "tests"),
        settings: RequestSettings {
            follow_redirects: dict_value(doc, "settings", "followRedirects")
                .and_then(|v| v.parse().ok()),
            max_redirects: dict_value(doc, "settings", "maxRedirects").and_then(|v| v.parse().ok()),
            timeout_ms: dict_value(doc, "settings", "timeout").and_then(|v| v.parse().ok()),
        },
    })
}

fn read_scope(doc: BruDocument) -> ScopeFile {
    let auth_mode = dict_value(&doc, "auth", "mode").unwrap_or_else(|| "none".to_owned());
    ScopeFile {
        name: dict_value(&doc, "meta", "name"),
        seq: parse_seq(dict_value(&doc, "meta", "seq").as_deref()),
        headers: kv_list(&doc, "headers"),
        auth: read_auth(&doc, &auth_mode),
        vars_pre: kv_list(&doc, "vars:pre-request"),
        vars_post: kv_list(&doc, "vars:post-response"),
        script_pre: text(&doc, "script:pre-request"),
        script_post: text(&doc, "script:post-response"),
        tests: text(&doc, "tests"),
        doc,
    }
}

fn read_environment(doc: BruDocument) -> EnvironmentFile {
    let secret_names = match doc.block("vars:secret").map(|b| b.content()) {
        Some(BlockContent::List(items)) => items.clone(),
        _ => Vec::new(),
    };
    EnvironmentFile {
        vars: kv_list(&doc, "vars"),
        secret_names,
        doc,
    }
}

fn read_body(doc: &BruDocument, mode: &str) -> Body {
    match mode {
        "none" => Body::None,
        "json" => Body::Json(text(doc, "body:json").unwrap_or_default()),
        "text" => Body::Text(text(doc, "body:text").unwrap_or_default()),
        "xml" => Body::Xml(text(doc, "body:xml").unwrap_or_default()),
        "formUrlEncoded" => Body::FormUrlEncoded(kv_list(doc, "body:form-urlencoded")),
        "multipartForm" => Body::Multipart(
            pairs(doc, "body:multipart-form")
                .iter()
                .map(multipart_field)
                .collect(),
        ),
        other => Body::Unsupported(other.to_owned()),
    }
}

fn read_auth(doc: &BruDocument, mode: &str) -> Auth {
    match mode {
        "none" => Auth::None,
        "inherit" => Auth::Inherit,
        "basic" => Auth::Basic {
            username: dict_value(doc, "auth:basic", "username").unwrap_or_default(),
            password: dict_value(doc, "auth:basic", "password").unwrap_or_default(),
        },
        "bearer" => Auth::Bearer {
            token: dict_value(doc, "auth:bearer", "token").unwrap_or_default(),
        },
        other => Auth::Unsupported(other.to_owned()),
    }
}

/// Bruno `mapPairListToKeyValPairsMultipart`: trailing `@contentType(x)` and `@file(a|b)` values.
fn multipart_field(pair: &Pair) -> MultipartField {
    let (raw, content_type) = match &pair.value {
        PairValue::Multiline { text, content_type } => (text.clone(), content_type.clone()),
        other => split_content_type(&other.as_text()),
    };
    let value = match raw.strip_prefix("@file(").and_then(|r| r.strip_suffix(')')) {
        Some(files) => MultipartValue::Files(
            files
                .split('|')
                .filter(|f| !f.is_empty())
                .map(str::to_owned)
                .collect(),
        ),
        None => MultipartValue::Text(raw),
    };
    MultipartField {
        name: pair.key.clone(),
        value,
        enabled: pair.enabled,
        content_type,
    }
}

fn split_content_type(value: &str) -> (String, Option<String>) {
    if let Some(start) = value.rfind("@contentType(")
        && let Some(ct) = value[start..]
            .strip_prefix("@contentType(")
            .and_then(|r| r.strip_suffix(')'))
    {
        return (value[..start].trim_end().to_owned(), Some(ct.to_owned()));
    }
    (value.to_owned(), None)
}

fn pairs<'a>(doc: &'a BruDocument, block: &str) -> &'a [Pair] {
    doc.block(block).map(|b| b.content().pairs()).unwrap_or(&[])
}

fn dict_value(doc: &BruDocument, block: &str, key: &str) -> Option<String> {
    pairs(doc, block)
        .iter()
        .find(|p| p.key == key)
        .map(|p| p.value.as_text())
}

fn kv_list(doc: &BruDocument, block: &str) -> Vec<KeyValue> {
    pairs(doc, block)
        .iter()
        .map(|p| KeyValue {
            name: p.key.clone(),
            value: p.value.as_text(),
            enabled: p.enabled,
        })
        .collect()
}

fn text(doc: &BruDocument, block: &str) -> Option<String> {
    doc.block(block)
        .and_then(|b| b.content().text())
        .map(str::to_owned)
}

fn parse_seq(value: Option<&str>) -> Option<f64> {
    value
        .and_then(|v| v.trim().parse::<f64>().ok())
        .filter(|v| v.is_finite())
}

fn format_seq(seq: f64) -> String {
    if seq.fract() == 0.0 {
        format!("{}", seq as i64)
    } else {
        seq.to_string()
    }
}

// ---- writing

fn write_request(doc: &mut BruDocument, req: &HttpRequest) {
    let old = read_request(doc).ok();
    let changed = |f: &dyn Fn(&HttpRequest) -> bool| old.as_ref().is_none_or(|o| !f(o));

    if changed(&|o| o.name == req.name) {
        set_dict_key(doc, "meta", "name", Some(&req.name));
    }
    if changed(&|o| o.seq == req.seq) {
        set_dict_key(doc, "meta", "seq", Some(&format_seq(req.seq)));
    }
    if changed(&|o| o.kind == req.kind) {
        set_dict_key(doc, "meta", "type", Some(&req.kind));
    }

    let current = doc
        .blocks()
        .find(|b| HTTP_BLOCKS.contains(&b.name.as_str()))
        .map(|b| b.name.clone());
    let target = req.method.block_name().unwrap_or("http").to_owned();
    match &current {
        Some(name) if *name != target => doc.rename_block(name, &target),
        Some(_) => {}
        None => doc.set_block(&target, BlockContent::Dict(Vec::new())),
    }
    if changed(&|o| o.method == req.method) {
        let custom = match &req.method {
            Method::Custom(m) => Some(m.as_str()),
            _ => None,
        };
        set_dict_key(doc, &target, "method", custom);
    }
    if changed(&|o| o.url == req.url) {
        set_dict_key(doc, &target, "url", Some(&req.url));
    }
    if changed(&|o| o.body == req.body) {
        set_dict_key(doc, &target, "body", Some(&body_mode(&req.body)));
        write_body_block(doc, &req.body);
    }
    if changed(&|o| o.auth == req.auth) {
        set_dict_key(doc, &target, "auth", Some(&auth_mode(&req.auth)));
        write_auth_blocks(doc, &req.auth);
    }
    for (block, new, get) in [
        (
            "params:query",
            &req.query,
            (|o: &HttpRequest| o.query.clone()) as fn(&HttpRequest) -> Vec<KeyValue>,
        ),
        ("params:path", &req.path_params, |o| o.path_params.clone()),
        ("headers", &req.headers, |o| o.headers.clone()),
        ("vars:pre-request", &req.vars_pre, |o| o.vars_pre.clone()),
        ("vars:post-response", &req.vars_post, |o| {
            o.vars_post.clone()
        }),
    ] {
        if changed(&|o| get(o) == *new) {
            write_kv_block(doc, block, new);
        }
    }
    let none = None;
    let old_text =
        |f: fn(&HttpRequest) -> &Option<String>| old.as_ref().map(f).unwrap_or(&none).clone();
    write_text_if_changed(
        doc,
        "script:pre-request",
        &old_text(|o| &o.script_pre),
        &req.script_pre,
    );
    write_text_if_changed(
        doc,
        "script:post-response",
        &old_text(|o| &o.script_post),
        &req.script_post,
    );
    write_text_if_changed(doc, "tests", &old_text(|o| &o.tests), &req.tests);
    if changed(&|o| o.settings == req.settings) {
        let s = &req.settings;
        set_dict_key_if_some(
            doc,
            "settings",
            "followRedirects",
            s.follow_redirects.map(|v| v.to_string()),
        );
        set_dict_key_if_some(
            doc,
            "settings",
            "maxRedirects",
            s.max_redirects.map(|v| v.to_string()),
        );
        set_dict_key_if_some(
            doc,
            "settings",
            "timeout",
            s.timeout_ms.map(|v| v.to_string()),
        );
    }
}

fn body_mode(body: &Body) -> String {
    match body {
        Body::None => "none".into(),
        Body::Json(_) => "json".into(),
        Body::Text(_) => "text".into(),
        Body::Xml(_) => "xml".into(),
        Body::FormUrlEncoded(_) => "formUrlEncoded".into(),
        Body::Multipart(_) => "multipartForm".into(),
        Body::Unsupported(mode) => mode.clone(),
    }
}

fn auth_mode(auth: &Auth) -> String {
    match auth {
        Auth::None => "none".into(),
        Auth::Inherit => "inherit".into(),
        Auth::Basic { .. } => "basic".into(),
        Auth::Bearer { .. } => "bearer".into(),
        Auth::Unsupported(mode) => mode.clone(),
    }
}

/// Writes the active body's block. Inactive body blocks are kept, as Bruno does.
fn write_body_block(doc: &mut BruDocument, body: &Body) {
    match body {
        Body::Json(t) => doc.set_block("body:json", BlockContent::Text(t.clone())),
        Body::Text(t) => doc.set_block("body:text", BlockContent::Text(t.clone())),
        Body::Xml(t) => doc.set_block("body:xml", BlockContent::Text(t.clone())),
        Body::FormUrlEncoded(items) => write_kv_block(doc, "body:form-urlencoded", items),
        Body::Multipart(fields) => {
            let existing = pairs(doc, "body:multipart-form").to_vec();
            let merged = merge_pairs(
                &existing,
                fields,
                |f| &f.name,
                multipart_pair_value,
                |p, f| multipart_field(p) == *f,
            );
            doc.set_block("body:multipart-form", BlockContent::Dict(merged));
        }
        Body::None | Body::Unsupported(_) => {}
    }
}

fn multipart_pair_value(field: &MultipartField) -> (PairValue, bool) {
    let raw = match &field.value {
        MultipartValue::Text(t) => t.clone(),
        MultipartValue::Files(files) => format!("@file({})", files.join("|")),
    };
    let value = match (&field.content_type, raw.contains('\n')) {
        (ct, true) => PairValue::Multiline {
            text: raw,
            content_type: ct.clone(),
        },
        (Some(ct), false) => PairValue::Single(format!("{raw} @contentType({ct})")),
        (None, false) => PairValue::Single(raw),
    };
    (value, field.enabled)
}

fn write_auth_blocks(doc: &mut BruDocument, auth: &Auth) {
    match auth {
        Auth::Basic { username, password } => {
            set_dict_key(doc, "auth:basic", "username", Some(username));
            set_dict_key(doc, "auth:basic", "password", Some(password));
        }
        Auth::Bearer { token } => set_dict_key(doc, "auth:bearer", "token", Some(token)),
        Auth::None | Auth::Inherit | Auth::Unsupported(_) => {}
    }
}

fn write_text_if_changed(
    doc: &mut BruDocument,
    block: &str,
    old: &Option<String>,
    new: &Option<String>,
) {
    if old == new {
        return;
    }
    match new {
        Some(t) => doc.set_block(block, BlockContent::Text(t.clone())),
        None => doc.remove_block(block),
    }
}

fn value_for(text: &str) -> PairValue {
    if text.contains('\n') {
        PairValue::Multiline {
            text: text.to_owned(),
            content_type: None,
        }
    } else {
        PairValue::Single(text.to_owned())
    }
}

/// Sets (or with `None` removes) one key of a dictionary block, creating the block if needed.
fn set_dict_key(doc: &mut BruDocument, block: &str, key: &str, value: Option<&str>) {
    let mut entries = pairs(doc, block).to_vec();
    match (entries.iter().position(|p| p.key == key), value) {
        (Some(i), Some(v)) if entries[i].value.as_text() != v => entries[i].value = value_for(v),
        (Some(_), Some(_)) => {}
        (Some(i), None) => {
            entries.remove(i);
        }
        (None, Some(v)) => entries.push(Pair {
            key: key.to_owned(),
            value: value_for(v),
            enabled: true,
            annotations: Vec::new(),
        }),
        (None, None) => return,
    }
    doc.set_block(block, BlockContent::Dict(entries));
}

fn set_dict_key_if_some(doc: &mut BruDocument, block: &str, key: &str, value: Option<String>) {
    if let Some(v) = value {
        set_dict_key(doc, block, key, Some(&v));
    }
}

/// Replaces a key/value block. Existing pairs with the same key keep their
/// annotations and value spelling; an empty list removes the block.
fn write_kv_block(doc: &mut BruDocument, block: &str, items: &[KeyValue]) {
    if items.is_empty() {
        doc.remove_block(block);
        return;
    }
    let existing = pairs(doc, block).to_vec();
    let merged = merge_pairs(
        &existing,
        items,
        |kv| &kv.name,
        |kv| (value_for(&kv.value), kv.enabled),
        |p, kv| p.value.as_text() == kv.value && p.enabled == kv.enabled,
    );
    doc.set_block(block, BlockContent::Dict(merged));
}

fn merge_pairs<T>(
    existing: &[Pair],
    items: &[T],
    key: impl Fn(&T) -> &String,
    value: impl Fn(&T) -> (PairValue, bool),
    same: impl Fn(&Pair, &T) -> bool,
) -> Vec<Pair> {
    let mut used = vec![false; existing.len()];
    items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let name = key(item);
            let matched = if existing.get(i).is_some_and(|p| &p.key == name) && !used[i] {
                Some(i)
            } else {
                (0..existing.len()).find(|&j| !used[j] && &existing[j].key == name)
            };
            match matched {
                Some(j) => {
                    used[j] = true;
                    let mut pair = existing[j].clone();
                    if !same(&pair, item) {
                        let (v, enabled) = value(item);
                        pair.value = v;
                        pair.enabled = enabled;
                    }
                    pair
                }
                None => {
                    let (v, enabled) = value(item);
                    Pair {
                        key: name.clone(),
                        value: v,
                        enabled,
                        annotations: Vec::new(),
                    }
                }
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const REQUEST: &str = include_str!("../../../../fixtures/bruno-lang/request.bru");
    const FOLDER: &str =
        include_str!("../../../../fixtures/bruno-tests/collection/url-serialization/folder.bru");

    fn line_changes(a: &str, b: &str) -> (usize, usize) {
        let diff = similar::TextDiff::from_lines(a, b);
        let removed = diff
            .iter_all_changes()
            .filter(|c| c.tag() == similar::ChangeTag::Delete)
            .count();
        let added = diff
            .iter_all_changes()
            .filter(|c| c.tag() == similar::ChangeTag::Insert)
            .count();
        (removed, added)
    }

    #[test]
    fn parses_request_fixture() {
        let file = RequestFile::parse(REQUEST).unwrap();
        let r = &file.request;
        assert_eq!(r.method, Method::Get);
        assert_eq!(r.url, "https://api.textlocal.in/send/:id");
        assert_eq!(
            r.auth,
            Auth::Bearer {
                token: "123".into()
            }
        );
        assert_eq!(r.path_params, [KeyValue::new("id", "123")]);
        assert_eq!(r.name, "Send Bulk SMS");
        assert_eq!(r.seq, 1.0);
    }

    #[test]
    fn disabled_entries_kept_not_enabled() {
        let file = RequestFile::parse(REQUEST).unwrap();
        let h = file
            .request
            .headers
            .iter()
            .find(|h| h.name == "transaction-id")
            .unwrap();
        assert!(!h.enabled);
        assert_eq!(h.value, "{{transactionId}}");
        assert_eq!(file.to_bru(), REQUEST);
    }

    #[test]
    fn edit_url_changes_one_line() {
        let mut file = RequestFile::parse(REQUEST).unwrap();
        file.request.url = "https://x/y".into();
        assert_eq!(line_changes(REQUEST, &file.to_bru()), (1, 1));
    }

    #[test]
    fn edit_annotated_header_keeps_annotation() {
        let src =
            "get {\n  url: http://h\n}\n\nheaders {\n  @description('why')\n  a: 1\n  b: 2\n}\n";
        let mut file = RequestFile::parse(src).unwrap();
        file.request.headers[0].value = "9".into();
        assert_eq!(file.to_bru(), src.replace("a: 1", "a: 9"));
    }

    #[test]
    fn switch_body_mode() {
        let src =
            "post {\n  url: http://h\n  body: json\n  auth: none\n}\n\nbody:json {\n  {}\n}\n";
        let mut file = RequestFile::parse(src).unwrap();
        file.request.body = Body::FormUrlEncoded(vec![KeyValue::new("a", "1")]);
        let out = file.to_bru();
        assert!(out.contains("  body: formUrlEncoded\n"), "{out}");
        assert!(out.contains("body:form-urlencoded {\n  a: 1\n}"), "{out}");
        assert!(out.contains("body:json {\n  {}\n}"), "{out}");
        let reparsed = RequestFile::parse(&out).unwrap();
        assert_eq!(reparsed.request.body, file.request.body);
    }

    #[test]
    fn change_method_renames_block_only() {
        let mut file = RequestFile::parse(REQUEST).unwrap();
        file.request.method = Method::Post;
        let out = file.to_bru();
        assert_eq!(line_changes(REQUEST, &out), (1, 1));
        assert!(out.contains("post {\n  url: https://api.textlocal.in/send/:id"));
    }

    #[test]
    fn multipart_fields() {
        let src = "post {\n  url: http://h\n  body: multipartForm\n}\n\nbody:multipart-form {\n  a: x @contentType(text/plain)\n  f: @file(a.txt|b.txt)\n}\n";
        let file = RequestFile::parse(src).unwrap();
        let Body::Multipart(fields) = &file.request.body else {
            panic!("not multipart")
        };
        assert_eq!(fields[0].value, MultipartValue::Text("x".into()));
        assert_eq!(fields[0].content_type.as_deref(), Some("text/plain"));
        assert_eq!(
            fields[1].value,
            MultipartValue::Files(vec!["a.txt".into(), "b.txt".into()])
        );
        assert_eq!(file.to_bru(), src);
    }

    #[test]
    fn missing_http_block_is_error() {
        assert!(matches!(
            RequestFile::parse("meta {\n  name: a\n}\n"),
            Err(ModelError::MissingHttpBlock)
        ));
        assert!(
            RequestFile::parse("meta {\n  name: a\n  type: grpc\n}\n\ngrpc {\n  url: x\n}\n")
                .is_ok()
        );
    }

    #[test]
    fn scope_file_auth_inherit() {
        let scope = ScopeFile::parse(FOLDER).unwrap();
        assert_eq!(scope.auth, Auth::Inherit);
        assert_eq!(scope.seq, Some(13.0));
        assert_eq!(scope.to_bru(), FOLDER);
    }

    #[test]
    fn environment_secrets() {
        let src = "vars {\n  host: http://localhost\n}\nvars:secret [\n  client_secret\n]\n";
        let env = EnvironmentFile::parse(src).unwrap();
        assert_eq!(env.secret_names, ["client_secret"]);
        assert_eq!(env.vars, [KeyValue::new("host", "http://localhost")]);
        assert_eq!(env.to_bru(), src);
    }
}
