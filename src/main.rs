mod app;
mod db;
mod mpv;
mod ui;

use gtk4::prelude::*;
use mpv::{MpvCommand, MpvEvent};

fn main() {
    let application = gtk4::Application::builder()
        .application_id("com.utono.linux-lit")
        .build();

    application.connect_activate(|gtk_app| {
        // Channel: GTK → Tokio (commands)
        let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::channel::<MpvCommand>(32);

        // Channel: Tokio → GTK (events)
        let (evt_tx, mut evt_rx) = tokio::sync::mpsc::channel::<MpvEvent>(32);

        // Create Tokio runtime, clone handle for GTK thread, then move runtime to background
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

        // Build the window with works list and Tokio handle for async DB operations
        let _state = app::build_window(gtk_app, works, tokio_handle);

        // Attach event receiver to GTK main loop
        glib::spawn_future_local(async move {
            while let Some(event) = evt_rx.recv().await {
                eprintln!("GTK received event: {:?}", event);
            }
        });

        let _ = cmd_tx;
    });

    application.run();
}
