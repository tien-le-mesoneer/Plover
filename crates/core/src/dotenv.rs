//! `.env` files, parsed like dotenv 16's `parse` (what Bruno uses: bruno-lang
//! `dotenvToJson.js`), and the process environment scripts and `{{process.env.X}}` see.

use std::path::Path;
use std::sync::OnceLock;

use regex::Regex;

use crate::vars::ValueMap;

/// dotenv 16 `parse`: no variable expansion; later keys win.
pub fn parse(src: &str) -> Vec<(String, String)> {
    // dotenv 16 `LINE`, with `\w` spelled as ASCII like JavaScript's.
    static LINE: OnceLock<Regex> = OnceLock::new();
    let line = LINE.get_or_init(|| {
        Regex::new(
            r#"(?m)^\s*(?:export\s+)?([A-Za-z0-9_.-]+)(?:\s*=\s*?|:\s+?)(\s*'(?:\\'|[^'])*'|\s*"(?:\\"|[^"])*"|\s*`(?:\\`|[^`])*`|[^#\r\n]+)?\s*(?:#.*)?$"#,
        )
        .expect("valid dotenv regex")
    });
    let src = src.replace("\r\n", "\n").replace('\r', "\n");
    let mut out: Vec<(String, String)> = Vec::new();
    for caps in line.captures_iter(&src) {
        let key = caps[1].to_owned();
        let mut value = caps.get(2).map_or("", |m| m.as_str()).trim().to_owned();
        let quote = value.chars().next();
        if let Some(q @ ('\'' | '"' | '`')) = quote
            && value.len() >= 2
            && value.ends_with(q)
        {
            value = value[1..value.len() - 1].to_owned();
        }
        if quote == Some('"') {
            value = value.replace("\\n", "\n").replace("\\r", "\r");
        }
        out.retain(|(k, _)| *k != key);
        out.push((key, value));
    }
    out
}

/// Bruno CLI `commands/run.js:661-674`: the process environment, overridden by the
/// collection's `.env` file when there is one.
pub fn process_env(collection_root: &Path) -> ValueMap {
    let case_insensitive = cfg!(windows);
    let mut env = ValueMap::new();
    for (k, v) in std::env::vars_os() {
        set_var(
            &mut env,
            k.to_string_lossy().into_owned(),
            v.to_string_lossy().into_owned(),
            case_insensitive,
        );
    }
    let path = collection_root.join(".env");
    if path.is_file() {
        match std::fs::read_to_string(&path) {
            Ok(text) => {
                for (k, v) in parse(&text) {
                    set_var(&mut env, k, v, case_insensitive);
                }
            }
            // Never log values; the path is enough.
            Err(e) => tracing::warn!("cannot read {}: {e}", path.display()),
        }
    }
    env
}

/// Windows variable names are case-insensitive, and Node's `process.env` (which Bruno reads)
/// matches any case: `process.env.PATH` finds `Path`. Lookups here are plain map/path walks,
/// so on Windows a variable is stored under its own name and its upper-case name, and a later
/// value replaces every case variant. Mixed-case lookups such as `process.env.path` are not
/// covered.
fn set_var(env: &mut ValueMap, key: String, value: String, case_insensitive: bool) {
    let value = serde_json::Value::String(value);
    if case_insensitive {
        env.retain(|k, _| !k.eq_ignore_ascii_case(&key));
        env.insert(key.to_ascii_uppercase(), value.clone());
    }
    env.insert(key, value);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_names_are_case_insensitive() {
        let mut env = ValueMap::new();
        set_var(&mut env, "Path".into(), r"C:\Windows".into(), true);
        assert_eq!(
            env.get("PATH").and_then(|v| v.as_str()),
            Some(r"C:\Windows")
        );
        assert_eq!(
            env.get("Path").and_then(|v| v.as_str()),
            Some(r"C:\Windows")
        );
        // `.env` `PATH=` replaces the process `Path` instead of adding a second variable.
        set_var(&mut env, "PATH".into(), "from-dotenv".into(), true);
        assert_eq!(env.len(), 1);
        assert_eq!(
            env.get("PATH").and_then(|v| v.as_str()),
            Some("from-dotenv")
        );

        let mut unix = ValueMap::new();
        set_var(&mut unix, "Path".into(), "a".into(), false);
        set_var(&mut unix, "PATH".into(), "b".into(), false);
        assert_eq!(unix.len(), 2);
    }

    fn kv(src: &str) -> Vec<(String, String)> {
        parse(src)
    }

    fn pairs(items: &[(&str, &str)]) -> Vec<(String, String)> {
        items
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn basic_quotes_comments_export() {
        let src = "A=1\nB = two words \n# comment\nexport C=3\nD='single # not comment'\nE=\"double\"\nF=val # trailing\nG=\nH=`back`\n";
        assert_eq!(
            kv(src),
            pairs(&[
                ("A", "1"),
                ("B", "two words"),
                ("C", "3"),
                ("D", "single # not comment"),
                ("E", "double"),
                ("F", "val"),
                ("G", ""),
                ("H", "back"),
            ])
        );
    }

    #[test]
    fn double_quotes_expand_newlines_and_allow_multiline() {
        let src = "A=\"line1\\nline2\"\nB='raw\\n'\nC=\"multi\nline\"\nD=after\n";
        assert_eq!(
            kv(src),
            pairs(&[
                ("A", "line1\nline2"),
                ("B", "raw\\n"),
                ("C", "multi\nline"),
                ("D", "after"),
            ])
        );
    }

    #[test]
    fn crlf_colon_syntax_and_no_expansion() {
        let src = "A=1\r\nB: two\r\nC=${A}\r\n";
        assert_eq!(kv(src), pairs(&[("A", "1"), ("B", "two"), ("C", "${A}")]));
    }

    #[test]
    fn dotenv_overrides_process_env() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(".env"),
            "PATH=from-dotenv\nPLOVER_ONLY_IN_DOTENV=x\n",
        )
        .unwrap();
        let env = process_env(dir.path());
        assert_eq!(
            env.get("PATH").and_then(|v| v.as_str()),
            Some("from-dotenv")
        );
        assert_eq!(
            env.get("PLOVER_ONLY_IN_DOTENV").and_then(|v| v.as_str()),
            Some("x")
        );
        let none = tempfile::tempdir().unwrap();
        assert!(process_env(none.path()).contains_key("PATH"));
    }

    /// Expected output produced by node dotenv 16.6.1 (the version Bruno CLI 4.3.0 installs).
    #[test]
    fn matches_node_dotenv_16() {
        let src = "A=1\n B = spaced value  \nC=\"quoted # hash\"\nD='it''s'\nE=\"esc \\\"q\\\" \\n done\"\nF=a=b=c\nG = # only comment\nH=\"unterminated\nI=next\nexport J=exp\nK.L-M=dots\nN:  colon\nO=`tick ' and \"`\nP='multi\nline single'\nQ=x#nospace\nR= \"  padded  \"\n";
        let expected: Vec<(String, String)> = serde_json::from_str(r###"[["A","1"],["B","spaced value"],["C","quoted # hash"],["D","it''s"],["E","esc \\\"q\\\" \n done"],["F","a=b=c"],["G",""],["H","\"unterminated"],["I","next"],["J","exp"],["K.L-M","dots"],["N","colon"],["O","tick ' and \""],["P","multi\nline single"],["Q","x"],["R","  padded  "]]"###).unwrap();
        assert_eq!(kv(src), expected);
    }
}
