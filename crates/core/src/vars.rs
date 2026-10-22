//! Variable scopes and `{{name}}` interpolation.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::request::KeyValue;

pub type VarMap = BTreeMap<String, String>;

/// Variables scripts can set to any JSON value (`bru.setVar('a', {b: 1})`).
pub type ValueMap = BTreeMap<String, serde_json::Value>;

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

/// Most placeholder levels resolved inside one another. Deeper references stay literal.
/// (Bruno's recursive `replace` overflows the JS stack a few thousand levels down.)
pub const MAX_INTERPOLATION_DEPTH: usize = 1000;

/// Largest result interpolation may produce for request parts (URL, headers, body).
/// Scripts use their memory limit instead.
pub const MAX_INTERPOLATED_BYTES: usize = 64 * 1024 * 1024;

/// Bounds for one interpolation call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InterpolateLimits {
    /// Largest output string; everything built along the way (passes and cached
    /// results) may total 2× this.
    pub max_bytes: usize,
    pub deadline: Option<std::time::Instant>,
}

impl Default for InterpolateLimits {
    fn default() -> Self {
        InterpolateLimits {
            max_bytes: MAX_INTERPOLATED_BYTES,
            deadline: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InterpolateError {
    #[error("interpolating variables produced more than {0} bytes")]
    TooLarge(usize),
    #[error("interpolating variables took too long")]
    Timeout,
}

#[derive(Debug, Clone, Default)]
pub struct VarStack {
    /// Lookup order: first layer that has the name wins.
    layers: Vec<ValueMap>,
}

impl VarStack {
    pub fn new(
        runtime: VarMap,
        request: VarMap,
        folders_outer_to_inner: Vec<VarMap>,
        environment: VarMap,
        collection: VarMap,
    ) -> VarStack {
        VarStack::from_values(
            string_values(&runtime),
            string_values(&request),
            folders_outer_to_inner.iter().map(string_values).collect(),
            string_values(&environment),
            string_values(&collection),
        )
    }

    /// Like `new`, with variables that may hold any JSON value.
    pub fn from_values(
        runtime: ValueMap,
        request: ValueMap,
        folders_outer_to_inner: Vec<ValueMap>,
        environment: ValueMap,
        collection: ValueMap,
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

    /// The text a top-level variable interpolates to.
    pub fn get(&self, name: &str) -> Option<String> {
        self.top(name).map(value_text)
    }

    fn top(&self, name: &str) -> Option<&serde_json::Value> {
        self.layers.iter().find_map(|layer| layer.get(name))
    }

    /// lodash `get` on the merged variables (bruno-common interpolate: `get(obj, placeholder)`):
    /// a name that exists as written wins; otherwise `a.b[0]["c.d"]` walks into object values.
    /// `None` is `undefined`; objects become their JSON text, `null` becomes `"null"`.
    pub fn lookup(&self, placeholder: &str) -> Option<String> {
        if let Some(v) = self.top(placeholder) {
            return Some(value_text(v));
        }
        let path = lodash_path(placeholder);
        let (first, rest) = path.split_first()?;
        let mut current = self.top(first)?;
        for key in rest {
            current = match current {
                serde_json::Value::Object(map) => map.get(key)?,
                serde_json::Value::Array(items) => items.get(key.parse::<usize>().ok()?)?,
                serde_json::Value::String(s) if key == "length" => {
                    return Some(s.encode_utf16().count().to_string());
                }
                _ => return None,
            };
        }
        Some(value_text(current))
    }

    /// Replaces `{{name}}` placeholders with default limits. Unknown names stay as written.
    /// If a limit is hit the input is returned unchanged; use `try_interpolate` to see why.
    pub fn interpolate(&self, input: &str) -> String {
        self.try_interpolate(input, &InterpolateLimits::default())
            .unwrap_or_else(|_| input.to_owned())
    }

    /// Port of bruno-common/src/interpolate/index.ts `replace`, without recursion: the
    /// nested `replace` calls are frames on an explicit stack. Each placeholder is resolved
    /// at most once per call (`results`), `visited` stops self-reference, placeholders more
    /// than `MAX_INTERPOLATION_DEPTH` levels down stay literal, and `limits` bound the
    /// output size, the total work and the time.
    pub fn try_interpolate(
        &self,
        input: &str,
        limits: &InterpolateLimits,
    ) -> Result<String, InterpolateError> {
        let mut visited: HashSet<String> = HashSet::new();
        let mut results: HashMap<String, String> = HashMap::new();
        let mut budget = Budget {
            max_bytes: limits.max_bytes,
            work_left: limits.max_bytes.saturating_mul(2),
            deadline: limits.deadline,
            steps: 0,
        };
        let mut stack = vec![Frame::new(input.to_owned())];
        let mut returned: Option<String> = None;
        loop {
            let depth = stack.len();
            let Some(frame) = stack.last_mut() else {
                return Ok(String::new());
            };
            if let Some(result) = returned.take() {
                // The nested `replace` for `waiting` finished.
                if let Some(whole) = frame.waiting.take() {
                    budget.charge(result.len())?;
                    results.insert(whole, result.clone());
                }
                frame.match_found = true;
                budget.push(&mut frame.out, &result)?;
                frame.idx += 1;
            }
            let mut child = None;
            while frame.idx < frame.matches.len() {
                budget.tick()?;
                let (start, end) = frame.matches[frame.idx];
                budget.push(&mut frame.out, &frame.input[frame.pos..start])?;
                frame.pos = end;
                let whole = &frame.input[start..end];
                // `results.has(match)` returns before touching `matchFound`.
                if let Some(cached) = results.get(whole) {
                    budget.push(&mut frame.out, cached)?;
                    frame.idx += 1;
                    continue;
                }
                let replacement = self.lookup(&whole[2..whole.len() - 2]);
                budget.charge(replacement.as_ref().map_or(0, String::len))?;
                // `patternRegex.test(replacement)` on the pass's shared global regex.
                let haystack = replacement.as_deref().unwrap_or("undefined");
                let has_placeholder = match haystack
                    .get(frame.last_index..)
                    .and_then(|h| placeholders(h).next())
                {
                    Some((_, e)) => {
                        frame.last_index += e;
                        true
                    }
                    None => {
                        frame.last_index = 0;
                        false
                    }
                };
                let whole = whole.to_owned();
                match replacement {
                    Some(r) if has_placeholder && !visited.contains(&whole) => {
                        visited.insert(whole.clone());
                        if depth > MAX_INTERPOLATION_DEPTH {
                            // Too deep to follow: keep the placeholder as written.
                            results.insert(whole.clone(), whole.clone());
                            frame.match_found = true;
                            budget.push(&mut frame.out, &whole)?;
                            frame.idx += 1;
                            continue;
                        }
                        frame.waiting = Some(whole);
                        child = Some(r);
                        break;
                    }
                    replacement => {
                        visited.insert(whole.clone());
                        let result = replacement.unwrap_or_else(|| whole.clone());
                        budget.push(&mut frame.out, &result)?;
                        results.insert(whole, result);
                        frame.match_found = true;
                        frame.idx += 1;
                    }
                }
            }
            if let Some(r) = child {
                stack.push(Frame::new(r));
                continue;
            }
            // The pass is over; another one runs while placeholders were replaced.
            let tail = frame.input[frame.pos..].to_owned();
            budget.push(&mut frame.out, &tail)?;
            let out = std::mem::take(&mut frame.out);
            if frame.match_found {
                *frame = Frame::new(out);
                continue;
            }
            stack.pop();
            if stack.is_empty() {
                return Ok(out);
            }
            returned = Some(out);
        }
    }
}

/// One `replace(str)` call: the current pass over `input`.
struct Frame {
    input: String,
    matches: Vec<(usize, usize)>,
    idx: usize,
    pos: usize,
    out: String,
    last_index: usize,
    match_found: bool,
    /// The placeholder whose nested `replace` is running in the frame above.
    waiting: Option<String>,
}

impl Frame {
    fn new(input: String) -> Frame {
        let matches = placeholders(&input).collect();
        Frame {
            input,
            matches,
            idx: 0,
            pos: 0,
            out: String::new(),
            last_index: 0,
            match_found: false,
            waiting: None,
        }
    }
}

struct Budget {
    max_bytes: usize,
    work_left: usize,
    deadline: Option<std::time::Instant>,
    steps: u32,
}

impl Budget {
    fn push(&mut self, out: &mut String, text: &str) -> Result<(), InterpolateError> {
        if out.len() + text.len() > self.max_bytes || text.len() > self.work_left {
            return Err(InterpolateError::TooLarge(self.max_bytes));
        }
        self.work_left -= text.len();
        out.push_str(text);
        Ok(())
    }

    fn charge(&mut self, bytes: usize) -> Result<(), InterpolateError> {
        self.work_left = self
            .work_left
            .checked_sub(bytes)
            .ok_or(InterpolateError::TooLarge(self.max_bytes))?;
        Ok(())
    }

    fn tick(&mut self) -> Result<(), InterpolateError> {
        self.steps = self.steps.wrapping_add(1);
        if self.steps.is_multiple_of(256)
            && let Some(deadline) = self.deadline
            && std::time::Instant::now() >= deadline
        {
            return Err(InterpolateError::Timeout);
        }
        Ok(())
    }
}

/// lodash `stringToPath`: `a.b[0]["c.d"]['e']` → `a`, `b`, `0`, `c.d`, `e`.
fn lodash_path(path: &str) -> Vec<String> {
    let chars: Vec<char> = path.chars().collect();
    let mut keys = Vec::new();
    let mut i = 0;
    if chars.first() == Some(&'.') {
        keys.push(String::new());
    }
    while i < chars.len() {
        match chars[i] {
            '.' => {
                i += 1;
                if i == chars.len() || chars[i] == '.' {
                    keys.push(String::new());
                }
            }
            '[' => {
                i += 1;
                let mut key = String::new();
                if i < chars.len() && (chars[i] == '"' || chars[i] == '\'') {
                    let quote = chars[i];
                    i += 1;
                    while i < chars.len() && chars[i] != quote {
                        if chars[i] == '\\' && i + 1 < chars.len() {
                            i += 1;
                        }
                        key.push(chars[i]);
                        i += 1;
                    }
                    i += 1; // closing quote
                    while i < chars.len() && chars[i] != ']' {
                        i += 1;
                    }
                } else {
                    while i < chars.len() && chars[i] != ']' {
                        key.push(chars[i]);
                        i += 1;
                    }
                }
                i += 1; // ]
                keys.push(key.trim().to_owned());
            }
            _ => {
                let mut key = String::new();
                while i < chars.len() && chars[i] != '.' && chars[i] != '[' {
                    key.push(chars[i]);
                    i += 1;
                }
                keys.push(key);
            }
        }
    }
    keys
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

/// The text a value interpolates to: strings as they are, anything else as JSON
/// (bruno-common `interpolate` stringifies objects).
pub fn value_text(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

pub fn value_texts(values: &ValueMap) -> VarMap {
    values
        .iter()
        .map(|(k, v)| (k.clone(), value_text(v)))
        .collect()
}

pub fn string_values(vars: &VarMap) -> ValueMap {
    vars.iter()
        .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
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
            assert_eq!(stack.get("host").as_deref(), Some(expected));
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
        assert_eq!(stack.get("x").as_deref(), Some("inner"));
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

    fn chain(n: usize) -> VarMap {
        (0..n)
            .map(|i| (format!("a{i}"), format!("x{{{{a{}}}}}", i + 1)))
            .collect()
    }

    #[test]
    fn deep_chain_is_iterative_and_stays_literal_past_the_cap() {
        // A small thread stack proves there is no recursion per level.
        let handle = std::thread::Builder::new()
            .stack_size(256 * 1024)
            .spawn(|| {
                let stack = VarStack::new(
                    chain(200_000),
                    VarMap::new(),
                    vec![],
                    VarMap::new(),
                    VarMap::new(),
                );
                stack.interpolate("{{a0}}")
            })
            .unwrap();
        let out = handle.join().unwrap();
        assert_eq!(
            out,
            format!(
                "{}{{{{a{MAX_INTERPOLATION_DEPTH}}}}}",
                "x".repeat(MAX_INTERPOLATION_DEPTH)
            )
        );
        // Chains within the cap resolve fully.
        let stack = VarStack::new(
            chain(10),
            VarMap::new(),
            vec![],
            VarMap::new(),
            VarMap::new(),
        );
        assert_eq!(stack.interpolate("{{a0}}"), "xxxxxxxxxx{{a10}}");
    }

    #[test]
    fn doubling_chain_hits_the_size_cap() {
        let mut env: VarMap = (1..40)
            .map(|i| {
                (
                    format!("v{i}"),
                    format!("{{{{v{}}}}}{{{{v{}}}}}", i - 1, i - 1),
                )
            })
            .collect();
        env.insert("v0".into(), "ab".into());
        let stack = VarStack::new(VarMap::new(), VarMap::new(), vec![], env, VarMap::new());
        let limits = InterpolateLimits {
            max_bytes: 1 << 20,
            deadline: None,
        };
        assert_eq!(
            stack.try_interpolate("{{v39}}", &limits),
            Err(InterpolateError::TooLarge(1 << 20))
        );
        // Small enough: fine.
        assert_eq!(
            stack.try_interpolate("{{v3}}", &limits).unwrap(),
            "ab".repeat(8)
        );
    }

    #[test]
    fn deadline_is_checked() {
        let env = map(&[("a", &"{{b}}".repeat(1000)), ("b", "x")]);
        let stack = VarStack::new(VarMap::new(), VarMap::new(), vec![], env, VarMap::new());
        let past = InterpolateLimits {
            max_bytes: 1 << 20,
            deadline: Some(std::time::Instant::now() - std::time::Duration::from_secs(1)),
        };
        assert_eq!(
            stack.try_interpolate(&"{{a}}{{b}}".repeat(300), &past),
            Err(InterpolateError::Timeout)
        );
    }

    #[test]
    fn object_paths_like_lodash_get() {
        let mut runtime = ValueMap::new();
        runtime.insert(
            "user".into(),
            serde_json::json!({ "id": 42, "tags": ["a", { "b": "x" }], "n": null, "name": "Ana" }),
        );
        runtime.insert("a.b".into(), serde_json::json!("exact"));
        runtime.insert("a".into(), serde_json::json!({ "b": "path" }));
        let stack = VarStack::from_values(
            runtime,
            ValueMap::new(),
            vec![],
            ValueMap::new(),
            ValueMap::new(),
        );
        assert_eq!(stack.interpolate("/users/{{user.id}}"), "/users/42");
        assert_eq!(
            stack.interpolate("{{user.tags[1].b}}|{{user.tags.0}}|{{user['name']}}"),
            "x|a|Ana"
        );
        assert_eq!(stack.interpolate("{{user.n}}"), "null");
        assert_eq!(
            stack.interpolate("{{user.missing}}|{{nope.x}}"),
            "{{user.missing}}|{{nope.x}}"
        );
        assert_eq!(stack.interpolate("{{a.b}}"), "exact");
        assert_eq!(stack.interpolate("{{user.tags}}"), r#"["a",{"b":"x"}]"#);
    }
}
