use tauri::{
    AppHandle, Wry,
    menu::{Menu, MenuItem, SubmenuBuilder},
};

pub fn build_menu(app: &AppHandle<Wry>) -> tauri::Result<Menu<Wry>> {
    let new_item = MenuItem::with_id(app, "new", "New Workflow", true, Some("CmdOrCtrl+N"))?;
    let open_item = MenuItem::with_id(app, "open", "Open\u{2026}", true, Some("CmdOrCtrl+O"))?;
    let save_item = MenuItem::with_id(app, "save", "Save", true, Some("CmdOrCtrl+S"))?;
    let toggle_sidebar = MenuItem::with_id(
        app,
        "toggle-sidebar",
        "Toggle Sidebar",
        true,
        Some("CmdOrCtrl+B"),
    )?;
    let toggle_logs =
        MenuItem::with_id(app, "toggle-logs", "Toggle Logs", true, Some("CmdOrCtrl+J"))?;
    let run_item = MenuItem::with_id(app, "run-workflow", "Run", true, Some("CmdOrCtrl+R"))?;
    let stop_item = MenuItem::with_id(app, "stop-workflow", "Stop", true, Some("CmdOrCtrl+."))?;
    let toggle_assistant = MenuItem::with_id(
        app,
        "toggle-assistant",
        "Toggle Assistant",
        true,
        Some("CmdOrCtrl+Shift+A"),
    )?;

    let file_menu = SubmenuBuilder::new(app, "File")
        .item(&new_item)
        .item(&open_item)
        .item(&save_item)
        .separator()
        .close_window()
        .build()?;

    let edit_menu = SubmenuBuilder::new(app, "Edit")
        .undo()
        .redo()
        .separator()
        .cut()
        .copy()
        .paste()
        .select_all()
        .build()?;

    let view_menu = SubmenuBuilder::new(app, "View")
        .item(&toggle_sidebar)
        .item(&toggle_logs)
        .build()?;

    let workflow_menu = SubmenuBuilder::new(app, "Workflow")
        .item(&run_item)
        .item(&stop_item)
        .separator()
        .item(&toggle_assistant)
        .build()?;

    let window_menu = SubmenuBuilder::new(app, "Window")
        .minimize()
        .separator()
        .close_window()
        .build()?;

    #[cfg(target_os = "macos")]
    {
        let app_menu = SubmenuBuilder::new(app, "Clickweave")
            .about(None)
            .separator()
            .services()
            .separator()
            .hide()
            .hide_others()
            .show_all()
            .separator()
            .quit()
            .build()?;

        Menu::with_items(
            app,
            &[
                &app_menu,
                &file_menu,
                &edit_menu,
                &view_menu,
                &workflow_menu,
                &window_menu,
            ],
        )
    }

    #[cfg(not(target_os = "macos"))]
    {
        let help_menu = SubmenuBuilder::new(app, "Help").about(None).build()?;

        Menu::with_items(
            app,
            &[
                &file_menu,
                &edit_menu,
                &view_menu,
                &workflow_menu,
                &window_menu,
                &help_menu,
            ],
        )
    }
}
