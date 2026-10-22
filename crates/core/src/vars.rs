//! Variable scopes and `{{name}}` interpolation.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::request::KeyValue;

pub type VarMap = BTreeMap<String, String>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Runtime,
    Request,
    Folder,
    Environment,
    Collection,
}

/// Highest precedence first. Matches Bruno (`bruno-cli/src/runner/interpolate-vars.js`).
pub const PRECEDENCE: [Scope; 5] = [
    Scope::Runtime,
    Scope::Request,
    Scope::Folder,
    Scope::Environment,
    Scope::Collection,
];

#[derive(Debug, Clone, Default)]
pub struct VarStack {
    /// Lookup order: first layer that has the name wins.
    layers: Vec<VarMap>,
}

impl VarStack {
    pub fn new(
        runtime: VarMap,
        request: VarMap,
        folders_outer_to_inner: Vec<VarMap>,
        environment: VarMap,
        collection: VarMap,
    ) -> VarStack {
        let (mut runtime, mut request, mut environment, mut collection) = (
            Some(runtime),
            Some(request),
            Some(environment),
            Some(collection),
        );
        let mut folders = folders_outer_to_inner;
        let mut layers = Vec::new();
        for scope in PRECEDENCE {
            match scope {
                Scope::Runtime => layers.extend(runtime.take()),
                Scope::Request => layers.extend(request.take()),
                Scope::Folder => layers.extend(folders.drain(..).rev()),
                Scope::Environment => layers.extend(environment.take()),
                Scope::Collection => layers.extend(collection.take()),
            }
        }
        VarStack { layers }
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.layers
            .iter()
            .find_map(|layer| layer.get(name))
            .map(String::as_str)
    }

    /// Replaces `{{name}}` placeholders. Unknown names stay as written.
    ///
    /// Port of bruno-common/src/interpolate/index.ts `replace`: placeholders are
    /// `{{[^}]+}}` looked up verbatim; each one is resolved at most once per call
    /// (`results`), and `visited` stops self-reference.
    pub fn interpolate(&self, input: &str) -> String {
        self.replace(input, &mut HashSet::new(), &mut HashMap::new())
    }

    fn replace(
        &self,
        input: &str,
        visited: &mut HashSet<String>,
        results: &mut HashMap<String, String>,
    ) -> String {
        let mut current = input.to_owned();
        loop {
            let mut match_found = false;
            // A fresh global regex per pass: `test()` below shares its `lastIndex`.
            let mut last_index = 0;
            let mut out = String::with_capacity(current.len());
            let mut pos = 0;
            for (start, end) in placeholders(&current) {
                out.push_str(&current[pos..start]);
                pos = end;
                let whole = &current[start..end];
                let replacement = self.get(&current[start + 2..end - 2]);
                if let Some(cached) = results.get(whole) {
                    out.push_str(cached);
                    continue;
                }
                // `patternRegex.test(replacement)` on the shared global regex.
                let haystack = replacement.unwrap_or("undefined");
                let has_placeholder = match haystack
                    .get(last_index..)
                    .and_then(|h| placeholders(h).next())
                {
                    Some((_, e)) => {
                        last_index += e;
                        true
                    }
                    None => {
                        last_index = 0;
                        false
                    }
                };
                let result = match replacement {
                    Some(r) if has_placeholder && !visited.contains(whole) => {
                        visited.insert(whole.to_owned());
                        self.replace(r, visited, results)
                    }
                    _ => {
                        visited.insert(whole.to_owned());
                        replacement.unwrap_or(whole).to_owned()
                    }
                };
                results.insert(whole.to_owned(), result.clone());
                match_found = true;
                out.push_str(&result);
            }
            out.push_str(&current[pos..]);
            current = out;
            if !match_found {
                return current;
            }
        }
    }
}

/// Byte ranges of `\{\{([^}]+)\}\}` matches, leftmost and non-overlapping.
fn placeholders(s: &str) -> impl Iterator<Item = (usize, usize)> + '_ {
    let mut from = 0;
    std::iter::from_fn(move || {
        while let Some(offset) = s.get(from..)?.find("{{") {
            let start = from + offset;
            let inner = &s[start + 2..];
            let len = inner.find('}').unwrap_or(inner.len());
            if len > 0 && inner[len..].starts_with("}}") {
                let end = start + 2 + len + 2;
                from = end;
                return Some((start, end));
            }
            from = start + 1;
        }
        None
    })
}

/// Enabled entries as a map; a later entry with the same name wins.
pub fn enabled_map(items: &[KeyValue]) -> VarMap {
    items
        .iter()
        .filter(|kv| kv.enabled)
        .map(|kv| (kv.name.clone(), kv.value.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(pairs: &[(&str, &str)]) -> VarMap {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn precedence_matches_bruno() {
        let host = |v: &str| map(&[("host", v)]);
        let mut scopes = vec![
            host("runtime"),
            host("request"),
            host("folder"),
            host("env"),
            host("collection"),
        ];
        for expected in ["runtime", "request", "folder", "env", "collection"] {
            let s = &scopes;
            let stack = VarStack::new(
                s[0].clone(),
                s[1].clone(),
                vec![s[2].clone()],
                s[3].clone(),
                s[4].clone(),
            );
            assert_eq!(stack.get("host"), Some(expected));
            let i = scopes.iter().position(|m| !m.is_empty()).unwrap();
            scopes[i].clear();
        }
    }

    #[test]
    fn inner_folder_beats_outer() {
        let outer = map(&[("x", "outer")]);
        let inner = map(&[("x", "inner")]);
        let stack = VarStack::new(
            VarMap::new(),
            VarMap::new(),
            vec![outer, inner],
            VarMap::new(),
            VarMap::new(),
        );
        assert_eq!(stack.get("x"), Some("inner"));
    }

    #[test]
    fn nested_reference() {
        let env = map(&[("base", "{{host}}/api"), ("host", "http://h")]);
        let stack = VarStack::new(VarMap::new(), VarMap::new(), vec![], env, VarMap::new());
        assert_eq!(stack.interpolate("{{base}}/x"), "http://h/api/x");
    }

    #[test]
    fn cycle_terminates() {
        let env = map(&[("a", "{{b}}"), ("b", "{{a}}")]);
        let stack = VarStack::new(VarMap::new(), VarMap::new(), vec![], env, VarMap::new());
        assert!(stack.interpolate("{{a}}").contains("{{"));
    }

    #[test]
    fn unknown_left_literal() {
        let env = map(&[("host", "h")]);
        let stack = VarStack::new(VarMap::new(), VarMap::new(), vec![], env, VarMap::new());
        assert_eq!(stack.interpolate("{{nope}}"), "{{nope}}");
        // Bruno looks up the placeholder text as written (`get(obj, " host ")`), so spaces don't match.
        assert_eq!(stack.interpolate("{{ host }}"), "{{ host }}");
        assert_eq!(stack.interpolate("a {{ b"), "a {{ b");
        assert_eq!(stack.interpolate("{{host}}{{nope}}{{host}}"), "h{{nope}}h");
    }

    #[test]
    fn enabled_map_skips_disabled() {
        let items = [
            KeyValue::new("a", "1"),
            KeyValue {
                local: false,
                name: "b".into(),
                value: "2".into(),
                enabled: false,
            },
        ];
        assert_eq!(enabled_map(&items), map(&[("a", "1")]));
    }

    // bruno-common/src/interpolate/index.ts `replace`: each placeholder is resolved once
    // and cached in `results`. For `a = {{a}}x8` Bruno's own trace gives a fixed
    // 8^6 copies of `{{a}}` (the inner call expands twice, the outer once more),
    // instead of growing without bound.
    #[test]
    fn fan_out_is_bounded() {
        let env = map(&[("a", &"{{a}}".repeat(8))]);
        let stack = VarStack::new(VarMap::new(), VarMap::new(), vec![], env, VarMap::new());
        let start = std::time::Instant::now();
        let out = stack.interpolate("{{a}}");
        assert_eq!(out.len(), 8usize.pow(6) * "{{a}}".len());
        assert!(start.elapsed() < std::time::Duration::from_secs(2));
    }

    #[test]
    fn repeated_placeholders_resolve() {
        let env = map(&[("a", "1"), ("b", "{{a}}{{a}}")]);
        let stack = VarStack::new(VarMap::new(), VarMap::new(), vec![], env, VarMap::new());
        assert_eq!(stack.interpolate("{{b}}-{{a}}-{{b}}"), "11-1-11");
    }
}
