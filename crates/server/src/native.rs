//! What a native shell (the desktop app) can do for the UI. The UI reaches these
//! through the API like everything else; web mode has no host and answers 501.

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;

pub type HostFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub trait NativeHost: Send + Sync {
    /// Shows a native folder picker; `None` when the user cancels.
    fn pick_folder<'a>(&'a self, title: &'a str) -> HostFuture<'a, Option<PathBuf>>;
    /// Opens `url` in the default browser.
    fn open_external<'a>(&'a self, url: &'a str) -> HostFuture<'a, Result<(), String>>;
}
