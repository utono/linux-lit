use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{Align, Label, ScrolledWindow, TextView};
use std::cell::Cell;
use std::rc::Rc;

/// Which side of a "<document> + ask" overlay holds keyboard focus.
/// `Doc` = the synopsis/gloss card or journal page; `Ask` = the input field.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AskFocus {
    Doc,
    Ask,
}

/// The shared multi-line "ask" input card, stacked below a synopsis/gloss or
/// journal card. Built with the canonical synopsis values; both overlays embed
/// one and delegate their `*_ask_*` methods to it so the two cards can't drift.
pub struct AskCard {
    container: gtk4::Box,
    title: Label,
    input: TextView,
    hint: Label,
    focus: Cell<AskFocus>,
    return_focus: gtk4::Widget,
}

impl AskCard {
    /// Build the card with the canonical synopsis values. `return_focus` is the
    /// document-side widget that GTK focus returns to when leaving the input
    /// (gloss: its scroller; journal: its page view).
    pub fn new(text_margins: i32, return_focus: &impl IsA<gtk4::Widget>) -> Self {
        let container = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        container.add_css_class("ask-card");
        container.set_margin_top(14);
        container.set_margin_start(text_margins);
        container.set_margin_end(text_margins);
        container.set_margin_bottom(14);

        let title = Label::new(Some(""));
        title.add_css_class("gloss-header");
        title.set_halign(Align::Start);
        title.set_margin_start(16);
        title.set_margin_top(12);
        container.append(&title);

        let scrolled = ScrolledWindow::new();
        scrolled.set_min_content_height(160);
        scrolled.set_max_content_height(320);
        scrolled.set_hscrollbar_policy(gtk4::PolicyType::Never);
        scrolled.set_margin_start(16);
        scrolled.set_margin_end(16);
        scrolled.set_margin_top(6);
        scrolled.set_margin_bottom(6);

        let input = TextView::new();
        input.set_editable(true);
        input.set_cursor_visible(true);
        input.set_wrap_mode(gtk4::WrapMode::Word);
        input.set_top_margin(6);
        input.set_bottom_margin(6);
        input.set_left_margin(6);
        input.set_right_margin(6);
        input.add_css_class("gloss-text");
        input.add_css_class("ask-input");
        scrolled.set_child(Some(&input));
        container.append(&scrolled);

        let hint = Label::new(Some(""));
        hint.add_css_class("ask-hint");
        hint.set_halign(Align::Center);
        hint.set_margin_bottom(10);
        container.append(&hint);

        container.set_visible(false);

        Self {
            container,
            title,
            input,
            hint,
            focus: Cell::new(AskFocus::Doc),
            return_focus: return_focus.clone().upcast(),
        }
    }

    /// The card box; the embedding overlay appends this into its own card column.
    pub fn container(&self) -> &gtk4::Box {
        &self.container
    }

    /// The input TextView — exposed so each overlay applies its own font.
    pub fn input(&self) -> &TextView {
        &self.input
    }

    /// Reveal with heading + hint, clear the field, re-align margins to
    /// card_width/4, focus the input (AskFocus::Ask + card-focused highlight).
    pub fn open(&self, title: &str, hint: &str, card_width: i32) {
        self.title.set_text(title);
        self.hint.set_text(hint);
        self.input.buffer().set_text("");
        if card_width > 0 {
            let margin = crate::ui::card_side_margin(card_width);
            self.container.set_margin_start(margin);
            self.container.set_margin_end(margin);
        }
        self.container.set_visible(true);
        self.set_focus(AskFocus::Ask);
    }

    /// Hide, set AskFocus::Doc, drop the highlight, return focus to return_focus.
    pub fn close(&self) {
        self.container.set_visible(false);
        self.focus.set(AskFocus::Doc);
        self.container.remove_css_class("card-focused");
        self.container.remove_css_class("card-dimmed");
        if self.input.has_focus() {
            let _ = self.return_focus.grab_focus();
        }
    }

    pub fn is_open(&self) -> bool {
        self.container.is_visible()
    }

    pub fn focus(&self) -> AskFocus {
        self.focus.get()
    }

    /// Flip Doc<->Ask (no-op if closed). Owns the card-focused/card-dimmed
    /// highlight swap and the input grab / return-focus grab.
    pub fn toggle_focus(&self) {
        if !self.is_open() {
            return;
        }
        let next = match self.focus.get() {
            AskFocus::Doc => AskFocus::Ask,
            AskFocus::Ask => AskFocus::Doc,
        };
        self.set_focus(next);
    }

    fn set_focus(&self, focus: AskFocus) {
        self.focus.set(focus);
        match focus {
            AskFocus::Ask => {
                self.container.remove_css_class("card-dimmed");
                self.container.add_css_class("card-focused");
                self.input.grab_focus();
            }
            AskFocus::Doc => {
                self.container.remove_css_class("card-focused");
                self.container.add_css_class("card-dimmed");
                if self.input.has_focus() {
                    let _ = self.return_focus.grab_focus();
                }
            }
        }
    }

    /// Read and clear the input's text.
    pub fn take_text(&self) -> String {
        let buffer = self.input.buffer();
        let text = buffer
            .text(&buffer.start_iter(), &buffer.end_iter(), false)
            .to_string();
        buffer.set_text("");
        text
    }
}

/// Hosts an `AskCard` inside a free-scroll overlay (journal Q&A / gloss synopsis)
/// and owns the **fixed-scroll-height** lifecycle that keeps the ask card from
/// occluding the reading text.
///
/// The bug it fixes (see `docs/troubleshooting/clip-prevention.md`, "occlusion
/// is not clipping"): the overlay container is added non-measured, so its
/// `vexpand` scroll keeps full height; when the ask card is revealed the box
/// grows past its minimum and the extra extends off-pane rather than shrinking
/// the scroll — so the card draws *over* the bottom rows. No clip recompute can
/// fix that because the viewport never resized.
///
/// The fix (proven on real hardware): turn the scroll's `vexpand` **off** and
/// set its height EXPLICITLY. The host stores the CLOSED height (pane − title −
/// footer) and, on open, sets the OPEN height (pane − title − ask, the footer
/// being hidden while asking). With `vexpand` off there is no fight to race, so
/// the viewport shrinks deterministically and the text ends above the card.
///
/// Both overlays delegate their `*_ask_*` methods here so the mechanism is
/// written once. **Precondition:** the hosted `ScrolledWindow` must have
/// `vexpand(false)` — the host sets explicit heights and a `vexpand` scroll would
/// override them.
pub struct AskCardHost {
    ask: AskCard,
    scrolled: ScrolledWindow,
    /// Hidden while the ask card is open (the ask card carries its own hint);
    /// `None` for surfaces without a footer (gloss).
    footer: Option<gtk4::Box>,
    /// Recomputes the surface's bottom clip (its `BottomClipGuard`). Indirected
    /// as a closure so the host need not own the guard (which isn't `Clone`).
    recompute: Rc<dyn Fn()>,
    /// The scroll height while the ask card is CLOSED (pane − title − footer),
    /// recorded by `size`. The close path restores this stored value rather than
    /// re-measuring just-toggled chrome — `footer.preferred_size()` reads 0 right
    /// after the footer is re-shown, which would leave the scroll stuck shrunk.
    closed_scroll_h: Cell<i32>,
    /// Pane height (the card_height the surface was last sized to), recorded by
    /// `size`. With the fixed-chrome height it yields the OPEN scroll height.
    card_height: Cell<i32>,
    /// Total height of the chrome that stays put in BOTH the closed and open
    /// states — everything in the card column except the scroll, the ask card,
    /// and the toggled `footer` (which is hidden on open). For the journal that is
    /// the title (its nav footer IS the toggled `footer`, so it is NOT counted
    /// here). For gloss — which has no toggled footer — it is the title PLUS the
    /// always-visible hint row PLUS, in echoes mode, the source header + rule.
    /// Recorded by `size` (measured once when the card is sized — more stable than
    /// a per-open `preferred_size`). Open scroll height = `card_height −
    /// fixed_chrome_h − ask`; the toggled footer (if any) has already dropped out
    /// because it is hidden, so it is not in this sum.
    fixed_chrome_h: Cell<i32>,
    card_width: Cell<i32>,
}

impl AskCardHost {
    pub fn new(
        ask: AskCard,
        scrolled: &ScrolledWindow,
        footer: Option<gtk4::Box>,
        recompute: Rc<dyn Fn()>,
    ) -> Self {
        Self {
            ask,
            scrolled: scrolled.clone(),
            footer,
            recompute,
            closed_scroll_h: Cell::new(0),
            card_height: Cell::new(0),
            fixed_chrome_h: Cell::new(0),
            card_width: Cell::new(0),
        }
    }

    /// The hosted `AskCard` (e.g. so the surface can append its container or read
    /// its input for font application).
    pub fn card(&self) -> &AskCard {
        &self.ask
    }

    /// Record the card geometry and set the scroll's CLOSED height. Call from the
    /// surface's size/show paths (once per (re)size) BEFORE any open/close.
    ///
    /// `fixed_chrome_h` is the total height of every non-scroll, non-ask widget
    /// that STAYS visible when the ask card opens (journal: just the title; gloss:
    /// title + hint row, plus echoes' header + rule). `footer_h` is the height of
    /// the TOGGLED `footer` that is hidden on open (journal's nav footer; 0 for a
    /// surface whose footer is `None`). Stores `closed_scroll_h = card_height −
    /// fixed_chrome_h − footer_h` and sets the scroll to it.
    pub fn size(&self, card_width: i32, card_height: i32, fixed_chrome_h: i32, footer_h: i32) {
        self.card_width.set(card_width);
        self.card_height.set(card_height);
        self.fixed_chrome_h.set(fixed_chrome_h);
        let scroll_h = (card_height - fixed_chrome_h - footer_h).max(80);
        self.closed_scroll_h.set(scroll_h);
        self.pin_scroll_height(scroll_h);
    }

    /// Pin the scroll viewport to EXACTLY `h` — both a floor (`height_request`)
    /// AND a cap (`max_content_height`). `height_request` alone is only a
    /// *minimum*: GTK is free to allocate the scroll TALLER when the
    /// `valign=Center` container has room (which it does once the box re-lays-out
    /// on a re-open), so the shrink was silently reverted to full height (proven:
    /// first ask-open held 817, a later one read 1025). `max_content_height` is a
    /// real cap on the viewport's natural height, so the box cannot over-allocate
    /// it. `propagate_natural_height(false)` is already set on both scrolls.
    fn pin_scroll_height(&self, h: i32) {
        self.scrolled.set_height_request(h);
        self.scrolled.set_max_content_height(h);
        self.scrolled.set_min_content_height(h);
        // Force a re-allocation NOW. `set_*_content_height` only updates the
        // widget's requested size; without an explicit invalidation GTK may not
        // re-run layout (the height "stuck" at the old value on a later open —
        // page_size never updated, so the viewport never shrank). queue_resize
        // schedules the relayout; the BottomClipGuard's page_size-notify handler
        // then recomputes the clip against the settled viewport.
        self.scrolled.queue_resize();
    }

    /// Reveal the ask card and SHRINK the scroll so the reading text ends above
    /// it (the occlusion fix). Open height = pane − fixed chrome − ask-natural;
    /// the toggled footer (if any) is hidden, freeing its slot. Recomputes the
    /// clip now and on the idle tick after the height lands.
    pub fn open(&self, title: &str, hint: &str) {
        self.ask.open(title, hint, self.card_width.get());
        if let Some(f) = &self.footer {
            f.set_visible(false);
        }
        let (_, ask_size) = self.ask.container().preferred_size();
        let scroll_h =
            (self.card_height.get() - self.fixed_chrome_h.get() - ask_size.height()).max(80);
        self.pin_scroll_height(scroll_h);
        self.recompute_now_and_idle();
    }

    /// Hide the ask card and RESTORE the scroll's closed height (re-shows the
    /// footer first). Uses the stored `closed_scroll_h` rather than re-measuring.
    pub fn close(&self) {
        self.ask.close();
        if let Some(f) = &self.footer {
            f.set_visible(true);
        }
        self.pin_scroll_height(self.closed_scroll_h.get().max(80));
        self.recompute_now_and_idle();
    }

    /// Recompute the clip synchronously AND on the next idle tick — the
    /// `set_height_request` lands on a later layout pass, so the synchronous
    /// recompute runs against the stale viewport and the idle one against the
    /// settled (shrunk/restored) viewport.
    fn recompute_now_and_idle(&self) {
        (self.recompute)();
        let recompute = self.recompute.clone();
        glib::idle_add_local_once(move || recompute());
    }

    pub fn toggle_focus(&self) {
        self.ask.toggle_focus();
    }

    pub fn take_text(&self) -> String {
        self.ask.take_text()
    }

    pub fn is_open(&self) -> bool {
        self.ask.is_open()
    }

    pub fn focus(&self) -> AskFocus {
        self.ask.focus()
    }

    pub fn input(&self) -> &TextView {
        self.ask.input()
    }
}
