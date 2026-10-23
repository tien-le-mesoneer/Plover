//! The app menu. On macOS the first submenu is the application menu; an Edit menu is
//! needed there for copy/paste shortcuts to reach the webview.

use tauri::menu::{Menu, MenuBuilder, MenuItemBuilder, PredefinedMenuItem, SubmenuBuilder};
use tauri::{AppHandle, Wry};

pub const OPEN_COLLECTION: &str = "open-collection";
pub const OPEN_IN_BROWSER: &str = "open-in-browser";
pub const RELOAD: &str = "reload";
pub const QUIT: &str = "quit";

pub fn build(app: &AppHandle<Wry>) -> tauri::Result<Menu<Wry>> {
    let open = MenuItemBuilder::with_id(OPEN_COLLECTION, "Open Collection…")
        .accelerator("CmdOrCtrl+O")
        .build(app)?;
    let browser = MenuItemBuilder::with_id(OPEN_IN_BROWSER, "Open in Browser").build(app)?;
    let reload = MenuItemBuilder::with_id(RELOAD, "Reload Window")
        .accelerator("CmdOrCtrl+R")
        .build(app)?;
    let quit = MenuItemBuilder::with_id(QUIT, "Quit Plover")
        .accelerator("CmdOrCtrl+Q")
        .build(app)?;

    let mut file = SubmenuBuilder::new(app, "File")
        .item(&open)
        .item(&browser)
        .separator()
        .item(&reload);
    if !cfg!(target_os = "macos") {
        file = file.separator().item(&quit);
    }
    let edit = SubmenuBuilder::new(app, "Edit")
        .undo()
        .redo()
        .separator()
        .cut()
        .copy()
        .paste()
        .select_all()
        .build()?;

    let mut menu = MenuBuilder::new(app);
    if cfg!(target_os = "macos") {
        let app_menu = SubmenuBuilder::new(app, "Plover")
            .item(&PredefinedMenuItem::about(app, Some("About Plover"), None)?)
            .separator()
            .item(&PredefinedMenuItem::hide(app, None)?)
            .item(&PredefinedMenuItem::hide_others(app, None)?)
            .separator()
            .item(&quit)
            .build()?;
        menu = menu.item(&app_menu);
    }
    menu.item(&file.build()?).item(&edit).build()
}
