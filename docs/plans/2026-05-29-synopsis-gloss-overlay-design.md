# Scene Synopsis in Gloss Overlay

Ctrl+H shows the current scene's synopsis in the gloss overlay modal (same rendering as glosses — centered card with scrim, app font/size). Pressing Escape or Ctrl+H again dismisses it. A toast appears if no synopsis exists for the current position. Plain `h` sidebar synopsis remains unchanged.

## New Action

In `src/input/actions/mod.rs`, add variant:

```rust
ShowSynopsisOverlay
```

Category: `Display`. Name: `"ShowSynopsisOverlay"`.

## Keybind

In `src/input/keymap_config.rs`, add default binding:

```rust
KeyCombo::ctrl("h") => Action::ShowSynopsisOverlay
```

Also add to `keymap.json` (stowed from `~/tty-dotfiles/linux-lit/`).

## Handler: show_synopsis_overlay

New function in `src/app.rs`. Called from keymap dispatch.

Logic:

1. If the gloss overlay is currently visible (showing a synopsis), dismiss it and return
2. Determine current line's `(div1, div2)` from `state.work.lines[state.current_line]`
3. Look up `state.synopsis_cache.get(&(div1, div2))`
4. If not found: show toast "No synopsis for this section", return
5. If found: format the title and content, call `state.gloss_overlay.show_gloss_with_color()`

## Title Formatting

The gloss overlay title shows the scene header:

- If `div1 > 0` and the work has multiple acts: "Act {div1}, Scene {div2}"
- If `div1 == 0` or single-act work: "Scene {div2}"

Detection of single-act: all entries in `synopsis_cache` have `div1 == 0` or `div1 == 1` and there's only one distinct div1 value.

## Synopsis Content in Gloss Buffer

Add a new method on `GlossOverlay` (e.g. `show_synopsis`) rather than reusing `show_gloss_with_color` directly. Reason: `show_gloss_with_color` hides the title label (`self.title.set_visible(false)`) and expects gloss-specific parameters (`source_line_numbers`, `root_color`). The new method:

1. Sets `self.title` text to the scene header (e.g. "Act 2, Scene 3") and makes it visible with `.gloss-title` styling
2. Uses `populate_gloss_buffer` to render the synopsis prose into `gloss_view` with `.gloss-text` styling
3. Hides gloss-specific elements (orig_header, original_label, corr_header, corrected_label, position_label)
4. Shows the container, hint ("Esc to close"), and scrolled overlay
5. Hides the scrim (same as `show_gloss_with_color`)

Signature: `pub fn show_synopsis(&self, title: &str, synopsis: &str, card_height: i32)`

## Dismiss Behavior

The gloss overlay already dismisses on Escape via existing key handling. Add Ctrl+H as a dismiss trigger:

In the keymap dispatch for `ShowSynopsisOverlay`, check if the gloss overlay is visible. If it is, hide it. If it isn't, show it with the current scene's synopsis.

This makes Ctrl+H a toggle: press once to show, press again (or Escape) to dismiss.

## No Synopsis Available

When `synopsis_cache` is empty for the current work or the current `(div1, div2)` has no entry, show a toast. Use the existing toast infrastructure (same pattern as "no concordance active" toast).

## Interaction with Existing h Bind

Plain `h` (`ToggleSynopsis`) continues to toggle the sidebar synopsis. `Ctrl+H` (`ShowSynopsisOverlay`) is independent — it shows the same data in the gloss overlay instead. Both can be active, but in practice the user will use one or the other.

If the gloss overlay is showing a synopsis and the user presses `h`, the sidebar toggles independently. If the sidebar is showing synopsis and the user presses `Ctrl+H`, the overlay appears on top. No special interaction logic needed — they are independent views of the same data.

## Files Not Modified

- `src/db/queries.rs` — `load_synopses` already exists
- `src/ui/vocab_popup.rs` — sidebar synopsis untouched

## Files Modified (updated list)

- `src/input/actions/mod.rs` — add `ShowSynopsisOverlay` variant, category, name
- `src/input/keymap_config.rs` — add `KeyCombo::ctrl("h")` default binding
- `src/input/keymap.rs` — route `ShowSynopsisOverlay` to handler
- `src/app.rs` — add `show_synopsis_overlay` function
- `src/ui/gloss_overlay.rs` — add `show_synopsis` method
- `~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json` — add Ctrl+H binding
