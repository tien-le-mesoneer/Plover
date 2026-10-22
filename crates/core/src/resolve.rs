//! Turns a request plus its folder/collection/environment scopes into a
//! concrete `ResolvedRequest`. Pure: no file or network I/O.

use std::path::PathBuf;

use base64::Engine;

use crate::collection::{Collection, Environment, Item, RequestRef};
use crate::request::{
    Auth, Body, KeyValue, Method, MultipartField, MultipartValue, RequestSettings,
};
use crate::script::ScriptVars;
use crate::vars::{ValueMap, VarStack, enabled_map, string_values};

#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedRequest {
    pub method: Method,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: ResolvedBody,
    pub settings: RequestSettings,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ResolvedBody {
    None,
    /// Raw body; its default `content-type` is already in `ResolvedRequest::headers`.
    Bytes(Vec<u8>),
    Form(Vec<(String, String)>),
    Multipart(Vec<ResolvedPart>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum ResolvedPart {
    Text {
        name: String,
        value: String,
        content_type: Option<String>,
    },
    File {
        name: String,
        path: PathBuf,
        content_type: Option<String>,
    },
}

pub struct ResolveContext<'a> {
    pub collection: &'a Collection,
    pub environment: Option<&'a Environment>,
    /// `--env-var` values, in-memory secrets and script changes; they win over the
    /// environment file (and supply `vars:secret`).
    pub env_overrides: &'a ValueMap,
    pub runtime: &'a ValueMap,
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ResolveError {
    #[error("{0} is not a runnable request")]
    NotARequest(PathBuf),
    #[error("unsupported {0}")]
    Unsupported(String),
    #[error("invalid URL: {0}")]
    InvalidUrl(String),
}

/// A request after Bruno's `prepareRequest`: scopes merged, auth and the default content
/// type applied, but nothing interpolated yet. This is what pre-request scripts see as `req`.
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedRequest {
    pub method: Method,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: PreparedBody,
    /// Basic auth is applied after interpolation (interpolate-vars.js:216-222).
    pub basic_auth: Option<(String, String)>,
    pub path_params: Vec<KeyValue>,
    pub settings: RequestSettings,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PreparedBody {
    None,
    /// JSON (comments already stripped), text and XML bodies.
    Text(String),
    Form(Vec<(String, String)>),
    Multipart(Vec<MultipartField>),
}

/// The variable scopes of one request, from its collection, folders, environment and
/// the current runtime variables.
pub fn initial_vars(
    ctx: &ResolveContext<'_>,
    req: &RequestRef<'_>,
) -> Result<ScriptVars, ResolveError> {
    let Item::Request { file, .. } = req.item else {
        return Err(ResolveError::NotARequest(req.path.to_path_buf()));
    };
    let mut folder = ValueMap::new();
    for scope in req.folders.iter().filter_map(|f| f.scope.as_ref()) {
        folder.extend(string_values(&enabled_map(&scope.vars_pre)));
    }
    Ok(ScriptVars {
        runtime: ctx.runtime.clone(),
        environment: environment_values(ctx),
        collection: ctx
            .collection
            .scope
            .as_ref()
            .map(|s| string_values(&enabled_map(&s.vars_pre)))
            .unwrap_or_default(),
        folder,
        request: string_values(&enabled_map(&file.request.vars_pre)),
    })
}

/// Environment file variables, `vars:secret` names (as `''`), then overrides.
pub fn environment_values(ctx: &ResolveContext<'_>) -> ValueMap {
    let mut environment = ValueMap::new();
    if let Some(env) = ctx.environment {
        environment = string_values(&enabled_map(&env.file.vars));
        // Secret names get '' (envToJson.js:168, :325) unless `--env-var` supplies a value.
        for secret in env.file.secrets.iter().filter(|s| s.enabled) {
            environment.insert(
                secret.name.clone(),
                serde_json::Value::String(String::new()),
            );
        }
    }
    environment.extend(
        ctx.env_overrides
            .iter()
            .map(|(k, v)| (k.clone(), v.clone())),
    );
    environment
}

pub fn resolve(
    ctx: &ResolveContext<'_>,
    req: &RequestRef<'_>,
) -> Result<ResolvedRequest, ResolveError> {
    let prepared = prepare(ctx, req)?;
    let vars = initial_vars(ctx, req)?;
    finalize(&prepared, &vars.var_stack(), &ctx.collection.root)
}

/// Bruno's `prepareRequest` (bruno-cli/src/runner/prepare-request.js): merges headers and
/// auth from the scopes and adds the default content type. Nothing is interpolated.
pub fn prepare(
    ctx: &ResolveContext<'_>,
    req: &RequestRef<'_>,
) -> Result<PreparedRequest, ResolveError> {
    let Item::Request { file, .. } = req.item else {
        return Err(ResolveError::NotARequest(req.path.to_path_buf()));
    };
    let request = &file.request;
    let collection_scope = ctx.collection.scope.as_ref();
    let folder_scopes: Vec<_> = req
        .folders
        .iter()
        .filter_map(|f| f.scope.as_ref())
        .collect();

    // Header assembly follows Bruno CLI step by step; see `fold_case_variants`.
    // 1. mergeHeaders (utils/collection.js:99-140): exact-name map, later scope wins.
    let mut headers: Vec<(String, String)> = Vec::new();
    let header_lists = collection_scope
        .map(|s| &s.headers)
        .into_iter()
        .chain(folder_scopes.iter().map(|s| &s.headers))
        .chain(std::iter::once(&request.headers));
    for list in header_lists {
        // prepare-request.js:34: enabled headers with a non-empty name only.
        for h in list.iter().filter(|h| h.enabled && !h.name.is_empty()) {
            set_exact(&mut headers, &h.name, h.value.clone());
        }
    }
    // prepare-request.js:36: an explicit content type is detected case-insensitively.
    let content_type_defined = headers
        .iter()
        .any(|(k, _)| k.eq_ignore_ascii_case("content-type"));

    // 2. Auth (prepare-request.js:60-70, :210-259). Bearer is set before interpolation.
    let auth = effective_auth(
        &request.auth,
        collection_scope.map(|s| &s.auth),
        folder_scopes.iter().map(|s| &s.auth),
    );
    let mut basic_auth = None;
    match auth {
        Auth::Bearer { token } => {
            set_exact(&mut headers, "Authorization", format!("Bearer {token}"))
        }
        Auth::Basic { username, password } => {
            basic_auth = Some((username.clone(), password.clone()))
        }
        Auth::Unsupported(mode) => {
            return Err(ResolveError::Unsupported(format!("auth mode `{mode}`")));
        }
        Auth::None | Auth::Inherit => {}
    }

    // 3. Default content type (lowercase key) unless one was given.
    // prepare-request.js:363-441; an empty JSON body sends nothing (:365).
    let default_content_type = match &request.body {
        Body::Json(t) if !t.is_empty() => Some("application/json"),
        Body::Text(_) => Some("text/plain"),
        Body::Xml(_) => Some("application/xml"),
        Body::FormUrlEncoded(_) => Some("application/x-www-form-urlencoded"),
        Body::Multipart(_) => Some("multipart/form-data"),
        _ => None,
    };
    if let Some(ct) = default_content_type.filter(|_| !content_type_defined) {
        set_exact(&mut headers, "content-type", ct.to_owned());
    }

    let body = match &request.body {
        Body::None => PreparedBody::None,
        Body::Json(t) if t.is_empty() => PreparedBody::None,
        // prepare-request.js:369-373: `decomment`, falling back to the raw text on error.
        Body::Json(t) => PreparedBody::Text(strip_json_comments(t).unwrap_or_else(|| t.clone())),
        Body::Text(t) | Body::Xml(t) => PreparedBody::Text(t.clone()),
        Body::FormUrlEncoded(items) => PreparedBody::Form(
            items
                .iter()
                .filter(|kv| kv.enabled)
                .map(|kv| (kv.name.clone(), kv.value.clone()))
                .collect(),
        ),
        Body::Multipart(fields) => {
            PreparedBody::Multipart(fields.iter().filter(|f| f.enabled).cloned().collect())
        }
        Body::Unsupported(mode) => {
            return Err(ResolveError::Unsupported(format!("body mode `{mode}`")));
        }
    };

    Ok(PreparedRequest {
        method: request.method.clone(),
        url: request.url.clone(),
        headers,
        body,
        basic_auth,
        path_params: request.path_params.clone(),
        settings: request.settings.clone(),
    })
}

/// Bruno's `interpolateVars` (bruno-cli/src/runner/interpolate-vars.js) and the URL
/// fix-ups before sending. `root` resolves multipart file paths.
pub fn finalize(
    prepared: &PreparedRequest,
    vars: &VarStack,
    root: &std::path::Path,
) -> Result<ResolvedRequest, ResolveError> {
    // 4. interpolate-vars.js:83-86: each key is deleted and re-set interpolated, in order.
    let mut headers = prepared.headers.clone();
    for (name, _) in headers.clone() {
        if let Some(pos) = headers.iter().position(|(k, _)| *k == name) {
            let (_, value) = headers.remove(pos);
            set_exact(
                &mut headers,
                &vars.interpolate(&name),
                vars.interpolate(&value),
            );
        }
    }

    // 5. Basic auth is added after interpolation (interpolate-vars.js:216-222).
    if let Some((username, password)) = &prepared.basic_auth {
        let raw = format!(
            "{}:{}",
            vars.interpolate(username),
            vars.interpolate(password)
        );
        let encoded = base64::engine::general_purpose::STANDARD.encode(raw);
        set_exact(&mut headers, "Authorization", format!("Basic {encoded}"));
    }
    let headers = fold_case_variants(headers);

    let mut url = substitute_path_params(
        &vars.interpolate(&prepared.url),
        &prepared.path_params,
        vars,
    )?;
    // run-single-request.js:391-393.
    if !has_explicit_scheme(&url) {
        url = format!("http://{url}");
    }

    let body = match &prepared.body {
        PreparedBody::None => ResolvedBody::None,
        PreparedBody::Text(t) => ResolvedBody::Bytes(vars.interpolate(t).into_bytes()),
        PreparedBody::Form(items) => ResolvedBody::Form(
            items
                .iter()
                .map(|(k, v)| (vars.interpolate(k), vars.interpolate(v)))
                .collect(),
        ),
        PreparedBody::Multipart(fields) => {
            let mut parts = Vec::new();
            for field in fields {
                let name = vars.interpolate(&field.name);
                let content_type = field.content_type.clone();
                match &field.value {
                    MultipartValue::Text(v) => parts.push(ResolvedPart::Text {
                        name,
                        value: vars.interpolate(v),
                        content_type,
                    }),
                    MultipartValue::Files(files) => {
                        parts.extend(files.iter().map(|f| ResolvedPart::File {
                            name: name.clone(),
                            path: root.join(vars.interpolate(f)),
                            content_type: content_type.clone(),
                        }))
                    }
                }
            }
            ResolvedBody::Multipart(parts)
        }
    };

    Ok(ResolvedRequest {
        method: prepared.method.clone(),
        url,
        headers,
        body,
        settings: prepared.settings.clone(),
    })
}

/// JS object / `Map` assignment: replaces the value of an identically named
/// key in place, otherwise appends.
fn set_exact(headers: &mut Vec<(String, String)>, name: &str, value: String) {
    match headers.iter_mut().find(|(k, _)| k == name) {
        Some(entry) => entry.1 = value,
        None => headers.push((name.to_owned(), value)),
    }
}

/// axios 1.18 `AxiosHeaders` construction (`setHeader` with `findKey`): names
/// differing only by case fold into the first key's spelling and position, and
/// take the value of the last such key.
pub(crate) fn fold_case_variants(headers: Vec<(String, String)>) -> Vec<(String, String)> {
    let mut folded: Vec<(String, String)> = Vec::with_capacity(headers.len());
    for (name, value) in headers {
        match folded
            .iter_mut()
            .find(|(k, _)| k.eq_ignore_ascii_case(&name))
        {
            Some(entry) => entry.1 = value,
            None => folded.push((name, value)),
        }
    }
    folded
}

/// Bruno `mergeAuth`: the innermost folder with a concrete auth mode wins over
/// the collection; `inherit` requests take that result.
fn effective_auth<'a>(
    request: &'a Auth,
    collection: Option<&'a Auth>,
    folders_outer_to_inner: impl Iterator<Item = &'a Auth>,
) -> &'a Auth {
    if *request != Auth::Inherit {
        return request;
    }
    let mut effective = collection.unwrap_or(&Auth::None);
    for auth in folders_outer_to_inner {
        if !matches!(auth, Auth::None | Auth::Inherit) {
            effective = auth;
        }
    }
    effective
}

/// bruno-common/src/utils/url/index.ts `getExplicitScheme`: `scheme://` with an ASCII
/// letter followed by letters, digits, `+`, `-` or `.`.
fn has_explicit_scheme(url: &str) -> bool {
    match url.find("://") {
        Some(end) if end >= 1 => {
            let scheme = &url[..end];
            scheme.starts_with(|c: char| c.is_ascii_alphabetic())
                && scheme
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
        }
        _ => false,
    }
}

/// Approximates the `decomment` package as Bruno calls it: removes `//` and
/// `/* */` comments outside string literals and drops lines left blank by a
/// removed comment. `None` (unterminated string or comment) means "send as is".
fn strip_json_comments(text: &str) -> Option<String> {
    let mut out = String::with_capacity(text.len());
    let mut line = String::new();
    let mut line_had_comment = false;
    let mut chars = text.chars().peekable();
    let end_line = |out: &mut String, line: &mut String, had_comment: &mut bool, newline: bool| {
        if !(*had_comment && line.trim().is_empty()) {
            out.push_str(line);
            if newline {
                out.push('\n');
            }
        }
        line.clear();
        *had_comment = false;
    };
    while let Some(c) = chars.next() {
        match c {
            '"' | '\'' | '`' => {
                line.push(c);
                loop {
                    let next = chars.next()?;
                    if next == '\n' && c != '`' {
                        return None;
                    }
                    line.push(next);
                    if next == '\\' {
                        line.push(chars.next()?);
                    } else if next == c {
                        break;
                    }
                }
            }
            '/' if chars.peek() == Some(&'/') => {
                line_had_comment = true;
                while chars.peek().is_some_and(|n| *n != '\n') {
                    chars.next();
                }
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                line_had_comment = true;
                loop {
                    match chars.next()? {
                        '*' if chars.peek() == Some(&'/') => {
                            chars.next();
                            break;
                        }
                        '\n' => end_line(&mut out, &mut line, &mut true, true),
                        _ => {}
                    }
                }
            }
            '\n' => end_line(&mut out, &mut line, &mut line_had_comment, true),
            _ => line.push(c),
        }
    }
    end_line(&mut out, &mut line, &mut line_had_comment, false);
    Some(out)
}

/// Port of bruno-cli/src/runner/interpolate-vars.js:139-198. With any path params
/// declared, the URL is parsed (WHATWG) and rebuilt as origin + path + trailing
/// slash + search; `:name` segments and OData `Name(:key)` segments are filled in.
fn substitute_path_params(
    url: &str,
    params: &[KeyValue],
    vars: &VarStack,
) -> Result<String, ResolveError> {
    if params.is_empty() {
        return Ok(url.to_owned());
    }
    let values: Vec<(&str, Option<String>)> = params
        .iter()
        .map(|p| {
            let value = vars.interpolate(&p.value);
            // interpolate-vars.js:6-22 `hasResolvablePathParamValue`.
            let resolvable = p.enabled && !value.trim().is_empty();
            (p.name.as_str(), resolvable.then_some(value))
        })
        .collect();
    // `find` returns the first param with that name, resolvable or not.
    let lookup = |name: &str| {
        values
            .iter()
            .find(|(n, _)| *n == name)
            .and_then(|(_, v)| v.as_deref())
    };

    let with_scheme = if url.starts_with("http://") || url.starts_with("https://") {
        url.to_owned()
    } else {
        format!("http://{url}")
    };
    let parsed =
        url::Url::parse(&with_scheme).map_err(|e| ResolveError::InvalidUrl(e.to_string()))?;
    let mut path = String::new();
    for segment in parsed.path().split('/').filter(|s| !s.is_empty()) {
        path.push('/');
        match segment.strip_prefix(':') {
            Some(name) => path.push_str(lookup(name).unwrap_or(segment)),
            None if is_odata_segment(segment) => path.push_str(&fill_odata(segment, &lookup)),
            None => path.push_str(segment),
        }
    }
    let trailing = if parsed.path().ends_with('/') {
        "/"
    } else {
        ""
    };
    let search = parsed
        .query()
        .filter(|q| !q.is_empty())
        .map(|q| format!("?{q}"))
        .unwrap_or_default();
    Ok(format!(
        "{}{path}{trailing}{search}",
        parsed.origin().ascii_serialization()
    ))
}

/// `/^[A-Za-z0-9_.-]+\([^)]*\)$/`
fn is_odata_segment(segment: &str) -> bool {
    let Some(open) = segment.find('(') else {
        return false;
    };
    let (head, rest) = segment.split_at(open);
    !head.is_empty()
        && head
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
        && rest.ends_with(')')
        && !rest[1..rest.len() - 1].contains(')')
}

/// Replaces the first occurrence of each `:name` (`/[:]([a-zA-Z_]\w*)/g`) with its value.
fn fill_odata<'a>(segment: &str, lookup: &impl Fn(&str) -> Option<&'a str>) -> String {
    let mut result = segment.to_owned();
    let bytes = segment.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let starts_name = bytes[i] == b':'
            && bytes
                .get(i + 1)
                .is_some_and(|c| c.is_ascii_alphabetic() || *c == b'_');
        if !starts_name {
            i += 1;
            continue;
        }
        let end = (i + 1..bytes.len())
            .find(|&j| !(bytes[j].is_ascii_alphanumeric() || bytes[j] == b'_'))
            .unwrap_or(bytes.len());
        let name = &segment[i + 1..end];
        if let Some(value) = lookup(name) {
            result = result.replacen(&format!(":{name}"), value, 1);
        }
        i = end;
    }
    result
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use super::*;

    struct Fixture {
        _dir: tempfile::TempDir,
        collection: Collection,
    }

    fn fixture(files: &[(&str, &str)]) -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let mut all = vec![(
            "bruno.json",
            r#"{"version":"1","name":"t","type":"collection"}"#,
        )];
        all.extend_from_slice(files);
        for (rel, text) in all {
            let path = dir.path().join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
        let collection = Collection::load(dir.path()).unwrap();
        Fixture {
            _dir: dir,
            collection,
        }
    }

    fn run(
        f: &Fixture,
        rel: &str,
        env: Option<&str>,
        overrides: &[(&str, &str)],
    ) -> Result<ResolvedRequest, ResolveError> {
        let overrides: ValueMap = overrides
            .iter()
            .map(|(k, v)| (k.to_string(), serde_json::Value::String(v.to_string())))
            .collect();
        let runtime = ValueMap::new();
        let ctx = ResolveContext {
            collection: &f.collection,
            environment: env.and_then(|e| f.collection.environment(e)),
            env_overrides: &overrides,
            runtime: &runtime,
        };
        let target = f.collection.root.join(rel);
        let refs = f.collection.requests_under(Path::new(&target));
        resolve(&ctx, &refs[0])
    }

    fn header<'a>(r: &'a ResolvedRequest, name: &str) -> Option<&'a str> {
        r.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
        list.iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    // Bruno `mergeHeaders` (bruno-cli/src/utils/collection.js:99-140) keys a Map by the exact
    // header name: a later scope replaces the value but keeps the first position.
    #[test]
    fn headers_merge_by_exact_name_later_wins() {
        let f = fixture(&[
            (
                "collection.bru",
                "headers {\n  X-A: collection\n  X-B: collection\n}\n",
            ),
            ("d/folder.bru", "headers {\n  X-C: folder\n}\n"),
            (
                "d/r.bru",
                "get {\n  url: http://h\n}\n\nheaders {\n  X-A: request\n}\n",
            ),
        ]);
        let r = run(&f, "d/r.bru", None, &[]).unwrap();
        assert_eq!(
            r.headers,
            pairs(&[("X-A", "request"), ("X-B", "collection"), ("X-C", "folder")])
        );
    }

    // Names differing only by case stay separate Map/object keys in Bruno
    // (collection.js:99-140, prepare-request.js:33-35). axios 1.18 `AxiosHeaders`
    // then folds them case-insensitively: the first key's spelling and position,
    // the value of the last key in object order. Here `X-A` (first, value
    // "request") and `x-a` (second, value "folder") fold to `X-A: folder`.
    #[test]
    fn case_variant_headers_fold_like_axios() {
        let f = fixture(&[
            ("collection.bru", "headers {\n  X-A: collection\n}\n"),
            ("d/folder.bru", "headers {\n  x-a: folder\n}\n"),
            (
                "d/r.bru",
                "get {\n  url: http://h\n}\n\nheaders {\n  X-A: request\n}\n",
            ),
        ]);
        let r = run(&f, "d/r.bru", None, &[]).unwrap();
        assert_eq!(r.headers, pairs(&[("X-A", "folder")]));
    }

    // prepare-request.js:258 sets `headers['Authorization']` after the explicit headers,
    // so it is the later key and its value wins the axios fold; the spelling stays the
    // explicit header's.
    #[test]
    fn bearer_overrides_lowercase_authorization() {
        let f = fixture(&[(
            "r.bru",
            "get {\n  url: http://h\n  auth: bearer\n}\n\nheaders {\n  authorization: mine\n}\n\nauth:bearer {\n  token: t\n}\n",
        )]);
        let r = run(&f, "r.bru", None, &[]).unwrap();
        assert_eq!(r.headers, pairs(&[("authorization", "Bearer t")]));
    }

    // prepare-request.js:34 only sends enabled headers with a non-empty name.
    #[test]
    fn empty_header_name_skipped() {
        let f = fixture(&[(
            "r.bru",
            "get {\n  url: http://h\n}\n\nheaders {\n  : v\n  x: 1\n}\n",
        )]);
        assert_eq!(
            run(&f, "r.bru", None, &[]).unwrap().headers,
            pairs(&[("x", "1")])
        );
    }

    // interpolate-vars.js:83-86 interpolates header names as well as values.
    #[test]
    fn header_names_interpolated() {
        let f = fixture(&[(
            "r.bru",
            "get {\n  url: http://h\n}\n\nheaders {\n  {{hn}}: {{hv}}\n}\n\nvars:pre-request {\n  hn: X-N\n  hv: 1\n}\n",
        )]);
        assert_eq!(
            run(&f, "r.bru", None, &[]).unwrap().headers,
            pairs(&[("X-N", "1")])
        );
    }

    #[test]
    fn disabled_headers_not_sent() {
        let f = fixture(&[(
            "r.bru",
            "get {\n  url: http://h\n}\n\nheaders {\n  ~x-off: 1\n  x-on: 2\n}\n",
        )]);
        let r = run(&f, "r.bru", None, &[]).unwrap();
        assert_eq!(r.headers, [("x-on".to_string(), "2".to_string())]);
    }

    #[test]
    fn auth_inherits_from_folder_then_collection() {
        let f = fixture(&[
            (
                "collection.bru",
                "auth {\n  mode: bearer\n}\n\nauth:bearer {\n  token: coll\n}\n",
            ),
            (
                "a/folder.bru",
                "auth {\n  mode: basic\n}\n\nauth:basic {\n  username: u\n  password: p\n}\n",
            ),
            ("a/r.bru", "get {\n  url: http://h\n  auth: inherit\n}\n"),
            ("b/folder.bru", "auth {\n  mode: none\n}\n"),
            ("b/r.bru", "get {\n  url: http://h\n  auth: inherit\n}\n"),
        ]);
        assert_eq!(
            header(&run(&f, "a/r.bru", None, &[]).unwrap(), "authorization"),
            Some("Basic dTpw")
        );
        assert_eq!(
            header(&run(&f, "b/r.bru", None, &[]).unwrap(), "authorization"),
            Some("Bearer coll")
        );
    }

    #[test]
    fn path_params_substituted() {
        let f = fixture(&[(
            "r.bru",
            "get {\n  url: http://h/users/:id?x=:id\n}\n\nparams:path {\n  id: {{uid}}\n}\n\nvars:pre-request {\n  uid: 7\n}\n",
        )]);
        assert_eq!(
            run(&f, "r.bru", None, &[]).unwrap().url,
            "http://h/users/7?x=:id"
        );
    }

    #[test]
    fn query_not_duplicated() {
        let f = fixture(&[(
            "r.bru",
            "get {\n  url: http://h/?a=1\n}\n\nparams:query {\n  a: 1\n}\n",
        )]);
        assert_eq!(run(&f, "r.bru", None, &[]).unwrap().url, "http://h/?a=1");
    }

    // Secret names get '' (envToJson.js:168, :325); `--env-var` values win over the file.
    #[test]
    fn env_override_wins() {
        let f = fixture(&[
            (
                "environments/dev.bru",
                "vars {\n  token: a\n}\nvars:secret [\n  secret,\n  ~off\n]\n",
            ),
            (
                "r.bru",
                "get {\n  url: http://h/{{secret}}/{{off}}\n  auth: bearer\n}\n\nauth:bearer {\n  token: {{token}}\n}\n",
            ),
        ]);
        let r = run(&f, "r.bru", Some("dev"), &[("token", "b"), ("secret", "s")]).unwrap();
        assert_eq!(header(&r, "authorization"), Some("Bearer b"));
        assert_eq!(r.url, "http://h/s/{{off}}");
        let r = run(&f, "r.bru", Some("dev"), &[]).unwrap();
        assert_eq!(header(&r, "authorization"), Some("Bearer a"));
        assert_eq!(r.url, "http://h//{{off}}");
    }

    // `@name` vars are request vars named `name` (bruToJson.js:1155-1165).
    #[test]
    fn local_var_resolves() {
        let f = fixture(&[(
            "r.bru",
            "get {\n  url: http://h/{{token}}\n}\n\nvars:pre-request {\n  @token: abc\n}\n",
        )]);
        assert_eq!(run(&f, "r.bru", None, &[]).unwrap().url, "http://h/abc");
    }

    #[test]
    fn json_content_type_default_and_explicit() {
        let body = "post {\n  url: http://h\n  body: json\n}\n\nbody:json {\n  {\"a\": \"{{v}}\"}\n}\n\nvars:pre-request {\n  v: 1\n}\n";
        let f = fixture(&[
            ("r.bru", body),
            (
                "e.bru",
                &format!("{body}\nheaders {{\n  Content-Type: application/vnd+json\n}}\n"),
            ),
        ]);
        let r = run(&f, "r.bru", None, &[]).unwrap();
        assert_eq!(r.headers, pairs(&[("content-type", "application/json")]));
        assert_eq!(r.body, ResolvedBody::Bytes(br#"{"a": "1"}"#.to_vec()));
        let r = run(&f, "e.bru", None, &[]).unwrap();
        assert_eq!(
            r.headers,
            pairs(&[("Content-Type", "application/vnd+json")])
        );
    }

    #[test]
    fn unsupported_auth_is_error() {
        let f = fixture(&[("r.bru", "get {\n  url: http://h\n  auth: oauth2\n}\n")]);
        assert!(matches!(
            run(&f, "r.bru", None, &[]),
            Err(ResolveError::Unsupported(_))
        ));
    }

    // interpolate-vars.js:6-22 `hasResolvablePathParamValue`: blank or disabled values keep `:name`.
    #[test]
    fn blank_or_disabled_path_param_kept() {
        let f = fixture(&[(
            "r.bru",
            "get {\n  url: http://h/users/:id/:x\n}\n\nparams:path {\n  id:\n  ~x: 1\n}\n",
        )]);
        assert_eq!(
            run(&f, "r.bru", None, &[]).unwrap().url,
            "http://h/users/:id/:x"
        );
    }

    // interpolate-vars.js:144-198 rebuilds the URL from `new URL(...)`:
    // origin + substituted path + trailing slash + search. The fragment is dropped.
    #[test]
    fn path_params_rebuild_url_like_bruno() {
        let f = fixture(&[(
            "r.bru",
            "get {\n  url: localhost:3000/users/:id/People(:key)/?q=1#frag\n}\n\nparams:path {\n  id: 7\n  key: 5\n}\n",
        )]);
        assert_eq!(
            run(&f, "r.bru", None, &[]).unwrap().url,
            "http://localhost:3000/users/7/People(5)/?q=1"
        );
    }

    // run-single-request.js:391-393: `http://` is prepended when the interpolated URL has
    // no explicit `scheme://` (bruno-common/src/utils/url/index.ts getExplicitScheme).
    #[test]
    fn url_without_scheme_gets_http() {
        let f = fixture(&[
            ("a.bru", "get {\n  url: localhost:3000/x\n}\n"),
            (
                "b.bru",
                "get {\n  url: {{host}}/x\n}\n\nvars:pre-request {\n  host: localhost:1\n}\n",
            ),
            ("c.bru", "get {\n  url: ftp://h/x\n}\n"),
            ("d.bru", "get {\n  url: HTTPS://h/x\n}\n"),
        ]);
        assert_eq!(
            run(&f, "a.bru", None, &[]).unwrap().url,
            "http://localhost:3000/x"
        );
        assert_eq!(
            run(&f, "b.bru", None, &[]).unwrap().url,
            "http://localhost:1/x"
        );
        assert_eq!(run(&f, "c.bru", None, &[]).unwrap().url, "ftp://h/x");
        assert_eq!(run(&f, "d.bru", None, &[]).unwrap().url, "HTTPS://h/x");
    }

    // prepare-request.js:363-374: comments are stripped with `decomment`; an empty JSON
    // body sends no body and no default content type.
    #[test]
    fn json_comments_stripped_and_empty_body_omitted() {
        let body = "post {\n  url: http://h\n  body: json\n}\n\nbody:json {\n  {\n    // note\n    \"a\": \"x // y\", /* inline */ \"b\": 1\n  }\n}\n";
        let f = fixture(&[
            ("r.bru", body),
            (
                "e.bru",
                "post {\n  url: http://h\n  body: json\n}\n\nbody:json {\n}\n",
            ),
        ]);
        let r = run(&f, "r.bru", None, &[]).unwrap();
        assert_eq!(
            r.body,
            ResolvedBody::Bytes(b"{\n  \"a\": \"x // y\",  \"b\": 1\n}".to_vec())
        );
        let r = run(&f, "e.bru", None, &[]).unwrap();
        assert_eq!(r.body, ResolvedBody::None);
        assert!(r.headers.is_empty());
    }

    // prepare-request.js:429-441: default content types for form and multipart bodies.
    #[test]
    fn form_and_multipart_default_content_types() {
        let f = fixture(&[
            (
                "f.bru",
                "post {\n  url: http://h\n  body: formUrlEncoded\n}\n\nbody:form-urlencoded {\n  a: 1\n}\n",
            ),
            (
                "m.bru",
                "post {\n  url: http://h\n  body: multipartForm\n}\n\nbody:multipart-form {\n  a: 1\n}\n\nheaders {\n  Content-Type: multipart/mixed\n}\n",
            ),
        ]);
        assert_eq!(
            run(&f, "f.bru", None, &[]).unwrap().headers,
            pairs(&[("content-type", "application/x-www-form-urlencoded")])
        );
        assert_eq!(
            run(&f, "m.bru", None, &[]).unwrap().headers,
            pairs(&[("Content-Type", "multipart/mixed")])
        );
    }
}
