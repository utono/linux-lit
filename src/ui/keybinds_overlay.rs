use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Label, Orientation, Overlay};

#[derive(Clone, Copy)]
enum ModifierType {
    Bare,
    Ctrl,
    Alt,
    CtrlAlt,
}

struct KeybindEntry {
    key_label: &'static str,
    action: &'static str,
    modifier: ModifierType,
    sub_binds: &'static [(&'static str, &'static str, ModifierType)],
}

const fn kb(key_label: &'static str, action: &'static str) -> KeybindEntry {
    KeybindEntry { key_label, action, modifier: ModifierType::Bare, sub_binds: &[] }
}

const fn kb_sub(
    key_label: &'static str,
    action: &'static str,
    sub_binds: &'static [(&'static str, &'static str, ModifierType)],
) -> KeybindEntry {
    KeybindEntry { key_label, action, modifier: ModifierType::Bare, sub_binds }
}

const fn kb_unbound(key_label: &'static str) -> KeybindEntry {
    KeybindEntry { key_label, action: "", modifier: ModifierType::Bare, sub_binds: &[] }
}

const fn kb_mod(key_label: &'static str, action: &'static str, modifier: ModifierType) -> KeybindEntry {
    KeybindEntry { key_label, action, modifier, sub_binds: &[] }
}

// Number row: $ + [ { ( & = ) } ] * ! |
// Only bound keys: +, 0, !, |
const NUMBER_ROW: &[KeybindEntry] = &[
    kb_unbound("$"),
    kb("+", "toggle speed"),
    kb_unbound("["),
    kb_unbound("{"),
    kb_unbound("("),
    kb_unbound("&"),
    kb_unbound("="),
    kb_unbound(")"),
    kb_unbound("}"),
    kb_unbound("]"),
    kb("0", "reset font"),
    kb("! |", "font \u{2212} / +"),
];

// Upper row: ; , . p y f g c r l / @ \  (with tab indent)
const UPPER_ROW: &[KeybindEntry] = &[
    kb_unbound(";"),
    kb_sub(",", "prev dialogue", &[("C-,", "settings", ModifierType::Ctrl)]),
    kb(".", "set chapter"),
    kb_sub("p P", "nudge \u{2212}/+0.2s", &[("C-p", "picker", ModifierType::Ctrl)]),
    kb("y", "prev chunk"),
    kb_sub("f F", "cycle font \u{2192}/\u{2190}", &[("C-f", "pg fwd", ModifierType::Ctrl)]),
    kb_unbound("g"),
    kb_unbound("c"),
    kb_unbound("r"),
    kb("l", "toggle signs"),
    kb_sub("/", "search", &[("C-/", "keybinds", ModifierType::Ctrl)]),
];

// Home row: a o e u i d h t n s -
const HOME_ROW: &[KeybindEntry] = &[
    kb("a", "play from ts"),
    kb("o O", "seek \u{2212}3.5/\u{2212}60"),
    kb("e E", "seek +3.5/+60"),
    kb_sub("u", "start time/undo", &[("C-u", "pg back", ModifierType::Ctrl)]),
    kb("i", "set end time"),
    kb_mod("C-d", "pg fwd", ModifierType::Ctrl),
    kb_unbound("h"),
    kb("Tab", "play/pause"),
    kb("n N", "next/prev match"),
    kb_unbound("s"),
    kb_unbound("\u{2212}"),
];

// Bottom row: ' q j k x b m w v z
const BOTTOM_ROW: &[KeybindEntry] = &[
    kb_unbound("'"),
    kb("q", "next dialogue"),
    kb("j", "cursor \u{2193}"),
    kb("k", "cursor \u{2191}"),
    kb("x", "next chunk"),
    kb_mod("C-b", "pg back", ModifierType::Ctrl),
    kb("m", "media picker"),
    kb_unbound("w"),
    kb("V", "visual mode"),
    kb_unbound("z"),
];

// Other: gg, G, arrow, backspace, esc, ctrl+arrows, alt combos
const OTHER_ROW: &[KeybindEntry] = &[
    kb("g g", "go to start"),
    kb("G", "go to end"),
    kb("\u{2192}", "set start time"),
    kb("\u{232b}", "delete ts"),
    kb("Esc", "clear AB loop"),
    kb_mod("C-\u{2191} C-\u{2193}", "vol +/\u{2212}", ModifierType::Ctrl),
    kb_mod("M-f", "font info", ModifierType::Alt),
    kb_mod("M-i", "translations", ModifierType::Alt),
    kb_mod("C-M-l", "save + quit", ModifierType::CtrlAlt),
];

pub struct KeybindsOverlay {
    pub overlay: Overlay,
    container: GtkBox,
}

impl KeybindsOverlay {
    pub fn new() -> Self {
        let overlay = Overlay::new();

        let container = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(0)
            .halign(gtk4::Align::Center)
            .valign(gtk4::Align::Center)
            .width_request(820)
            .build();
        container.add_css_class("keybinds-overlay");

        let rows: &[(&str, &[KeybindEntry], i32)] = &[
            ("Number Row", NUMBER_ROW, 0),
            ("Upper Row", UPPER_ROW, 16),
            ("Home Row", HOME_ROW, 24),
            ("Bottom Row", BOTTOM_ROW, 36),
            ("Other", OTHER_ROW, 0),
        ];

        for &(title, entries, indent) in rows {
            let header = Label::builder()
                .label(title)
                .halign(gtk4::Align::Start)
                .css_classes(vec!["keybind-row-header"])
                .build();
            container.append(&header);

            let row_box = GtkBox::builder()
                .orientation(Orientation::Horizontal)
                .spacing(3)
                .margin_start(indent)
                .margin_bottom(10)
                .build();
            if title == "Other" {
                row_box.set_halign(gtk4::Align::Start);
            }

            for entry in entries {
                let cell = GtkBox::builder()
                    .orientation(Orientation::Vertical)
                    .css_classes(vec!["keybind-key"])
                    .build();

                if entry.action.is_empty() {
                    cell.add_css_class("keybind-key-unbound");
                }

                let modifier_class = match entry.modifier {
                    ModifierType::Bare => "keybind-label-bare",
                    ModifierType::Ctrl => "keybind-label-ctrl",
                    ModifierType::Alt => "keybind-label-alt",
                    ModifierType::CtrlAlt => "keybind-label-ctrlalt",
                };

                let key_label = Label::builder()
                    .label(entry.key_label)
                    .halign(gtk4::Align::Start)
                    .css_classes(vec![modifier_class])
                    .build();
                cell.append(&key_label);

                if !entry.action.is_empty() {
                    let action_label = Label::builder()
                        .label(entry.action)
                        .halign(gtk4::Align::Start)
                        .css_classes(vec!["keybind-action"])
                        .build();
                    cell.append(&action_label);
                }

                for &(sub_key, sub_action, sub_mod) in entry.sub_binds {
                    let sub_class = match sub_mod {
                        ModifierType::Bare => "keybind-label-bare",
                        ModifierType::Ctrl => "keybind-label-ctrl",
                        ModifierType::Alt => "keybind-label-alt",
                        ModifierType::CtrlAlt => "keybind-label-ctrlalt",
                    };
                    let sub_label = Label::builder()
                        .label(&format!("{} {}", sub_key, sub_action))
                        .halign(gtk4::Align::Start)
                        .css_classes(vec![sub_class, "keybind-action"])
                        .build();
                    cell.append(&sub_label);
                }

                row_box.append(&cell);
            }

            container.append(&row_box);
        }

        // Legend bar
        let legend = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .spacing(16)
            .css_classes(vec!["keybind-legend"])
            .build();

        let legend_items: &[(&str, &str)] = &[
            ("keybind-label-bare", "key"),
            ("keybind-label-ctrl", "Ctrl+"),
            ("keybind-label-alt", "Alt+"),
            ("keybind-label-ctrlalt", "Ctrl+Alt+"),
        ];
        for &(class, text) in legend_items {
            let item = Label::builder()
                .label(&format!("\u{25a0} {}", text))
                .css_classes(vec![class])
                .build();
            legend.append(&item);
        }

        let close_hint = Label::builder()
            .label("Esc to close \u{00b7} Ctrl+/ to toggle")
            .halign(gtk4::Align::End)
            .hexpand(true)
            .css_classes(vec!["keybind-action"])
            .build();
        legend.append(&close_hint);

        container.append(&legend);

        KeybindsOverlay { overlay, container }
    }

    pub fn show(&self) {
        self.container.set_visible(true);
    }

    pub fn hide(&self) {
        self.container.set_visible(false);
    }

    pub fn is_visible(&self) -> bool {
        self.container.is_visible()
    }

    pub fn attach(&self, base: &impl IsA<gtk4::Widget>) {
        self.overlay.set_child(Some(base));
        self.overlay.add_overlay(&self.container);
        self.container.set_visible(false);
    }
}
