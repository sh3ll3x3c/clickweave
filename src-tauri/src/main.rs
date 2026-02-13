// Prevents additional console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;

use commands::*;
use std::sync::Mutex;
use tauri::Manager;
use tauri_specta::{Builder, collect_commands};
use tracing_subscriber::{EnvFilter, Layer, fmt, layer::SubscriberExt, util::SubscriberInitExt};

fn log_dir() -> std::path::PathBuf {
    #[cfg(target_os = "macos")]
    {
        std::path::PathBuf::from(std::env::var("HOME").expect("HOME should be set"))
            .join("Library/Logs/Clickweave")
    }
    #[cfg(not(target_os = "macos"))]
    {
        std::env::current_dir()
            .unwrap_or_else(|_| std::path::PathBuf::from("."))
            .join("logs")
    }
}

fn main() {
    let log_dir = log_dir();
    std::fs::create_dir_all(&log_dir).ok();

    let file_appender = tracing_appender::rolling::RollingFileAppender::builder()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .filename_prefix("clickweave")
        .filename_suffix("txt")
        .build(&log_dir)
        .expect("failed to create log file appender");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    let console_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let file_filter = EnvFilter::new("trace");

    tracing_subscriber::registry()
        .with(fmt::layer().with_filter(console_filter))
        .with(
            fmt::layer()
                .json()
                .with_writer(non_blocking)
                .with_filter(file_filter),
        )
        .init();

    let builder = Builder::<tauri::Wry>::new().commands(collect_commands![
        ping,
        pick_workflow_file,
        pick_save_file,
        open_project,
        save_project,
        validate,
        node_type_defaults,
        plan_workflow,
        patch_workflow,
        run_workflow,
        stop_workflow,
        list_runs,
        load_run_events,
        read_artifact_base64,
        import_asset,
    ]);

    #[cfg(debug_assertions)]
    {
        let bindings_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("CARGO_MANIFEST_DIR should have a parent")
            .join("ui/src/bindings.ts");
        builder
            .export(
                specta_typescript::Typescript::default()
                    .bigint(specta_typescript::BigIntExportBehavior::Number),
                bindings_path,
            )
            .expect("Failed to export typescript bindings");
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .manage(Mutex::new(ExecutorHandle::default()))
        .invoke_handler(builder.invoke_handler())
        .setup(move |app| {
            let app_data_dir = app
                .path()
                .app_data_dir()
                .expect("Failed to resolve app data dir");
            std::fs::create_dir_all(&app_data_dir).ok();
            app.manage(AppDataDir(app_data_dir));
            builder.mount_events(app);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
