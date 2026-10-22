//! Variable scopes and `{{name}}` interpolation.

use std::collections::BTreeMap;

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

/// Nested references deeper than this are left as written (also stops cycles).
const MAX_DEPTH: usize = 10;

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
    pub fn interpolate(&self, input: &str) -> String {
        self.interpolate_at(input, 0)
    }

    fn interpolate_at(&self, input: &str, depth: usize) -> String {
        let mut out = String::with_capacity(input.len());
        let mut rest = input;
        while let Some(start) = rest.find("{{") {
            out.push_str(&rest[..start]);
            let after = &rest[start + 2..];
            let Some(end) = after.find("}}") else {
                out.push_str(&rest[start..]);
                return out;
            };
            let inner = &after[..end];
            let name = inner.trim();
            match self.get(name) {
                Some(value) if !name.is_empty() && !inner.contains('{') && depth < MAX_DEPTH => {
                    out.push_str(&self.interpolate_at(value, depth + 1));
                }
                _ => out.push_str(&rest[start..start + 2 + end + 2]),
            }
            rest = &after[end + 2..];
        }
        out.push_str(rest);
        out
    }
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
        assert_eq!(stack.interpolate("{{ host }}"), "h");
        assert_eq!(stack.interpolate("a {{ b"), "a {{ b");
        assert_eq!(stack.interpolate("{{host}}{{nope}}{{host}}"), "h{{nope}}h");
    }

    #[test]
    fn enabled_map_skips_disabled() {
        let items = [
            KeyValue::new("a", "1"),
            KeyValue {
                name: "b".into(),
                value: "2".into(),
                enabled: false,
            },
        ];
        assert_eq!(enabled_map(&items), map(&[("a", "1")]));
    }
}
