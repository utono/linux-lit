mod app;
mod config;
mod db;
mod input;
mod logging;
mod mpv;
mod ui;

use gtk4::prelude::*;
use mpv::{MpvCommand, MpvEvent};

fn main() {
    // Clear and set up log file
    let log_path = std::env::var("HOME").unwrap_or_default() + "/utono/linux-lit/linux-lit.log";
    let _ = std::fs::write(&log_path, "");
    logging::init(&log_path);

    let application = gtk4::Application::builder()
        .application_id("com.utono.linux-lit")
        .build();

    application.connect_activate(|gtk_app| {
        // Channel: GTK → Tokio (commands)
        let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::channel::<MpvCommand>(32);

        // Channel: Tokio → GTK (events)
        let (evt_tx, mut evt_rx) = tokio::sync::mpsc::channel::<MpvEvent>(32);

        // Create Tokio runtime, clone handle for GTK thread, move runtime to background thread
        let rt = tokio::runtime::Runtime::new().expect("Failed to create Tokio runtime");
        let tokio_handle = rt.handle().clone();
        std::thread::spawn(move || {
            rt.block_on(async move {
                while let Some(cmd) = cmd_rx.recv().await {
                    eprintln!("Tokio received command: {:?}", cmd);
                }
                let _ = evt_tx;
            });
        });

        // Load works list from database (blocking is OK during startup — 133 works, sub-ms)
        let works = {
            let conn = db::queries::open_db().expect("Failed to open lit.db");
            db::queries::list_works(&conn).expect("Failed to list works")
        };

        // Load config
        let config = config::load();

        // Build the window with works list, Tokio handle, and config
        let _state = app::build_window(gtk_app, works, tokio_handle, config);

        // Attach event receiver to GTK main loop
        glib::spawn_future_local(async move {
            while let Some(event) = evt_rx.recv().await {
                eprintln!("GTK received event: {:?}", event);
            }
        });

        // Keep cmd_tx alive so the Tokio runtime doesn't shut down
        // (dropping cmd_tx closes the channel, ending the recv loop and dropping the runtime)
        std::mem::forget(cmd_tx);
    });

    application.run();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spawn_blocking_via_handle() {
        // Reproduce the exact runtime pattern used in connect_activate:
        // Runtime on background thread, handle used from another thread for spawn_blocking
        let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::channel::<MpvCommand>(32);

        let rt = tokio::runtime::Runtime::new().unwrap();
        let handle = rt.handle().clone();
        std::thread::spawn(move || {
            rt.block_on(async move { while let Some(_cmd) = cmd_rx.recv().await {} });
        });

        // spawn_blocking from outside the runtime, using the handle
        let result = handle.block_on(async {
            handle
                .spawn_blocking(|| {
                    let conn = db::queries::open_db().unwrap();
                    db::queries::load_work(&conn, "Ham").unwrap()
                })
                .await
                .unwrap()
        });

        assert_eq!(result.title, "Hamlet");
        assert!(result.lines.len() > 4000);

        // Clean up: drop cmd_tx so runtime shuts down
        drop(cmd_tx);
    }
}
