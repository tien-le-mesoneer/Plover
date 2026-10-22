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
    /// `content_type` is set only when no `content-type` header was given.
    Bytes {
        content_type: Option<String>,
        data: Vec<u8>,
    },
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

    let mut headers = Vec::new();
    let header_lists = collection_scope
        .map(|s| &s.headers)
        .into_iter()
        .chain(folder_scopes.iter().map(|s| &s.headers))
        .chain(std::iter::once(&request.headers));
    for list in header_lists {
        for h in list.iter().filter(|h| h.enabled) {
            set_header(
                &mut headers,
                &vars.interpolate(&h.name),
                vars.interpolate(&h.value),
            );
        }
    }

    let auth = effective_auth(
        &request.auth,
        collection_scope.map(|s| &s.auth),
        folder_scopes.iter().map(|s| &s.auth),
    );
    match auth {
        Auth::Basic { username, password } => {
            let raw = format!(
                "{}:{}",
                vars.interpolate(username),
                vars.interpolate(password)
            );
            let encoded = base64::engine::general_purpose::STANDARD.encode(raw);
            set_header(&mut headers, "Authorization", format!("Basic {encoded}"));
        }
        Auth::Bearer { token } => {
            set_header(
                &mut headers,
                "Authorization",
                format!("Bearer {}", vars.interpolate(token)),
            );
        }
        Auth::Unsupported(mode) => {
            return Err(ResolveError::Unsupported(format!("auth mode `{mode}`")));
        }
        Auth::None | Auth::Inherit => {}
    }

    let url = substitute_path_params(&vars.interpolate(&request.url), &request.path_params, &vars);
    let has_content_type = headers
        .iter()
        .any(|(k, _)| k.eq_ignore_ascii_case("content-type"));
    let bytes = |text: &str, ct: &str| ResolvedBody::Bytes {
        content_type: (!has_content_type).then(|| ct.to_owned()),
        data: vars.interpolate(text).into_bytes(),
    };
    let body = match &request.body {
        Body::None => ResolvedBody::None,
        Body::Json(t) => bytes(t, "application/json"),
        Body::Text(t) => bytes(t, "text/plain"),
        Body::Xml(t) => bytes(t, "application/xml"),
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

/// Replaces a header case-insensitively in place, or appends it.
fn set_header(headers: &mut Vec<(String, String)>, name: &str, value: String) {
    match headers
        .iter_mut()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
    {
        Some(entry) => *entry = (name.to_owned(), value),
        None => headers.push((name.to_owned(), value)),
    }
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

    const COLLECTION_HEADERS: &str = "headers {\n  x-a: collection\n  x-b: collection\n}\n";

    #[test]
    fn headers_merge_request_wins() {
        let f = fixture(&[
            ("collection.bru", COLLECTION_HEADERS),
            ("d/folder.bru", "headers {\n  X-B: folder\n}\n"),
            (
                "d/r.bru",
                "get {\n  url: http://h\n}\n\nheaders {\n  x-a: request\n}\n",
            ),
        ]);
        let r = run(&f, "d/r.bru", None, &[]).unwrap();
        assert_eq!(header(&r, "x-a"), Some("request"));
        assert_eq!(header(&r, "x-b"), Some("folder"));
        assert_eq!(r.headers.len(), 2);
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
        assert_eq!(
            r.body,
            ResolvedBody::Bytes {
                content_type: Some("application/json".into()),
                data: br#"{"a": "1"}"#.to_vec()
            }
        );
        let r = run(&f, "e.bru", None, &[]).unwrap();
        assert!(matches!(
            r.body,
            ResolvedBody::Bytes {
                content_type: None,
                ..
            }
        ));
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
