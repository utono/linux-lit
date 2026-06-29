# GTK4 Gutter / Sign Column Research

**Date:** 2026-03-26

## Approaches

### 1. TextView::set_gutter() (Recommended next step)

GTK4's `TextView` has a native `set_gutter()` method that places a widget into a "border window" alongside the text. Call with `TextWindowType::Left` to place a widget on the left side.

The gutter widget lives inside the TextView's coordinate system and scrolls vertically with the text content automatically — no manual scroll sync needed.

```rust
text_view.set_gutter(gtk4::TextWindowType::Left, Some(&my_gutter_widget));
```

- [Gtk.TextView.set_gutter](https://docs.gtk.org/gtk4/method.TextView.set_gutter.html)
- [TextView in gtk4-rs](https://gtk-rs.org/gtk4-rs/git/docs/gtk4/struct.TextView.html)

### 2. GtkSourceView 5 (sourceview5 crate) — Full Gutter API

GtkSourceView 5 provides a purpose-built gutter system:

- **Gutter** — left or right gutter of a `sourceview5::View` (extends `gtk4::TextView`)
- **GutterRenderer** — base class for gutter cell renderers (full Widget in v5)
- **GutterRendererText** — built-in renderer for text (line numbers)
- **GutterRendererPixbuf** — built-in renderer for icons
- **GutterLines** — batches info about visible lines for performance
- **Gutter::insert(renderer, position)** — inserts renderer at position (line numbers at -30, marks at -20)

Requires replacing `gtk4::TextView` with `sourceview5::View` (subclass of TextView, so most buffer/tag code continues to work).

- Crate: `sourceview5 = "0.11.0"`
- [sourceview5-rs on GitHub](https://github.com/bilelmoussaoui/sourceview5-rs)
- [sourceview5 on crates.io](https://crates.io/crates/sourceview5)
- [GtkSourceView 5 Gutter API](https://gnome.pages.gitlab.gnome.org/gtksourceview/gtksourceview5/class.GutterLines.html)

### 3. Custom DrawingArea (Current linux-lit approach)

A standalone `DrawingArea` in an HBox beside the `TextView`, with a Cairo `set_draw_func` callback. Works but requires manual scroll synchronization via `vadjustment().connect_value_changed()` and coordinate mapping via `buffer_to_window_coords()`.

## Notable Projects

- **Lapce** — has `gutter.rs` but uses its own UI framework (Floem/wgpu), not GTK4
- **RustEditorKit** — GTK4 Rust editor toolkit wrapping GtkSourceView with gutter customization
- **GNOME Text Editor** — uses GtkSourceView 5 with built-in gutter
- **cosmic-edit** — COSMIC's editor uses libcosmic/iced, not GTK4

## Recommendation

- **Immediate:** Use `TextView::set_gutter(TextWindowType::Left, ...)` to eliminate manual scroll sync
- **Long-term:** Adopt `sourceview5::View` for full gutter infrastructure (marks, line numbers, icons)
