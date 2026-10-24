//! The server's `NativeHost` in the desktop app: native folder picker, save dialog and
//! opening URLs in the default browser, through Tauri's dialog and opener plugins.

use std::path::PathBuf;

use plover_server::native::{HostFuture, NativeHost};
use tauri::{AppHandle, Wry};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

pub struct DesktopHost {
    pub app: AppHandle<Wry>,
}

/// Shows the native folder picker and waits for the user's choice (`None` = cancelled).
pub async fn pick_folder(app: &AppHandle<Wry>, title: &str) -> Option<PathBuf> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title(title)
        .pick_folder(move |picked| {
            let _ = tx.send(picked.and_then(|p| p.into_path().ok()));
        });
    rx.await.ok().flatten()
}

impl NativeHost for DesktopHost {
    fn pick_folder<'a>(&'a self, title: &'a str) -> HostFuture<'a, Option<PathBuf>> {
        Box::pin(pick_folder(&self.app, title))
    }

    fn save_file<'a>(
        &'a self,
        default_name: &'a str,
        bytes: Vec<u8>,
    ) -> HostFuture<'a, Result<Option<PathBuf>, String>> {
        Box::pin(async move {
            let (tx, rx) = tokio::sync::oneshot::channel();
            self.app
                .dialog()
                .file()
                .set_title("Save Report")
                .set_file_name(default_name)
                .save_file(move |picked| {
                    let _ = tx.send(picked.and_then(|p| p.into_path().ok()));
                });
            let Some(path) = rx.await.ok().flatten() else {
                return Ok(None);
            };
            tokio::task::spawn_blocking({
                let path = path.clone();
                move || std::fs::write(&path, bytes)
            })
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
            Ok(Some(path))
        })
    }

    fn open_external<'a>(&'a self, url: &'a str) -> HostFuture<'a, Result<(), String>> {
        Box::pin(async move {
            self.app
                .opener()
                .open_url(url, None::<&str>)
                .map_err(|e| e.to_string())
        })
    }
}
