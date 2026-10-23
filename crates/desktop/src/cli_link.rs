//! One binary (decision P4-5): the desktop executable runs as the `plv` CLI when it is
//! invoked as `plv`, with `run`/`serve`, or with `PLOVER_AS_CLI=1`; "Install Command
//! Line Tool…" links `plv` to it.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

/// Whether this invocation is the CLI rather than the GUI.
pub fn is_cli(argv: &[OsString], as_cli_env: bool) -> bool {
    if as_cli_env {
        return true;
    }
    let invoked_as_plv = argv.first().is_some_and(|program| {
        let program = program.to_string_lossy();
        // Both separators: a Windows path may be inspected on any platform.
        let name = program.rsplit(['/', '\\']).next().unwrap_or_default();
        let name = name.to_ascii_lowercase();
        name == "plv" || name == "plv.exe"
    });
    invoked_as_plv
        || argv
            .get(1)
            .is_some_and(|first| first == "run" || first == "serve")
}

#[derive(Debug, PartialEq, Eq)]
pub struct Installed {
    pub path: PathBuf,
    /// Whether the link's directory is on `PATH`.
    pub on_path: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum LinkError {
    /// A file that isn't a link to this app is already there; it is left alone.
    ForeignFile(PathBuf),
    Io(String),
}

/// Where the link goes, in order: `/usr/local/bin` if it exists and is writable,
/// else `~/.local/bin` (created if needed).
pub fn default_candidates(home: &Path) -> Vec<PathBuf> {
    vec![PathBuf::from("/usr/local/bin"), home.join(".local/bin")]
}

/// A link we may replace: one that points at a Plover executable (this one, or one
/// from before the app moved).
fn is_own_link(link: &Path) -> bool {
    std::fs::read_link(link).is_ok_and(|target| {
        target
            .file_name()
            .is_some_and(|name| name == "plover" || name == "plover.exe")
    })
}

/// Creates `<dir>/plv` → `exe` in the first usable candidate directory.
pub fn install_link(
    exe: &Path,
    candidates: &[PathBuf],
    path_env: Option<&OsStr>,
) -> Result<Installed, LinkError> {
    let io = |e: std::io::Error, path: &Path| LinkError::Io(format!("{}: {e}", path.display()));
    for (i, dir) in candidates.iter().enumerate() {
        let last = i + 1 == candidates.len();
        if !dir.is_dir() {
            if !last {
                continue;
            }
            std::fs::create_dir_all(dir).map_err(|e| io(e, dir))?;
        }
        let link = dir.join("plv");
        match std::fs::symlink_metadata(&link) {
            Ok(meta) if meta.file_type().is_symlink() && is_own_link(&link) => {
                match std::fs::remove_file(&link) {
                    Ok(()) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied && !last => continue,
                    Err(e) => return Err(io(e, &link)),
                }
            }
            Ok(_) => return Err(LinkError::ForeignFile(link)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(io(e, &link)),
        }
        match symlink(exe, &link) {
            Ok(()) => {
                let on_path =
                    path_env.is_some_and(|p| std::env::split_paths(p).any(|entry| entry == *dir));
                return Ok(Installed {
                    path: link,
                    on_path,
                });
            }
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied && !last => continue,
            Err(e) => return Err(io(e, &link)),
        }
    }
    Err(LinkError::Io("no directory to install plv into".into()))
}

#[cfg(unix)]
fn symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(not(unix))]
fn symlink(_target: &Path, _link: &Path) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "installing the command line tool is not supported on this platform",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(a: &[&str]) -> Vec<OsString> {
        a.iter().map(OsString::from).collect()
    }

    #[test]
    fn dispatch_rules() {
        assert!(is_cli(&args(&["/usr/local/bin/plv", "--version"]), false));
        assert!(is_cli(&args(&["C:\\Plover\\plv.exe"]), false));
        assert!(is_cli(&args(&["C:\\Plover\\PLV.EXE", "run", "x"]), false));
        assert!(is_cli(
            &args(&["/Applications/Plover.app/Contents/MacOS/plover", "run", "c"]),
            false
        ));
        assert!(is_cli(&args(&["plover", "serve"]), false));
        assert!(
            is_cli(&args(&["/usr/bin/plover", "--help"]), true),
            "PLOVER_AS_CLI"
        );
        assert!(!is_cli(
            &args(&["/Applications/Plover.app/Contents/MacOS/plover"]),
            false
        ));
        assert!(!is_cli(&args(&["plover", "/path/to/collection"]), false));
        assert!(!is_cli(&args(&["plover", "-psn_0_1234"]), false));
    }

    #[cfg(unix)]
    mod unix {
        use super::super::*;
        use std::os::unix::fs::PermissionsExt;

        fn exe(dir: &Path) -> PathBuf {
            let exe = dir.join("Plover.app/Contents/MacOS/plover");
            std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
            std::fs::write(&exe, "#!/bin/sh\n").unwrap();
            exe
        }

        #[test]
        fn install_link_rules() {
            let tmp = tempfile::tempdir().unwrap();
            let exe = exe(tmp.path());
            let system = tmp.path().join("usr-local-bin");
            let user = tmp.path().join("home/.local/bin");
            std::fs::create_dir_all(&system).unwrap();

            // Writable first candidate.
            let path_env = OsString::from(format!("/bin:{}", system.display()));
            let got = install_link(&exe, &[system.clone(), user.clone()], Some(&path_env)).unwrap();
            assert_eq!(
                got,
                Installed {
                    path: system.join("plv"),
                    on_path: true
                }
            );
            assert_eq!(std::fs::read_link(system.join("plv")).unwrap(), exe);

            // Our own link is replaced (e.g. the app moved).
            let got = install_link(&exe, &[system.clone(), user.clone()], Some(&path_env)).unwrap();
            assert_eq!(got.path, system.join("plv"));

            // A foreign file is refused and left alone.
            std::fs::remove_file(system.join("plv")).unwrap();
            std::fs::write(system.join("plv"), "someone else's plv").unwrap();
            assert_eq!(
                install_link(&exe, &[system.clone(), user.clone()], None),
                Err(LinkError::ForeignFile(system.join("plv")))
            );
            assert_eq!(
                std::fs::read_to_string(system.join("plv")).unwrap(),
                "someone else's plv"
            );

            // Read-only first candidate: fall back to ~/.local/bin (created), not on PATH.
            std::fs::remove_file(system.join("plv")).unwrap();
            std::fs::set_permissions(&system, std::fs::Permissions::from_mode(0o555)).unwrap();
            let got = install_link(
                &exe,
                &[system.clone(), user.clone()],
                Some(OsStr::new("/bin")),
            )
            .unwrap();
            std::fs::set_permissions(&system, std::fs::Permissions::from_mode(0o755)).unwrap();
            assert_eq!(
                got,
                Installed {
                    path: user.join("plv"),
                    on_path: false
                }
            );
            assert_eq!(std::fs::read_link(user.join("plv")).unwrap(), exe);

            // A missing /usr/local/bin is skipped, never created.
            let missing = tmp.path().join("missing");
            let got = install_link(&exe, &[missing.clone(), user.clone()], None).unwrap();
            assert_eq!(got.path, user.join("plv"));
            assert!(!missing.exists());
        }

        #[test]
        fn default_candidates_order() {
            assert_eq!(
                default_candidates(Path::new("/home/u")),
                [
                    PathBuf::from("/usr/local/bin"),
                    PathBuf::from("/home/u/.local/bin")
                ]
            );
        }
    }
}
