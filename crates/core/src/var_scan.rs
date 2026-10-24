//! Variable names scripts set at runtime (`bru.setVar("token", …)`, `vars:post-response`),
//! found by reading the collection's scripts without running them. The UI marks such a
//! `{{token}}` "set later" instead of "undefined".

use std::collections::HashSet;
use std::path::Path;
use std::sync::OnceLock;

use regex::Regex;

use crate::collection::{Collection, Item};
use crate::request::KeyValue;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetLater {
    pub name: String,
    /// `/`-separated path of the first file that sets it (`auth/login.bru`,
    /// `auth/folder.bru`, `collection.bru`).
    pub by: String,
}

/// Every literal name set by a script or a `vars:post-response` block, first setter in
/// run order (collection, then each request's folders and the request itself).
pub fn scan_set_later(collection: &Collection) -> Vec<SetLater> {
    let mut found = Scan::default();
    if let Some(scope) = &collection.scope {
        found.file(
            "collection.bru",
            [&scope.script_pre, &scope.script_post, &scope.tests],
            &scope.vars_post,
        );
    }
    for r in collection.requests_under(&collection.root) {
        for folder in &r.folders {
            if let Some(scope) = &folder.scope {
                let by = format!("{}/folder.bru", relative(&collection.root, &folder.path));
                found.file(
                    &by,
                    [&scope.script_pre, &scope.script_post, &scope.tests],
                    &scope.vars_post,
                );
            }
        }
        if let Item::Request { file, .. } = r.item {
            let q = &file.request;
            found.file(
                &relative(&collection.root, r.path),
                [&q.script_pre, &q.script_post, &q.tests],
                &q.vars_post,
            );
        }
    }
    found.out
}

#[derive(Default)]
struct Scan {
    seen: HashSet<String>,
    out: Vec<SetLater>,
}

impl Scan {
    fn file(&mut self, by: &str, scripts: [&Option<String>; 3], vars_post: &[KeyValue]) {
        static SET: OnceLock<Regex> = OnceLock::new();
        let set = SET.get_or_init(|| {
            Regex::new(r#"bru\.set(?:Env|Global(?:Env)?)?Var\(\s*["'`]([^"'`]+)["'`]"#)
                .expect("valid setVar regex")
        });
        let from_scripts = scripts
            .into_iter()
            .flatten()
            .flat_map(|s| set.captures_iter(s).map(|c| c[1].to_owned()))
            // `${…}` in a template literal is a computed name.
            .filter(|n| !n.contains("${"));
        let from_vars = vars_post
            .iter()
            .filter(|kv| kv.enabled)
            .map(|kv| kv.name.clone());
        for name in from_scripts.chain(from_vars).collect::<Vec<_>>() {
            if self.seen.insert(name.clone()) {
                self.out.push(SetLater {
                    name,
                    by: by.to_owned(),
                });
            }
        }
    }
}

fn relative(root: &Path, path: &Path) -> String {
    let rel = path.strip_prefix(root).unwrap_or(path);
    rel.components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}
