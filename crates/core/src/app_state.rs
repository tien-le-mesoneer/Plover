//! What a long-lived Plover process remembers between starts (`state.json`): the
//! collections that were open, recently opened ones, and per-collection UI layout.
//! Only paths and layout are stored — never secrets or variable values.
//!
//! The server owns this file, not the browser: the desktop webview gets a new random
//! port (so a new origin and empty `localStorage`) on every launch.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

pub const STATE_VERSION: u32 = 1;
pub const STATE_FILE: &str = "state.json";
pub const MAX_RECENT: usize = 10;
pub const MAX_TABS: usize = 50;
pub const MAX_EXPANDED: usize = 1000;
pub const MAX_UI_STRING: usize = 1024;

/// Layout of one collection in the UI. Unsaved drafts are deliberately not kept.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct UiState {
    /// Item IDs of the open request tabs, in tab order.
    pub tabs: Vec<String>,
    pub active: Option<String>,
    /// Item IDs of expanded folders.
    pub expanded: Vec<String>,
    /// Selected environment name.
    pub env: Option<String>,
    /// The whole collection is folded in the sidebar.
    pub collapsed: bool,
}

impl UiState {
    pub fn validate(&self) -> Result<(), String> {
        if self.tabs.len() > MAX_TABS {
            return Err(format!("at most {MAX_TABS} tabs"));
        }
        if self.expanded.len() > MAX_EXPANDED {
            return Err(format!("at most {MAX_EXPANDED} expanded folders"));
        }
        let strings = self
            .tabs
            .iter()
            .chain(&self.expanded)
            .chain(&self.active)
            .chain(&self.env);
        for s in strings {
            if s.len() > MAX_UI_STRING {
                return Err(format!("names are limited to {MAX_UI_STRING} bytes"));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenEntry {
    /// The collection's canonical root.
    pub root: String,
    /// Unix milliseconds.
    pub opened_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentEntry {
    pub root: String,
    pub name: String,
    pub last_opened: u64,
}

/// Smallest and largest sidebar width that is stored, in CSS pixels.
pub const SIDEBAR_WIDTH_RANGE: std::ops::RangeInclusive<u32> = 120..=4000;

/// Window layout that is the same for every collection.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LayoutState {
    /// The dragged sidebar width; `None` is the default width.
    pub sidebar_width: Option<u32>,
}

impl LayoutState {
    pub fn validate(&self) -> Result<(), String> {
        match self.sidebar_width {
            Some(w) if !SIDEBAR_WIDTH_RANGE.contains(&w) => Err(format!(
                "sidebarWidth must be {}..={} px",
                SIDEBAR_WIDTH_RANGE.start(),
                SIDEBAR_WIDTH_RANGE.end()
            )),
            _ => Ok(()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedState {
    pub version: u32,
    pub open: Vec<OpenEntry>,
    /// Newest first, at most [`MAX_RECENT`].
    pub recent: Vec<RecentEntry>,
    /// Keyed by canonical root.
    pub ui: BTreeMap<String, UiState>,
    /// Missing in files written before it existed.
    #[serde(default)]
    pub layout: LayoutState,
}

impl Default for PersistedState {
    fn default() -> Self {
        PersistedState {
            version: STATE_VERSION,
            open: Vec::new(),
            recent: Vec::new(),
            ui: BTreeMap::new(),
            layout: LayoutState::default(),
        }
    }
}

/// The per-user directory for `state.json`: `PLOVER_STATE_DIR` if set, else macOS
/// `~/Library/Application Support/Plover`, Windows `%APPDATA%\Plover`, Linux
/// `$XDG_CONFIG_HOME/plover` or `~/.config/plover`. `None` when no home is known.
pub fn default_dir() -> Option<PathBuf> {
    let var = |name: &str| {
        std::env::var_os(name)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    if let Some(dir) = var("PLOVER_STATE_DIR") {
        return Some(dir);
    }
    if cfg!(target_os = "macos") {
        return var("HOME").map(|h| h.join("Library/Application Support/Plover"));
    }
    if cfg!(windows) {
        return var("APPDATA").map(|d| d.join("Plover"));
    }
    var("XDG_CONFIG_HOME")
        .or_else(|| var("HOME").map(|h| h.join(".config")))
        .map(|d| d.join("plover"))
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// `state.json` plus its in-memory copy. Every change is written through atomically.
pub struct AppStateStore {
    file: Option<PathBuf>,
    state: Mutex<PersistedState>,
}

impl AppStateStore {
    /// Keeps state for this process only (nothing is read or written).
    pub fn in_memory() -> AppStateStore {
        AppStateStore {
            file: None,
            state: Mutex::default(),
        }
    }

    /// Reads `<dir>/state.json`. A missing file is an empty state; an unreadable,
    /// unparsable or unknown-version file is renamed to `state.json.bak-<unix-ms>`
    /// and replaced by an empty state.
    pub fn load(dir: &Path) -> AppStateStore {
        let file = dir.join(STATE_FILE);
        let state = match std::fs::read(&file) {
            Ok(bytes) => match serde_json::from_slice::<PersistedState>(&bytes) {
                Ok(s) if s.version == STATE_VERSION => s,
                Ok(s) => {
                    back_up(&file, &format!("unknown version {}", s.version));
                    PersistedState::default()
                }
                Err(e) => {
                    back_up(&file, &e.to_string());
                    PersistedState::default()
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => PersistedState::default(),
            Err(e) => {
                back_up(&file, &e.to_string());
                PersistedState::default()
            }
        };
        AppStateStore {
            file: Some(file),
            state: Mutex::new(state),
        }
    }

    pub fn snapshot(&self) -> PersistedState {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn change(&self, f: impl FnOnce(&mut PersistedState)) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        f(&mut state);
        let Some(file) = &self.file else { return };
        let write = || -> std::io::Result<()> {
            let dir = file.parent().unwrap_or(Path::new("."));
            std::fs::create_dir_all(dir)?;
            let bytes = serde_json::to_vec_pretty(&*state).map_err(std::io::Error::other)?;
            crate::store::write_atomic(file, &bytes).map_err(std::io::Error::other)?;
            Ok(())
        };
        if let Err(e) = write() {
            tracing::warn!(file = %file.display(), error = %e, "could not save Plover's state");
        }
    }

    /// Remembers `root` as open (appended if new, so saved order is open order) and
    /// moves it to the front of the recent list.
    pub fn record_open(&self, root: &str, name: &str) {
        let now = now_ms();
        self.change(|s| {
            if !s.open.iter().any(|o| o.root == root) {
                s.open.push(OpenEntry {
                    root: root.to_owned(),
                    opened_at: now,
                });
            }
            s.recent.retain(|r| r.root != root);
            s.recent.insert(
                0,
                RecentEntry {
                    root: root.to_owned(),
                    name: name.to_owned(),
                    last_opened: now,
                },
            );
            s.recent.truncate(MAX_RECENT);
        });
    }

    /// Forgets `root` for the next start; it stays in the recent list.
    pub fn record_close(&self, root: &str) {
        self.change(|s| s.open.retain(|o| o.root != root));
    }

    pub fn remove_recent(&self, root: &str) {
        self.change(|s| s.recent.retain(|r| r.root != root));
    }

    pub fn ui(&self, root: &str) -> Option<UiState> {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .ui
            .get(root)
            .cloned()
    }

    pub fn layout(&self) -> LayoutState {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .layout
            .clone()
    }

    pub fn set_layout(&self, layout: LayoutState) -> Result<(), String> {
        layout.validate()?;
        self.change(|s| s.layout = layout);
        Ok(())
    }

    pub fn set_ui(&self, root: &str, ui: UiState) -> Result<(), String> {
        ui.validate()?;
        self.change(|s| {
            s.ui.insert(root.to_owned(), ui);
        });
        Ok(())
    }
}

fn back_up(file: &Path, reason: &str) {
    let backup = file.with_file_name(format!("{STATE_FILE}.bak-{}", now_ms()));
    match std::fs::rename(file, &backup) {
        Ok(()) => tracing::warn!(
            backup = %backup.display(),
            %reason,
            "Plover's state file could not be used; it was set aside and Plover starts fresh"
        ),
        Err(e) => {
            tracing::warn!(file = %file.display(), error = %e, %reason, "could not set aside Plover's state file")
        }
    }
}
