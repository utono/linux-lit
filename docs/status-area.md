# Status Area

The bottom of the window has two mutually exclusive status bars, plus overlay indicators. All are hidden by default.

## Layout

```
vbox
  ├── main content overlay
  │     ├── sync-off icon (lower-left corner)
  │     └── word-status label (lower-left, above sync icon)
  ├── concordance bar
  └── search bar
```

## Concordance Bar

Activated when entering concordance mode on a word. Three-section horizontal bar:

- **Left:** `concordance: <word>` — the tracked word
- **Center:** position indicator (e.g. `3/12`)
- **Right:** hint text `r/R: next/prev | Esc: exit` (dimmed)

CSS class: `.concordance-bar`
Source: `src/ui/concordance_bar.rs`

## Search Bar

Activated by `/` (vim-style forward search). Three-widget horizontal bar:

- **Left:** `/` label
- **Center:** text entry (grabs focus on show)
- **Right:** match counter `[current/total]`

CSS class: `.search-bar`
Source: `src/ui/search_bar.rs`

## Overlay Icons

### Sync-Off Icon

- **Glyph:** `⇄̸` (crossed-out bidirectional arrow)
- **Position:** lower-left corner of the main content area (12px margins)
- **Keybind:** `s` toggles playback sync on/off
- **Visible when:** sync is disabled (`sync_enabled == false`)
- **Hidden when:** sync is enabled (default state)

CSS class: `.sync-off-icon`
Source: `src/app.rs:446-454`, toggled at `src/input/keymap.rs:993-998`

### Word Status Label

- **Content:** the word just copied to clipboard
- **Position:** lower-left corner, above the sync-off icon (12px start, 40px bottom margins)
- **Keybind:** `w` cycles through words on the cursor line
- **Behavior:** each press copies the next whitespace-split, punctuation-stripped word to the system clipboard via `wl-copy` and displays it. Wraps back to the first word after the last. Resets to the first word when the cursor moves to a different line.
- **Auto-hide:** disappears 2 seconds after the last `w` press (timer resets on each press)

CSS class: `.word-status`
Source: `src/app.rs:461-468`, handler at `src/input/navigation.rs:word_cycle_copy`
