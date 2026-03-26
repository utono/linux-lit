use gtk4::prelude::*;
use gtk4::{
    ApplicationWindow, CssProvider, EventControllerKey, ScrolledWindow, TextBuffer, TextView,
    WrapMode,
};

/// Builds and shows the main application window.
/// Returns the TextView so the caller can hold a reference for later updates.
pub fn build_window(app: &gtk4::Application) -> TextView {
    let window = ApplicationWindow::builder()
        .application(app)
        .title("linux-lit")
        .default_width(1000)
        .default_height(800)
        .build();

    let buffer = TextBuffer::new(None);
    let text_view = TextView::builder()
        .buffer(&buffer)
        .editable(false)
        .cursor_visible(false)
        .wrap_mode(WrapMode::Word)
        .build();

    // Apply serif font via CSS
    let css_provider = CssProvider::new();
    css_provider.load_from_string(&format!(
        "textview {{ font-family: Georgia, 'Noto Serif', 'Liberation Serif', 'DejaVu Serif'; font-size: {}pt; }}",
        18
    ));
    gtk4::style_context_add_provider_for_display(
        &gtk4::gdk::Display::default().expect("No display"),
        &css_provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );

    // Line spacing: 1.6x at 18pt ≈ ~14px above + 14px below
    text_view.set_pixels_above_lines(14);
    text_view.set_pixels_below_lines(14);

    // Set initial margins for centered ~700px text column
    text_view.set_left_margin(150);
    text_view.set_right_margin(150);

    // Scrolled window — hide scrollbar
    let scrolled = ScrolledWindow::builder()
        .child(&text_view)
        .hscrollbar_policy(gtk4::PolicyType::Never)
        .vscrollbar_policy(gtk4::PolicyType::External)
        .vexpand(true)
        .hexpand(true)
        .build();

    // Recalculate margins on resize to keep text column centered
    let text_view_for_resize = text_view.clone();
    scrolled.connect_notify_local(Some("width"), move |scrolled, _| {
        let width = scrolled.width();
        let margin = ((width - 700) / 2).max(20);
        text_view_for_resize.set_left_margin(margin);
        text_view_for_resize.set_right_margin(margin);
    });

    // Key event controller — stub that logs presses
    let key_controller = EventControllerKey::new();
    key_controller.connect_key_pressed(|_controller, keyval, _keycode, _state| {
        if let Some(name) = keyval.name() {
            eprintln!("key: {}", name);
        }
        glib::Propagation::Proceed
    });
    window.add_controller(key_controller);

    window.set_child(Some(&scrolled));
    window.present();

    text_view
}
