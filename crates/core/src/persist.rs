//! Script changes to variables, written back to disk the way Bruno 4.3 does after every
//! request (bruno-cli/src/utils/persist-variables.js; the app persists the active
//! environment too, bruno-app/src/providers/App/useIpcEvents.js:233-238). Only the variables
//! that changed are touched, so the rest of the file keeps its bytes.

use std::collections::BTreeSet;

use serde_json::Value;

use crate::bru::syntax::{
    Annotation, BlockContent, BruDocument, ListItem, Pair, PairValue, SyntaxError,
};
use crate::vars::ValueMap;

/// What a request changed in one variable scope.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct VarChanges {
    /// Added or changed, in name order.
    pub set: Vec<(String, Value)>,
    pub deleted: Vec<String>,
}

impl VarChanges {
    pub fn between(before: &ValueMap, after: &ValueMap) -> VarChanges {
        VarChanges {
            set: after
                .iter()
                .filter(|(k, v)| before.get(*k) != Some(*v))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
            deleted: before
                .keys()
                .filter(|k| !after.contains_key(*k))
                .cloned()
                .collect(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.set.is_empty() && self.deleted.is_empty()
    }
}

/// `mergeScriptVarsIntoEnvList` on an environment file's source. Secret names never get a
/// value; `keep` are names supplied from outside the file (`--env-var`, secrets held in
/// memory) whose rows a deletion leaves alone.
pub fn apply_env_changes(
    src: &str,
    changes: &VarChanges,
    keep: &BTreeSet<String>,
) -> Result<String, SyntaxError> {
    let mut doc = BruDocument::parse(src)?;
    let mut secrets: Vec<ListItem> = match doc.block("vars:secret").map(|b| b.content()) {
        Some(BlockContent::List(items)) => items.clone(),
        _ => Vec::new(),
    };
    let before = secrets.clone();
    // A deleted secret's row goes too, unless its value came from outside the file.
    for name in changes.deleted.iter().filter(|n| !keep.contains(*n)) {
        secrets.retain(|s| !(s.enabled && &s.name == name));
    }
    // Secret rows stay name-only: their values never reach disk (markSecretWithEmptyValue).
    let secret_names: BTreeSet<&str> = before.iter().map(|s| s.name.as_str()).collect();
    merge_block(&mut doc, "vars", changes, keep, |name| {
        secret_names.contains(name)
    });
    if secrets != before {
        if secrets.is_empty() {
            doc.remove_block("vars:secret");
        } else {
            doc.set_block("vars:secret", BlockContent::List(secrets));
        }
    }
    Ok(doc.write())
}

/// `mergeScriptVarsIntoCollectionVarsList` on `collection.bru` (`vars:pre-request`).
pub fn apply_collection_var_changes(
    src: &str,
    changes: &VarChanges,
) -> Result<String, SyntaxError> {
    let mut doc = BruDocument::parse(src)?;
    merge_block(
        &mut doc,
        "vars:pre-request",
        changes,
        &BTreeSet::new(),
        |_| false,
    );
    Ok(doc.write())
}

/// The data-type annotations Bruno writes (`applyInferredDataType`); strings have none.
const TYPE_ANNOTATIONS: [&str; 3] = ["number", "boolean", "object"];

/// The text Bruno writes for a value (`valueToString`, objects indented by 2) and its type.
fn written_value(value: &Value) -> (PairValue, Option<&'static str>) {
    let (text, kind) = match value {
        Value::Null => (String::new(), None),
        Value::String(s) => (s.clone(), None),
        Value::Number(n) => (n.to_string(), Some("number")),
        Value::Bool(b) => (b.to_string(), Some("boolean")),
        other => (
            serde_json::to_string_pretty(other).unwrap_or_default(),
            Some("object"),
        ),
    };
    let value = if text.contains('\n') {
        PairValue::Multiline {
            text,
            content_type: None,
        }
    } else {
        PairValue::Single(text)
    };
    (value, kind)
}

fn set_type(annotations: &mut Vec<Annotation>, kind: Option<&str>) {
    annotations.retain(|a| !TYPE_ANNOTATIONS.contains(&a.name.as_str()));
    if let Some(kind) = kind {
        annotations.push(Annotation {
            name: kind.to_owned(),
            arg: None,
        });
    }
}

/// Changed enabled rows get their new value, deleted enabled rows go, new names are
/// appended; disabled rows are left alone (`mergeScriptVarsIntoEnvList`).
fn merge_block(
    doc: &mut BruDocument,
    block: &str,
    changes: &VarChanges,
    keep: &BTreeSet<String>,
    skip: impl Fn(&str) -> bool,
) {
    let original: Vec<Pair> = match doc.block(block).map(|b| b.content()) {
        Some(BlockContent::Dict(pairs)) => pairs.clone(),
        _ => Vec::new(),
    };
    let mut pairs = original.clone();
    for name in changes.deleted.iter().filter(|n| !keep.contains(*n)) {
        pairs.retain(|p| !(p.enabled && &p.key == name));
    }
    for (name, value) in changes.set.iter().filter(|(n, _)| !skip(n)) {
        let (written, kind) = written_value(value);
        match pairs.iter_mut().find(|p| p.enabled && &p.key == name) {
            Some(pair) => {
                pair.value = written;
                set_type(&mut pair.annotations, kind);
            }
            None => {
                let mut annotations = Vec::new();
                set_type(&mut annotations, kind);
                pairs.push(Pair {
                    key: name.clone(),
                    value: written,
                    enabled: true,
                    annotations,
                });
            }
        }
    }
    if pairs == original {
        return;
    }
    if pairs.is_empty() {
        doc.remove_block(block);
    } else {
        doc.set_block(block, BlockContent::Dict(pairs));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn map(items: &[(&str, Value)]) -> ValueMap {
        items
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect()
    }

    #[test]
    fn changes_between_maps() {
        let before = map(&[("a", json!("1")), ("b", json!("x")), ("c", json!("same"))]);
        let after = map(&[("a", json!("2")), ("c", json!("same")), ("d", json!(5))]);
        assert_eq!(
            VarChanges::between(&before, &after),
            VarChanges {
                set: vec![("a".into(), json!("2")), ("d".into(), json!(5))],
                deleted: vec!["b".into()],
            }
        );
    }

    /// The probe from docs/superpowers/plans/2026-10-23-script-gaps.md: the same script
    /// against Bruno CLI 4.3.0 produced exactly this file.
    #[test]
    fn env_file_matches_bruno() {
        let src = "vars {\n  host: http://127.0.0.1:18931\n  keep: 1\n  gone: x\n  ~off: y\n}\nvars:secret [\n  secret_tok\n]\n";
        let changes = VarChanges {
            set: vec![
                ("added".into(), json!(5)),
                ("keep".into(), json!("2")),
                ("obj".into(), json!({ "a": 1 })),
                ("secret_tok".into(), json!("leak?")),
            ],
            deleted: vec!["gone".into()],
        };
        assert_eq!(
            apply_env_changes(src, &changes, &BTreeSet::new()).unwrap(),
            "vars {\n  host: http://127.0.0.1:18931\n  keep: 2\n  ~off: y\n  @number\n  added: 5\n  @object\n  obj: '''\n    {\n      \"a\": 1\n    }\n  '''\n}\nvars:secret [\n  secret_tok\n]\n"
        );
    }

    #[test]
    fn env_types_strings_and_kept_names() {
        let src = "vars {\n  @description('the port')\n  @number\n  port: 80\n  flag: x\n  token: from-cli\n}\n";
        let changes = VarChanges {
            set: vec![
                ("flag".into(), json!(true)),
                ("multi".into(), json!("a\nb")),
                ("port".into(), json!("eighty")),
            ],
            deleted: vec!["token".into()],
        };
        let keep = BTreeSet::from(["token".to_owned()]);
        assert_eq!(
            apply_env_changes(src, &changes, &keep).unwrap(),
            "vars {\n  @description('the port')\n  port: eighty\n  @boolean\n  flag: true\n  token: from-cli\n  multi: '''\n    a\n    b\n  '''\n}\n"
        );
    }

    #[test]
    fn deleted_secret_row_goes_and_crlf_stays() {
        let src = "vars {\r\n  a: 1\r\n}\r\nvars:secret [\r\n  s,\r\n  ~t\r\n]\r\n";
        let changes = VarChanges {
            set: vec![("t".into(), json!("never written"))],
            deleted: vec!["s".into()],
        };
        assert_eq!(
            apply_env_changes(src, &changes, &BTreeSet::new()).unwrap(),
            "vars {\r\n  a: 1\r\n}\r\nvars:secret [\r\n  ~t\r\n]\r\n"
        );
    }

    #[test]
    fn empty_env_gets_a_vars_block() {
        let changes = VarChanges {
            set: vec![("a".into(), json!("1"))],
            deleted: vec![],
        };
        assert_eq!(
            apply_env_changes("", &changes, &BTreeSet::new()).unwrap(),
            "vars {\n  a: 1\n}\n"
        );
    }

    #[test]
    fn collection_vars() {
        let src = "headers {\n  x: 1\n}\n\nvars:pre-request {\n  cv: one\n  old: o\n  ~off: z\n}\n";
        let changes = VarChanges {
            set: vec![("cv".into(), json!("two")), ("n".into(), json!(3))],
            deleted: vec!["old".into()],
        };
        assert_eq!(
            apply_collection_var_changes(src, &changes).unwrap(),
            "headers {\n  x: 1\n}\n\nvars:pre-request {\n  cv: two\n  ~off: z\n  @number\n  n: 3\n}\n"
        );
    }
}
