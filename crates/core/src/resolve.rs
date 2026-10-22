//! Turns a request plus its folder/collection/environment scopes into a
//! concrete `ResolvedRequest`. Pure: no file or network I/O.

use std::path::PathBuf;

use base64::Engine;

use crate::collection::{Collection, Environment, Item, RequestRef};
use crate::request::{Auth, Body, KeyValue, Method, MultipartValue, RequestSettings};
use crate::vars::{VarMap, VarStack, enabled_map};

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
    /// `--env-var` values; they win over the environment file (and supply `vars:secret`).
    pub env_overrides: &'a VarMap,
    pub runtime: &'a VarMap,
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ResolveError {
    #[error("{0} is not a runnable request")]
    NotARequest(PathBuf),
    #[error("unsupported {0}")]
    Unsupported(String),
}

pub fn resolve(
    ctx: &ResolveContext<'_>,
    req: &RequestRef<'_>,
) -> Result<ResolvedRequest, ResolveError> {
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

    let mut environment = ctx
        .environment
        .map(|e| enabled_map(&e.file.vars))
        .unwrap_or_default();
    environment.extend(
        ctx.env_overrides
            .iter()
            .map(|(k, v)| (k.clone(), v.clone())),
    );
    let vars = VarStack::new(
        ctx.runtime.clone(),
        enabled_map(&request.vars_pre),
        folder_scopes
            .iter()
            .map(|s| enabled_map(&s.vars_pre))
            .collect(),
        environment,
        collection_scope
            .map(|s| enabled_map(&s.vars_pre))
            .unwrap_or_default(),
    );

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
    match auth {
        Auth::Bearer { token } => {
            set_exact(&mut headers, "Authorization", format!("Bearer {token}"))
        }
        Auth::Unsupported(mode) => {
            return Err(ResolveError::Unsupported(format!("auth mode `{mode}`")));
        }
        Auth::None | Auth::Inherit | Auth::Basic { .. } => {}
    }

    // 3. Default content type for raw bodies (prepare-request.js:366-386), lowercase key.
    let default_content_type = match &request.body {
        Body::Json(_) => Some("application/json"),
        Body::Text(_) => Some("text/plain"),
        Body::Xml(_) => Some("application/xml"),
        _ => None,
    };
    if let Some(ct) = default_content_type.filter(|_| !content_type_defined) {
        set_exact(&mut headers, "content-type", ct.to_owned());
    }

    // 4. interpolate-vars.js:83-86: each key is deleted and re-set interpolated, in order.
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
    if let Auth::Basic { username, password } = auth {
        let raw = format!(
            "{}:{}",
            vars.interpolate(username),
            vars.interpolate(password)
        );
        let encoded = base64::engine::general_purpose::STANDARD.encode(raw);
        set_exact(&mut headers, "Authorization", format!("Basic {encoded}"));
    }
    let headers = fold_case_variants(headers);

    let url = substitute_path_params(&vars.interpolate(&request.url), &request.path_params, &vars);
    let body = match &request.body {
        Body::None => ResolvedBody::None,
        Body::Json(t) | Body::Text(t) | Body::Xml(t) => {
            ResolvedBody::Bytes(vars.interpolate(t).into_bytes())
        }
        Body::FormUrlEncoded(items) => ResolvedBody::Form(
            items
                .iter()
                .filter(|kv| kv.enabled)
                .map(|kv| (vars.interpolate(&kv.name), vars.interpolate(&kv.value)))
                .collect(),
        ),
        Body::Multipart(fields) => {
            let mut parts = Vec::new();
            for field in fields.iter().filter(|f| f.enabled) {
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
                            path: ctx.collection.root.join(vars.interpolate(f)),
                            content_type: content_type.clone(),
                        }))
                    }
                }
            }
            ResolvedBody::Multipart(parts)
        }
        Body::Unsupported(mode) => {
            return Err(ResolveError::Unsupported(format!("body mode `{mode}`")));
        }
    };

    Ok(ResolvedRequest {
        method: request.method.clone(),
        url,
        headers,
        body,
        settings: request.settings.clone(),
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
fn fold_case_variants(headers: Vec<(String, String)>) -> Vec<(String, String)> {
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

/// Replaces `:name` path segments with enabled path params; query and fragment are untouched.
fn substitute_path_params(url: &str, params: &[KeyValue], vars: &VarStack) -> String {
    if params.iter().all(|p| !p.enabled) {
        return url.to_owned();
    }
    let path_start = url
        .find("://")
        .map(|i| url[i + 3..].find('/').map_or(url.len(), |j| i + 3 + j))
        .unwrap_or(0);
    let path_end = url[path_start..]
        .find(['?', '#'])
        .map_or(url.len(), |i| path_start + i);
    let path = url[path_start..path_end]
        .split('/')
        .map(|segment| {
            let name = segment.strip_prefix(':');
            match name.and_then(|n| params.iter().find(|p| p.enabled && p.name == n)) {
                Some(p) => vars.interpolate(&p.value),
                None => segment.to_owned(),
            }
        })
        .collect::<Vec<_>>()
        .join("/");
    format!("{}{}{}", &url[..path_start], path, &url[path_end..])
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
        let overrides: VarMap = overrides
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        let runtime = VarMap::new();
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

    #[test]
    fn env_override_wins() {
        let f = fixture(&[
            (
                "environments/dev.bru",
                "vars {\n  token: a\n}\nvars:secret [\n  secret\n]\n",
            ),
            (
                "r.bru",
                "get {\n  url: http://h/{{secret}}\n  auth: bearer\n}\n\nauth:bearer {\n  token: {{token}}\n}\n",
            ),
        ]);
        let r = run(&f, "r.bru", Some("dev"), &[("token", "b"), ("secret", "s")]).unwrap();
        assert_eq!(header(&r, "authorization"), Some("Bearer b"));
        assert_eq!(r.url, "http://h/s");
        let r = run(&f, "r.bru", Some("dev"), &[]).unwrap();
        assert_eq!(header(&r, "authorization"), Some("Bearer a"));
        assert_eq!(r.url, "http://h/{{secret}}");
    }

    // prepare-request.js:36 detects an explicit content type case-insensitively;
    // only without one does :366-367 add `content-type: application/json`.
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
}
