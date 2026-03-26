mod app;
mod mpv;

use gtk4::prelude::*;
use mpv::{MpvCommand, MpvEvent};

fn main() {
    let application = gtk4::Application::builder()
        .application_id("com.utono.linux-lit")
        .build();

    application.connect_activate(|app| {
        // Channel: GTK → Tokio (commands)
        let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::channel::<MpvCommand>(32);

        // Channel: Tokio → GTK (events)
        let (evt_tx, mut evt_rx) = tokio::sync::mpsc::channel::<MpvEvent>(32);

        // Spawn Tokio runtime on a background thread
        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().expect("Failed to create Tokio runtime");
            rt.block_on(async move {
                // Stub: drain commands, log them
                while let Some(cmd) = cmd_rx.recv().await {
                    eprintln!("Tokio received command: {:?}", cmd);
                }
                // evt_tx available for sending events back to GTK
                let _ = evt_tx;
            });
        });

        // Build the window
        let _text_view = app::build_window(app);

        // Attach event receiver to GTK main loop via spawn_future_local
        glib::spawn_future_local(async move {
            while let Some(event) = evt_rx.recv().await {
                eprintln!("GTK received event: {:?}", event);
            }
        });

        // cmd_tx available for UI to send commands to Tokio
        let _ = cmd_tx;
    });

    application.run();
}
