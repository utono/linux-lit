//! Vim registers: the unnamed register plus named `"a`–`"z`. Each holds text
//! and a linewise flag (so `p` knows whether to put charwise or open a line).

use std::collections::HashMap;

pub struct Registers {
    unnamed: String,
    unnamed_linewise: bool,
    named: HashMap<char, (String, bool)>,
}

impl Registers {
    pub fn new() -> Self {
        Registers {
            unnamed: String::new(),
            unnamed_linewise: false,
            named: HashMap::new(),
        }
    }

    /// Store `text` into `reg` (None = unnamed). A named yank ALSO updates the
    /// unnamed register, matching vim.
    pub fn yank(&mut self, reg: Option<char>, text: String, linewise: bool) {
        if let Some(r) = reg {
            self.named.insert(r.to_ascii_lowercase(), (text.clone(), linewise));
        }
        self.unnamed = text;
        self.unnamed_linewise = linewise;
    }

    /// Read `reg` (None = unnamed). Returns (text, linewise).
    pub fn get(&self, reg: Option<char>) -> Option<(&str, bool)> {
        match reg {
            Some(r) => self
                .named
                .get(&r.to_ascii_lowercase())
                .map(|(t, l)| (t.as_str(), *l)),
            None => {
                if self.unnamed.is_empty() {
                    None
                } else {
                    Some((self.unnamed.as_str(), self.unnamed_linewise))
                }
            }
        }
    }
}

impl Default for Registers {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unnamed_roundtrip() {
        let mut r = Registers::new();
        assert!(r.get(None).is_none());
        r.yank(None, "hi".into(), false);
        assert_eq!(r.get(None), Some(("hi", false)));
    }

    #[test]
    fn named_also_updates_unnamed() {
        let mut r = Registers::new();
        r.yank(Some('a'), "x".into(), true);
        assert_eq!(r.get(Some('a')), Some(("x", true)));
        assert_eq!(r.get(None), Some(("x", true)));
    }
}
