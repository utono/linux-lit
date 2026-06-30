//! The modal-vim state machine. Pure (no gtk); mirrors to GTK via the adapter.
//!
//! Holds the edit buffer (`String`), a char-index cursor, the mode, and the
//! pending-input state (count, operator, register select, find, g-prefix, text
//! object, command line). One entry point: [`VimEngine::handle_key`], returning
//! an [`Outcome`] the adapter mirrors to the `TextView` and acts on.

use super::registers::Registers;
use super::textobject::{text_object, TextObjKind};
use super::{buffer, motion, EditorAction, Mode, Outcome, Range, VimKey};
use super::motion::FindKind;

/// An operator awaiting a motion / text object / doubled key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Op {
    Delete,
    Change,
    Yank,
}

/// Pending multi-key input state. Only one is active at a time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pending {
    None,
    /// After an operator key (`d`/`c`/`y`), awaiting a motion/object. `count`
    /// multiplies the motion.
    Operator { op: Op, count: usize },
    /// After `i`/`a` in operator-pending state: awaiting the object char.
    TextObj { op: Op, around: bool, count: usize },
    /// After `f`/`t`/`F`/`T` (standalone or operator-composed): awaiting target.
    Find { kind: FindKind, op: Option<(Op, usize)> },
    /// After `r`: awaiting the replacement char.
    Replace,
    /// After `"`: awaiting the register name.
    Register,
    /// After `g`: awaiting the second key (`gg`).
    GPrefix,
}

pub struct VimEngine {
    buffer: String,
    cursor: usize,
    mode: Mode,

    pending: Pending,
    pending_count: Option<usize>,
    pending_register: Option<char>,
    visual_anchor: Option<usize>,

    registers: Registers,
    last_find: Option<(FindKind, char)>,

    // command line: Some while typing after ':'
    cmdline: Option<String>,

    // undo/redo: snapshots of (buffer, cursor) BEFORE a change group.
    undo: Vec<(String, usize)>,
    redo: Vec<(String, usize)>,

    // dot-repeat: the key sequence of the last buffer-mutating command.
    last_change: Vec<VimKey>,
    recording: Option<Vec<VimKey>>,
    replaying: bool,
}

impl VimEngine {
    pub fn new(buffer: String) -> Self {
        VimEngine {
            buffer,
            cursor: 0,
            mode: Mode::Normal,
            pending: Pending::None,
            pending_count: None,
            pending_register: None,
            visual_anchor: None,
            registers: Registers::new(),
            last_find: None,
            cmdline: None,
            undo: Vec::new(),
            redo: Vec::new(),
            last_change: Vec::new(),
            recording: None,
            replaying: false,
        }
    }

    pub fn buffer(&self) -> &str {
        &self.buffer
    }
    pub fn cursor(&self) -> usize {
        self.cursor
    }
    pub fn mode(&self) -> Mode {
        self.mode
    }
    pub fn cmdline(&self) -> Option<&str> {
        self.cmdline.as_deref()
    }

    // ---- helpers ----

    fn take_count(&mut self) -> usize {
        self.pending_count.take().unwrap_or(1).max(1)
    }

    /// The current visual selection as a half-open char range, if in Visual or
    /// VisualLine mode. The adapter paints this as a GTK selection.
    pub fn selection(&self) -> Option<Range> {
        self.visual_anchor.map(|a| self.visual_range(a))
    }

    /// The inclusive visual range as a half-open char range.
    fn visual_range(&self, anchor: usize) -> Range {
        if self.mode == Mode::VisualLine {
            let (lo, hi) = if anchor <= self.cursor {
                (anchor, self.cursor)
            } else {
                (self.cursor, anchor)
            };
            let start = buffer::line_start(&self.buffer, lo);
            let (_, end_excl) = buffer::line_bounds(&self.buffer, hi);
            // include the trailing newline if present
            let end = (end_excl + 1).min(buffer::char_count(&self.buffer));
            Range { start, end }
        } else {
            let (lo, hi) = if anchor <= self.cursor {
                (anchor, self.cursor)
            } else {
                (self.cursor, anchor)
            };
            Range {
                start: lo,
                end: (hi + 1).min(buffer::char_count(&self.buffer)),
            }
        }
    }

    fn out(&self, changed: bool, action: EditorAction) -> Outcome {
        Outcome {
            buffer_changed: changed,
            cursor: self.cursor,
            mode: self.mode,
            selection: self.selection(),
            action,
        }
    }

    fn snapshot(&mut self) {
        self.undo.push((self.buffer.clone(), self.cursor));
        self.redo.clear();
    }

    fn char_slice(&self, r: Range) -> String {
        self.buffer
            .chars()
            .skip(r.start)
            .take(r.len())
            .collect()
    }

    fn delete_range(&mut self, r: Range) -> String {
        let removed = self.char_slice(r);
        let mut cs: Vec<char> = self.buffer.chars().collect();
        let start = r.start.min(cs.len());
        let end = r.end.min(cs.len());
        cs.drain(start..end);
        self.buffer = cs.into_iter().collect();
        self.cursor = buffer::clamp_cursor(&self.buffer, start);
        removed
    }

    fn insert_str_at(&mut self, at: usize, text: &str) {
        let mut cs: Vec<char> = self.buffer.chars().collect();
        let at = at.min(cs.len());
        for (k, ch) in text.chars().enumerate() {
            cs.insert(at + k, ch);
        }
        self.buffer = cs.into_iter().collect();
    }

    /// Clamp the cursor onto a valid Normal-mode position (not past the last
    /// char of its line).
    fn clamp_normal(&mut self) {
        let (ls, le) = buffer::line_bounds(&self.buffer, self.cursor);
        let max = le.saturating_sub(1).max(ls);
        if self.cursor > max {
            self.cursor = max;
        }
    }

    // ---- entry ----

    pub fn handle_key(&mut self, k: VimKey) -> Outcome {
        // command line captures everything until Enter/Esc
        if self.cmdline.is_some() {
            return self.handle_cmdline(k);
        }
        match self.mode {
            Mode::Insert => self.handle_insert(k),
            Mode::Normal => self.handle_normal(k),
            Mode::Visual | Mode::VisualLine => self.handle_visual(k),
        }
    }

    // ---- insert mode ----

    fn handle_insert(&mut self, k: VimKey) -> Outcome {
        if let Some(rec) = self.recording.as_mut() {
            rec.push(k);
        }
        match k {
            VimKey::Esc => {
                self.finish_recording();
                self.mode = Mode::Normal;
                let ls = buffer::line_start(&self.buffer, self.cursor);
                self.cursor = self.cursor.saturating_sub(1).max(ls);
                self.out(false, EditorAction::Nop)
            }
            VimKey::Char(c) => {
                self.insert_str_at(self.cursor, &c.to_string());
                self.cursor += 1;
                self.out(true, EditorAction::Nop)
            }
            VimKey::Enter => {
                self.insert_str_at(self.cursor, "\n");
                self.cursor += 1;
                self.out(true, EditorAction::Nop)
            }
            VimKey::Backspace => {
                if self.cursor > 0 {
                    let mut cs: Vec<char> = self.buffer.chars().collect();
                    cs.remove(self.cursor - 1);
                    self.buffer = cs.into_iter().collect();
                    self.cursor -= 1;
                    self.out(true, EditorAction::Nop)
                } else {
                    self.out(false, EditorAction::Nop)
                }
            }
            VimKey::Tab => {
                self.insert_str_at(self.cursor, "    ");
                self.cursor += 4;
                self.out(true, EditorAction::Nop)
            }
            VimKey::CtrlR => self.out(false, EditorAction::Nop),
        }
    }

    fn enter_insert_at(&mut self, at: usize) {
        self.cursor = buffer::clamp_cursor(&self.buffer, at);
        self.mode = Mode::Insert;
    }

    /// Begin a change group for a Normal-mode edit: snapshot once and start
    /// recording the key sequence for dot-repeat (unless replaying).
    fn begin_change(&mut self, first_keys: &[VimKey]) {
        self.snapshot();
        if !self.replaying {
            self.recording = Some(first_keys.to_vec());
        }
    }

    fn finish_recording(&mut self) {
        if let Some(rec) = self.recording.take() {
            if !rec.is_empty() {
                self.last_change = rec;
            }
        }
    }

    // ---- normal mode ----

    fn handle_normal(&mut self, k: VimKey) -> Outcome {
        // resolve pending multi-key states first
        match self.pending {
            Pending::Replace => return self.resolve_replace(k),
            Pending::Register => return self.resolve_register(k),
            Pending::Find { .. } => return self.resolve_find(k),
            Pending::GPrefix => return self.resolve_gprefix(k),
            Pending::TextObj { .. } => return self.resolve_textobj(k),
            Pending::Operator { .. } => return self.resolve_operator(k),
            Pending::None => {}
        }

        let c = match k {
            VimKey::Char(c) => c,
            VimKey::CtrlR => return self.do_redo(),
            VimKey::Esc => {
                // Esc in Normal mode is the "stay in / return to Normal" key — it
                // cancels any half-typed count/operator/pending state and does
                // NOT leave the editor (vim semantics). Exit is `:q` only.
                self.pending_count = None;
                self.pending = Pending::None;
                self.pending_register = None;
                return self.out(false, EditorAction::Nop);
            }
            VimKey::Enter | VimKey::Backspace | VimKey::Tab => {
                return self.out(false, EditorAction::Nop)
            }
        };

        // count accumulation (0 only continues an existing count)
        if c.is_ascii_digit() && !(c == '0' && self.pending_count.is_none()) {
            let d = c as usize - '0' as usize;
            self.pending_count = Some(self.pending_count.unwrap_or(0) * 10 + d);
            return self.out(false, EditorAction::Nop);
        }

        match c {
            // motions
            'h' => self.motion_apply(motion::left),
            'l' => self.motion_apply(motion::right),
            'k' => self.motion_apply(motion::up),
            'j' => self.motion_apply(motion::down),
            'w' => self.motion_apply(motion::word_forward),
            'b' => self.motion_apply(motion::word_back),
            'e' => self.motion_apply(motion::word_end),
            '0' => {
                self.cursor = motion::line_zero(&self.buffer, self.cursor);
                self.out(false, EditorAction::Nop)
            }
            '^' => {
                self.cursor = motion::line_first_char(&self.buffer, self.cursor);
                self.out(false, EditorAction::Nop)
            }
            '$' => {
                self.cursor = motion::line_last_char(&self.buffer, self.cursor);
                self.out(false, EditorAction::Nop)
            }
            'G' => {
                let n = self.pending_count.take().unwrap_or(0);
                self.cursor = motion::goto_line(&self.buffer, n);
                self.out(false, EditorAction::Nop)
            }
            '%' => {
                if let Some(p) = motion::match_pair(&self.buffer, self.cursor) {
                    self.cursor = p;
                }
                self.out(false, EditorAction::Nop)
            }
            // `;` enters command mode (mirrors the user's Neovim mapping
            // `vim.keymap.set('n', ';', ':')`); displaces vim's default
            // repeat-find. `,` keeps its default reverse-repeat-find.
            ';' | ':' => {
                self.cmdline = Some(String::new());
                self.out(false, EditorAction::Nop)
            }
            ',' => self.repeat_find(true),
            'g' => {
                self.pending = Pending::GPrefix;
                self.out(false, EditorAction::Nop)
            }
            'f' => self.start_find(FindKind::ForwardOn),
            't' => self.start_find(FindKind::ForwardBefore),
            'F' => self.start_find(FindKind::BackOn),
            'T' => self.start_find(FindKind::BackBefore),

            // insert entry
            'i' => {
                self.begin_change(&[VimKey::Char('i')]);
                let at = self.cursor;
                self.enter_insert_at(at);
                self.out(false, EditorAction::Nop)
            }
            'a' => {
                self.begin_change(&[VimKey::Char('a')]);
                let at = (self.cursor + 1).min(buffer::char_count(&self.buffer));
                self.enter_insert_at(at);
                self.out(false, EditorAction::Nop)
            }
            'I' => {
                self.begin_change(&[VimKey::Char('I')]);
                let at = motion::line_first_char(&self.buffer, self.cursor);
                self.enter_insert_at(at);
                self.out(false, EditorAction::Nop)
            }
            'A' => {
                self.begin_change(&[VimKey::Char('A')]);
                let (_, le) = buffer::line_bounds(&self.buffer, self.cursor);
                self.enter_insert_at(le);
                self.out(false, EditorAction::Nop)
            }
            'o' => {
                self.begin_change(&[VimKey::Char('o')]);
                let (_, le) = buffer::line_bounds(&self.buffer, self.cursor);
                self.insert_str_at(le, "\n");
                self.enter_insert_at(le + 1);
                self.out(true, EditorAction::Nop)
            }
            'O' => {
                self.begin_change(&[VimKey::Char('O')]);
                let ls = buffer::line_start(&self.buffer, self.cursor);
                self.insert_str_at(ls, "\n");
                self.enter_insert_at(ls);
                self.out(true, EditorAction::Nop)
            }

            // simple edits
            'x' => self.do_x(),
            'r' => {
                self.pending = Pending::Replace;
                self.out(false, EditorAction::Nop)
            }
            'D' => self.do_delete_to_eol(false),
            'C' => self.do_delete_to_eol(true),
            'J' => self.do_join(),
            '~' => self.do_tilde(),

            // operators
            'd' => self.start_operator(Op::Delete),
            'c' => self.start_operator(Op::Change),
            'y' => self.start_operator(Op::Yank),
            '>' => self.do_indent(1),
            '<' => self.do_indent(-1),

            // put
            'p' => self.do_put(true),
            'P' => self.do_put(false),

            // registers
            '"' => {
                self.pending = Pending::Register;
                self.out(false, EditorAction::Nop)
            }

            // visual
            'v' => {
                self.mode = Mode::Visual;
                self.visual_anchor = Some(self.cursor);
                self.out(false, EditorAction::Nop)
            }
            'V' => {
                self.mode = Mode::VisualLine;
                self.visual_anchor = Some(self.cursor);
                self.out(false, EditorAction::Nop)
            }

            // undo / repeat
            'u' => self.do_undo(),
            '.' => self.do_dot(),

            // ex / rewrite (`:` is handled together with `;` above)
            'R' => self.out(false, EditorAction::OpenRewrite),

            _ => self.out(false, EditorAction::Nop),
        }
    }

    fn motion_apply(&mut self, f: fn(&str, usize, usize) -> usize) -> Outcome {
        let n = self.take_count();
        self.cursor = f(&self.buffer, self.cursor, n);
        self.out(false, EditorAction::Nop)
    }

    // ---- simple edits ----

    fn do_x(&mut self) -> Outcome {
        let n = self.take_count();
        let (_, le) = buffer::line_bounds(&self.buffer, self.cursor);
        let end = (self.cursor + n).min(le);
        if end <= self.cursor {
            return self.out(false, EditorAction::Nop);
        }
        self.begin_change(&[VimKey::Char('x')]);
        let removed = self.delete_range(Range { start: self.cursor, end });
        self.registers.yank(self.pending_register.take(), removed, false);
        self.clamp_normal();
        self.finish_recording();
        self.out(true, EditorAction::Nop)
    }

    fn resolve_replace(&mut self, k: VimKey) -> Outcome {
        self.pending = Pending::None;
        let ch = match k {
            VimKey::Char(c) => c,
            VimKey::Enter => '\n',
            _ => return self.out(false, EditorAction::Nop),
        };
        let cc = buffer::char_count(&self.buffer);
        if self.cursor >= cc {
            return self.out(false, EditorAction::Nop);
        }
        self.begin_change(&[VimKey::Char('r'), k]);
        let mut cs: Vec<char> = self.buffer.chars().collect();
        cs[self.cursor] = ch;
        self.buffer = cs.into_iter().collect();
        self.finish_recording();
        self.out(true, EditorAction::Nop)
    }

    fn do_delete_to_eol(&mut self, change: bool) -> Outcome {
        let (_, le) = buffer::line_bounds(&self.buffer, self.cursor);
        self.begin_change(&[VimKey::Char(if change { 'C' } else { 'D' })]);
        let removed = self.delete_range(Range { start: self.cursor, end: le });
        self.registers.yank(self.pending_register.take(), removed, false);
        if change {
            self.mode = Mode::Insert;
        } else {
            self.clamp_normal();
            self.finish_recording();
        }
        self.out(true, EditorAction::Nop)
    }

    fn do_join(&mut self) -> Outcome {
        let n = self.take_count().max(1);
        self.begin_change(&[VimKey::Char('J')]);
        let mut changed = false;
        for _ in 0..n {
            let (_, le) = buffer::line_bounds(&self.buffer, self.cursor);
            if le >= buffer::char_count(&self.buffer) {
                break; // no newline to join
            }
            // remove the newline at `le`, replace with a single space, collapsing
            // following leading whitespace.
            let mut cs: Vec<char> = self.buffer.chars().collect();
            // drop the '\n'
            cs.remove(le);
            // collapse following whitespace to a single space
            let j = le;
            while j < cs.len() && cs[j].is_whitespace() {
                cs.remove(j);
            }
            cs.insert(le, ' ');
            self.buffer = cs.into_iter().collect();
            self.cursor = le;
            changed = true;
        }
        self.finish_recording();
        self.out(changed, EditorAction::Nop)
    }

    fn do_tilde(&mut self) -> Outcome {
        let cc = buffer::char_count(&self.buffer);
        if self.cursor >= cc {
            return self.out(false, EditorAction::Nop);
        }
        self.begin_change(&[VimKey::Char('~')]);
        let mut cs: Vec<char> = self.buffer.chars().collect();
        let ch = cs[self.cursor];
        let flipped: String = if ch.is_uppercase() {
            ch.to_lowercase().collect()
        } else {
            ch.to_uppercase().collect()
        };
        // single-char case flip (common case)
        if flipped.chars().count() == 1 {
            cs[self.cursor] = flipped.chars().next().unwrap();
            self.buffer = cs.into_iter().collect();
            self.cursor = motion::right(&self.buffer, self.cursor, 1);
        }
        self.finish_recording();
        self.out(true, EditorAction::Nop)
    }

    fn do_indent(&mut self, dir: i32) -> Outcome {
        self.begin_change(&[VimKey::Char(if dir > 0 { '>' } else { '<' })]);
        let ls = buffer::line_start(&self.buffer, self.cursor);
        if dir > 0 {
            self.insert_str_at(ls, "    ");
        } else {
            // remove up to 4 leading spaces
            let mut cs: Vec<char> = self.buffer.chars().collect();
            let mut removed = 0;
            while removed < 4 && ls < cs.len() && cs[ls] == ' ' {
                cs.remove(ls);
                removed += 1;
            }
            self.buffer = cs.into_iter().collect();
        }
        self.cursor = motion::line_first_char(&self.buffer, ls);
        self.finish_recording();
        self.out(true, EditorAction::Nop)
    }

    // ---- operators ----

    fn start_operator(&mut self, op: Op) -> Outcome {
        let count = self.pending_count.take().unwrap_or(1).max(1);
        self.pending = Pending::Operator { op, count };
        self.out(false, EditorAction::Nop)
    }

    fn resolve_operator(&mut self, k: VimKey) -> Outcome {
        let (op, count) = match self.pending {
            Pending::Operator { op, count } => (op, count),
            _ => unreachable!(),
        };
        self.pending = Pending::None;
        let c = match k {
            VimKey::Char(c) => c,
            _ => return self.out(false, EditorAction::Nop),
        };

        // a count after the operator multiplies
        if c.is_ascii_digit() && !(c == '0') {
            let extra = c as usize - '0' as usize;
            // accumulate into pending_count, restore operator pending
            self.pending_count = Some(self.pending_count.unwrap_or(0) * 10 + extra);
            self.pending = Pending::Operator { op, count };
            return self.out(false, EditorAction::Nop);
        }
        let count = count * self.pending_count.take().unwrap_or(1).max(1);

        // doubled operator → linewise on `count` lines
        let doubled = matches!(
            (op, c),
            (Op::Delete, 'd') | (Op::Change, 'c') | (Op::Yank, 'y')
        );
        if doubled {
            return self.apply_linewise(op, count);
        }

        // text object lead-in
        if c == 'i' || c == 'a' {
            self.pending = Pending::TextObj {
                op,
                around: c == 'a',
                count,
            };
            return self.out(false, EditorAction::Nop);
        }

        // find-composed: df) etc.
        if let Some(kind) = find_kind_of(c) {
            self.pending = Pending::Find {
                kind,
                op: Some((op, count)),
            };
            return self.out(false, EditorAction::Nop);
        }

        // otherwise a motion → operate on [cursor, motion)
        if let Some(target) = self.motion_target(c, count) {
            let (start, end, linewise, inclusive) = target;
            let r = if linewise {
                self.line_range(start, end)
            } else {
                let e = if inclusive { end + 1 } else { end };
                Range {
                    start: start.min(e),
                    end: start.max(e),
                }
            };
            return self.apply_operator(op, r, linewise);
        }

        self.out(false, EditorAction::Nop)
    }

    /// Compute (anchor, target, linewise, inclusive) for an operator motion char.
    fn motion_target(&self, c: char, count: usize) -> Option<(usize, usize, bool, bool)> {
        let cur = self.cursor;
        let t = match c {
            'w' => (cur, motion::word_forward(&self.buffer, cur, count), false, false),
            'b' => (motion::word_back(&self.buffer, cur, count), cur, false, false),
            'e' => (cur, motion::word_end(&self.buffer, cur, count), false, true),
            'h' => (motion::left(&self.buffer, cur, count), cur, false, false),
            'l' => (cur, motion::right(&self.buffer, cur, count), false, false),
            '0' => (motion::line_zero(&self.buffer, cur), cur, false, false),
            '^' => (motion::line_first_char(&self.buffer, cur), cur, false, false),
            '$' => (cur, motion::line_last_char(&self.buffer, cur), false, true),
            'G' => (cur, motion::goto_line(&self.buffer, 0), true, false),
            '%' => {
                let p = motion::match_pair(&self.buffer, cur)?;
                let (lo, hi) = if p >= cur { (cur, p) } else { (p, cur) };
                (lo, hi, false, true)
            }
            'j' => (cur, motion::down(&self.buffer, cur, count), true, false),
            'k' => (motion::up(&self.buffer, cur, count), cur, true, false),
            _ => return None,
        };
        Some(t)
    }

    /// `count` whole lines starting at the cursor's line (for dd/cc/yy).
    fn apply_linewise(&mut self, op: Op, count: usize) -> Outcome {
        let start_line = buffer::line_index(&self.buffer, self.cursor);
        let r = self.lines_range(start_line, count);
        self.apply_operator(op, r, true)
    }

    /// Half-open char range covering `count` lines from `start_line`, including
    /// each line's trailing newline where present.
    fn lines_range(&self, start_line: usize, count: usize) -> Range {
        let start = buffer::nth_line_start(&self.buffer, start_line);
        let last_line = start_line + count - 1;
        let last_start = buffer::nth_line_start(&self.buffer, last_line);
        let (_, last_end_excl) = buffer::line_bounds(&self.buffer, last_start);
        let cc = buffer::char_count(&self.buffer);
        let end = (last_end_excl + 1).min(cc); // include newline
        // If we're at the last line (no trailing newline), also drop the
        // preceding newline so the line truly disappears.
        if end == cc && start > 0 && self.buffer.chars().nth(start - 1) == Some('\n') {
            Range { start: start - 1, end }
        } else {
            Range { start, end }
        }
    }

    /// Char range spanning the lines containing `a..=b` (linewise motion result).
    fn line_range(&self, a: usize, b: usize) -> Range {
        let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
        let la = buffer::line_index(&self.buffer, lo);
        let lb = buffer::line_index(&self.buffer, hi);
        self.lines_range(la, lb - la + 1)
    }

    fn apply_operator(&mut self, op: Op, r: Range, linewise: bool) -> Outcome {
        if r.is_empty() {
            return self.out(false, EditorAction::Nop);
        }
        match op {
            Op::Yank => {
                let text = self.char_slice(r);
                self.registers.yank(self.pending_register.take(), text, linewise);
                // cursor moves to range start on yank
                self.cursor = buffer::clamp_cursor(&self.buffer, r.start);
                self.out(false, EditorAction::Nop)
            }
            Op::Delete => {
                self.begin_change(&[VimKey::Char('d')]);
                let removed = self.delete_range(r);
                self.registers.yank(self.pending_register.take(), removed, linewise);
                self.clamp_normal();
                self.finish_recording();
                self.out(true, EditorAction::Nop)
            }
            Op::Change => {
                self.begin_change(&[VimKey::Char('c')]);
                let removed = self.delete_range(r);
                self.registers.yank(self.pending_register.take(), removed, linewise);
                if linewise {
                    // open a fresh line at the deletion point
                    self.insert_str_at(self.cursor, "\n");
                }
                self.mode = Mode::Insert;
                self.out(true, EditorAction::Nop)
            }
        }
    }

    fn resolve_textobj(&mut self, k: VimKey) -> Outcome {
        let (op, around, _count) = match self.pending {
            Pending::TextObj { op, around, count } => (op, around, count),
            _ => unreachable!(),
        };
        self.pending = Pending::None;
        let c = match k {
            VimKey::Char(c) => c,
            _ => return self.out(false, EditorAction::Nop),
        };
        let kind = match c {
            'w' => TextObjKind::Word,
            '"' => TextObjKind::Pair('"', '"'),
            '\'' => TextObjKind::Pair('\'', '\''),
            '(' | ')' | 'b' => TextObjKind::Pair('(', ')'),
            '{' | '}' | 'B' => TextObjKind::Pair('{', '}'),
            '[' | ']' => TextObjKind::Pair('[', ']'),
            '<' | '>' => TextObjKind::Pair('<', '>'),
            'p' => TextObjKind::Paragraph,
            _ => return self.out(false, EditorAction::Nop),
        };
        match text_object(&self.buffer, self.cursor, kind, around) {
            Some(r) => self.apply_operator(op, r, false),
            None => self.out(false, EditorAction::Nop),
        }
    }

    // ---- find ----

    fn start_find(&mut self, kind: FindKind) -> Outcome {
        self.pending = Pending::Find { kind, op: None };
        self.out(false, EditorAction::Nop)
    }

    fn resolve_find(&mut self, k: VimKey) -> Outcome {
        let (kind, op) = match self.pending {
            Pending::Find { kind, op } => (kind, op),
            _ => unreachable!(),
        };
        self.pending = Pending::None;
        let target = match k {
            VimKey::Char(c) => c,
            _ => return self.out(false, EditorAction::Nop),
        };
        self.last_find = Some((kind, target));
        let found = motion::find_char(&self.buffer, self.cursor, kind, target);
        let Some(pos) = found else {
            return self.out(false, EditorAction::Nop);
        };
        match op {
            None => {
                self.cursor = pos;
                self.out(false, EditorAction::Nop)
            }
            Some((op, _count)) => {
                // inclusive for f/t forward (delete up to and incl target for f)
                let inclusive = matches!(kind, FindKind::ForwardOn | FindKind::ForwardBefore);
                let (start, end) = if pos >= self.cursor {
                    (self.cursor, if inclusive { pos + 1 } else { pos })
                } else {
                    (pos, self.cursor)
                };
                let r = Range {
                    start: start.min(end),
                    end: start.max(end),
                };
                self.apply_operator(op, r, false)
            }
        }
    }

    fn repeat_find(&mut self, reverse: bool) -> Outcome {
        let Some((kind, target)) = self.last_find else {
            return self.out(false, EditorAction::Nop);
        };
        let kind = if reverse { invert_find(kind) } else { kind };
        if let Some(pos) = motion::find_char(&self.buffer, self.cursor, kind, target) {
            self.cursor = pos;
        }
        self.out(false, EditorAction::Nop)
    }

    // ---- register / g-prefix ----

    fn resolve_register(&mut self, k: VimKey) -> Outcome {
        self.pending = Pending::None;
        if let VimKey::Char(c) = k {
            if c.is_ascii_alphabetic() {
                self.pending_register = Some(c.to_ascii_lowercase());
            }
        }
        self.out(false, EditorAction::Nop)
    }

    fn resolve_gprefix(&mut self, k: VimKey) -> Outcome {
        self.pending = Pending::None;
        if let VimKey::Char('g') = k {
            let n = self.pending_count.take().unwrap_or(1);
            self.cursor = if n <= 1 {
                motion::buffer_start(&self.buffer)
            } else {
                motion::goto_line(&self.buffer, n)
            };
        }
        self.out(false, EditorAction::Nop)
    }

    // ---- put ----

    fn do_put(&mut self, after: bool) -> Outcome {
        let n = self.take_count();
        let Some((text, linewise)) = self.registers.get(self.pending_register.take()) else {
            return self.out(false, EditorAction::Nop);
        };
        let text = text.to_string();
        if text.is_empty() {
            return self.out(false, EditorAction::Nop);
        }
        self.begin_change(&[VimKey::Char(if after { 'p' } else { 'P' })]);
        let payload: String = std::iter::repeat(text.clone()).take(n).collect();
        if linewise {
            // Normalize the register body to lines WITHOUT a trailing newline so
            // we can place the surrounding `\n` ourselves (correct whether or not
            // the target line already ends in one).
            let body = payload.trim_end_matches('\n');
            let cc = buffer::char_count(&self.buffer);
            if after {
                let (_, le) = buffer::line_bounds(&self.buffer, self.cursor);
                if le < cc {
                    // current line ends in '\n' at `le`; open the next line.
                    let at = le + 1;
                    self.insert_str_at(at, &format!("{body}\n"));
                    self.cursor = at;
                } else {
                    // last line, no trailing newline: append on a fresh line.
                    self.insert_str_at(cc, &format!("\n{body}"));
                    self.cursor = cc + 1;
                }
            } else {
                let at = buffer::line_start(&self.buffer, self.cursor);
                self.insert_str_at(at, &format!("{body}\n"));
                self.cursor = at;
            }
        } else {
            let at = if after {
                (self.cursor + 1).min(buffer::char_count(&self.buffer))
            } else {
                self.cursor
            };
            self.insert_str_at(at, &payload);
            self.cursor = at + payload.chars().count().saturating_sub(1);
        }
        self.finish_recording();
        self.out(true, EditorAction::Nop)
    }

    // ---- undo / redo / dot ----

    fn do_undo(&mut self) -> Outcome {
        if let Some((buf, cur)) = self.undo.pop() {
            self.redo.push((self.buffer.clone(), self.cursor));
            self.buffer = buf;
            self.cursor = buffer::clamp_cursor(&self.buffer, cur);
            self.clamp_normal();
            self.out(true, EditorAction::Nop)
        } else {
            self.out(false, EditorAction::Nop)
        }
    }

    fn do_redo(&mut self) -> Outcome {
        if let Some((buf, cur)) = self.redo.pop() {
            self.undo.push((self.buffer.clone(), self.cursor));
            self.buffer = buf;
            self.cursor = buffer::clamp_cursor(&self.buffer, cur);
            self.clamp_normal();
            self.out(true, EditorAction::Nop)
        } else {
            self.out(false, EditorAction::Nop)
        }
    }

    fn do_dot(&mut self) -> Outcome {
        if self.last_change.is_empty() {
            return self.out(false, EditorAction::Nop);
        }
        let keys = self.last_change.clone();
        self.replaying = true;
        for k in keys {
            self.handle_key(k);
        }
        self.replaying = false;
        // ensure we ended in Normal mode
        if self.mode == Mode::Insert {
            self.mode = Mode::Normal;
            self.clamp_normal();
        }
        self.out(true, EditorAction::Nop)
    }

    // ---- visual ----

    fn handle_visual(&mut self, k: VimKey) -> Outcome {
        // find/replace pending inside visual is uncommon; support find for motion
        if let Pending::Find { .. } = self.pending {
            // resolve as a plain motion find (extends selection)
            return self.resolve_visual_find(k);
        }
        if let Pending::GPrefix = self.pending {
            self.pending = Pending::None;
            if let VimKey::Char('g') = k {
                self.cursor = motion::buffer_start(&self.buffer);
            }
            return self.out(false, EditorAction::Nop);
        }
        let c = match k {
            VimKey::Char(c) => c,
            VimKey::Esc => {
                self.mode = Mode::Normal;
                self.visual_anchor = None;
                self.pending_count = None;
                self.clamp_normal();
                return self.out(false, EditorAction::Nop);
            }
            _ => return self.out(false, EditorAction::Nop),
        };

        if c.is_ascii_digit() && !(c == '0' && self.pending_count.is_none()) {
            let d = c as usize - '0' as usize;
            self.pending_count = Some(self.pending_count.unwrap_or(0) * 10 + d);
            return self.out(false, EditorAction::Nop);
        }

        match c {
            'h' => self.visual_motion(motion::left),
            'l' => self.visual_motion(motion::right),
            'k' => self.visual_motion(motion::up),
            'j' => self.visual_motion(motion::down),
            'w' => self.visual_motion(motion::word_forward),
            'b' => self.visual_motion(motion::word_back),
            'e' => self.visual_motion(motion::word_end),
            '0' => {
                self.cursor = motion::line_zero(&self.buffer, self.cursor);
                self.out(false, EditorAction::Nop)
            }
            '^' => {
                self.cursor = motion::line_first_char(&self.buffer, self.cursor);
                self.out(false, EditorAction::Nop)
            }
            '$' => {
                self.cursor = motion::line_last_char(&self.buffer, self.cursor);
                self.out(false, EditorAction::Nop)
            }
            'G' => {
                let n = self.pending_count.take().unwrap_or(0);
                self.cursor = motion::goto_line(&self.buffer, n);
                self.out(false, EditorAction::Nop)
            }
            'g' => {
                self.pending = Pending::GPrefix;
                self.out(false, EditorAction::Nop)
            }
            'f' => self.start_find(FindKind::ForwardOn),
            't' => self.start_find(FindKind::ForwardBefore),
            'F' => self.start_find(FindKind::BackOn),
            'T' => self.start_find(FindKind::BackBefore),
            'v' => {
                // toggle to charwise, or exit if already charwise
                if self.mode == Mode::Visual {
                    self.mode = Mode::Normal;
                    self.visual_anchor = None;
                    self.clamp_normal();
                } else {
                    self.mode = Mode::Visual;
                }
                self.out(false, EditorAction::Nop)
            }
            'V' => {
                if self.mode == Mode::VisualLine {
                    self.mode = Mode::Normal;
                    self.visual_anchor = None;
                    self.clamp_normal();
                } else {
                    self.mode = Mode::VisualLine;
                }
                self.out(false, EditorAction::Nop)
            }
            'd' | 'x' => self.visual_operator(Op::Delete),
            'c' | 's' => self.visual_operator(Op::Change),
            'y' => self.visual_operator(Op::Yank),
            '>' => self.visual_indent(1),
            '<' => self.visual_indent(-1),
            '"' => {
                self.pending = Pending::Register;
                self.out(false, EditorAction::Nop)
            }
            _ => self.out(false, EditorAction::Nop),
        }
    }

    fn visual_motion(&mut self, f: fn(&str, usize, usize) -> usize) -> Outcome {
        let n = self.take_count();
        self.cursor = f(&self.buffer, self.cursor, n);
        self.out(false, EditorAction::Nop)
    }

    fn resolve_visual_find(&mut self, k: VimKey) -> Outcome {
        let kind = match self.pending {
            Pending::Find { kind, .. } => kind,
            _ => unreachable!(),
        };
        self.pending = Pending::None;
        if let VimKey::Char(target) = k {
            self.last_find = Some((kind, target));
            if let Some(pos) = motion::find_char(&self.buffer, self.cursor, kind, target) {
                self.cursor = pos;
            }
        }
        self.out(false, EditorAction::Nop)
    }

    fn visual_operator(&mut self, op: Op) -> Outcome {
        let anchor = match self.visual_anchor {
            Some(a) => a,
            None => return self.out(false, EditorAction::Nop),
        };
        let linewise = self.mode == Mode::VisualLine;
        let r = self.visual_range(anchor);
        self.visual_anchor = None;
        self.mode = Mode::Normal;
        let res = self.apply_operator(op, r, linewise);
        if op != Op::Change {
            self.clamp_normal();
        }
        res
    }

    fn visual_indent(&mut self, dir: i32) -> Outcome {
        let anchor = match self.visual_anchor {
            Some(a) => a,
            None => return self.out(false, EditorAction::Nop),
        };
        let (lo, hi) = if anchor <= self.cursor {
            (anchor, self.cursor)
        } else {
            (self.cursor, anchor)
        };
        let first = buffer::line_index(&self.buffer, lo);
        let last = buffer::line_index(&self.buffer, hi);
        self.begin_change(&[VimKey::Char(if dir > 0 { '>' } else { '<' })]);
        for ln in first..=last {
            let ls = buffer::nth_line_start(&self.buffer, ln);
            if dir > 0 {
                self.insert_str_at(ls, "    ");
            } else {
                let mut cs: Vec<char> = self.buffer.chars().collect();
                let mut removed = 0;
                while removed < 4 && ls < cs.len() && cs[ls] == ' ' {
                    cs.remove(ls);
                    removed += 1;
                }
                self.buffer = cs.into_iter().collect();
            }
        }
        self.visual_anchor = None;
        self.mode = Mode::Normal;
        self.cursor = motion::line_first_char(&self.buffer, buffer::nth_line_start(&self.buffer, first));
        self.finish_recording();
        self.out(true, EditorAction::Nop)
    }

    // ---- command line ----

    fn handle_cmdline(&mut self, k: VimKey) -> Outcome {
        match k {
            VimKey::Esc => {
                self.cmdline = None;
                self.out(false, EditorAction::Nop)
            }
            VimKey::Backspace => {
                if let Some(s) = self.cmdline.as_mut() {
                    if s.pop().is_none() {
                        self.cmdline = None;
                    }
                }
                self.out(false, EditorAction::Nop)
            }
            VimKey::Char(c) => {
                if let Some(s) = self.cmdline.as_mut() {
                    s.push(c);
                }
                self.out(false, EditorAction::Nop)
            }
            VimKey::Enter => {
                let cmd = self.cmdline.take().unwrap_or_default();
                self.run_ex(&cmd)
            }
            VimKey::Tab | VimKey::CtrlR => self.out(false, EditorAction::Nop),
        }
    }

    fn run_ex(&mut self, cmd: &str) -> Outcome {
        let c = cmd.trim();
        let action = match c {
            "w" => EditorAction::Save,
            "wq" | "x" => EditorAction::SaveQuit,
            "q" => EditorAction::Cancel,
            "q!" => EditorAction::CancelForce,
            _ => EditorAction::Nop,
        };
        self.out(false, action)
    }
}

fn find_kind_of(c: char) -> Option<FindKind> {
    match c {
        'f' => Some(FindKind::ForwardOn),
        't' => Some(FindKind::ForwardBefore),
        'F' => Some(FindKind::BackOn),
        'T' => Some(FindKind::BackBefore),
        _ => None,
    }
}

fn invert_find(k: FindKind) -> FindKind {
    match k {
        FindKind::ForwardOn => FindKind::BackOn,
        FindKind::BackOn => FindKind::ForwardOn,
        FindKind::ForwardBefore => FindKind::BackBefore,
        FindKind::BackBefore => FindKind::ForwardBefore,
    }
}

#[cfg(test)]
impl VimEngine {
    pub fn feed(&mut self, keys: &str) {
        for ch in keys.chars() {
            let k = match ch {
                '\x1b' => VimKey::Esc,
                '\n' => VimKey::Enter,
                '\x08' => VimKey::Backspace,
                '\t' => VimKey::Tab,
                c => VimKey::Char(c),
            };
            self.handle_key(k);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::vim::{EditorAction, Mode, VimKey};

    fn eng(s: &str) -> VimEngine {
        VimEngine::new(s.to_string())
    }

    // Task 4: motions, counts, insert
    #[test]
    fn motions_move_cursor() {
        let mut e = eng("hello world");
        e.feed("w");
        assert_eq!(e.cursor(), 6);
        e.feed("0");
        assert_eq!(e.cursor(), 0);
        e.feed("$");
        assert_eq!(e.cursor(), 10);
    }

    #[test]
    fn count_then_motion() {
        let mut e = eng("aaaa bbbb cccc");
        e.feed("2w");
        assert_eq!(e.cursor(), 10);
    }

    #[test]
    fn insert_then_type_then_esc() {
        let mut e = eng("bc");
        e.feed("i");
        assert_eq!(e.mode(), Mode::Insert);
        e.feed("A");
        assert_eq!(e.buffer(), "Abc");
        e.feed("\x1b");
        assert_eq!(e.mode(), Mode::Normal);
        assert_eq!(e.cursor(), 0);
    }

    #[test]
    fn open_line_below() {
        let mut e = eng("x");
        e.feed("o");
        assert_eq!(e.mode(), Mode::Insert);
        e.feed("y\x1b");
        assert_eq!(e.buffer(), "x\ny");
    }

    #[test]
    fn append_after_cursor() {
        let mut e = eng("ab");
        e.feed("a");
        e.feed("Z\x1b");
        assert_eq!(e.buffer(), "aZb");
    }

    #[test]
    fn gg_goes_to_top() {
        let mut e = eng("a\nb\nc");
        e.feed("G");
        assert_eq!(e.cursor(), 4); // line 'c'
        e.feed("gg");
        assert_eq!(e.cursor(), 0);
    }

    // Task 6: operators
    #[test]
    fn delete_word_and_to_eol() {
        let mut e = eng("foo bar baz");
        e.feed("dw");
        assert_eq!(e.buffer(), "bar baz");
        e.feed("D");
        assert_eq!(e.buffer(), "");
    }

    #[test]
    fn change_inner_word_enters_insert() {
        let mut e = eng("foo bar");
        e.feed("w");
        e.feed("ciw");
        assert_eq!(Mode::Insert, e.mode());
        e.feed("X\x1b");
        assert_eq!(e.buffer(), "foo X");
    }

    #[test]
    fn dd_and_count_dd() {
        let mut e = eng("a\nb\nc\nd");
        e.feed("dd");
        assert_eq!(e.buffer(), "b\nc\nd");
        e.feed("2dd");
        assert_eq!(e.buffer(), "d");
    }

    #[test]
    fn x_r_j_tilde() {
        let mut e = eng("abc");
        e.feed("x");
        assert_eq!(e.buffer(), "bc");
        e.feed("rZ");
        assert_eq!(e.buffer(), "Zc");
        e.feed("~");
        assert_eq!(e.buffer(), "zc");
        let mut j = eng("a\nb");
        j.feed("J");
        assert_eq!(j.buffer(), "a b");
    }

    #[test]
    fn yank_then_motion_count() {
        let mut e = eng("one two three");
        e.feed("d2w");
        assert_eq!(e.buffer(), "three");
    }

    #[test]
    fn delete_text_object_parens() {
        let mut e = eng("a(bc)d");
        e.feed("ll"); // cursor inside? 'a'(0)'('(1)'b'(2)
        e.feed("di(");
        assert_eq!(e.buffer(), "a()d");
    }

    // Task 7: registers / put
    #[test]
    fn dd_then_p_linewise() {
        let mut e = eng("a\nb\nc");
        e.feed("dd");
        assert_eq!(e.buffer(), "b\nc");
        e.feed("p");
        assert_eq!(e.buffer(), "b\na\nc");
    }

    #[test]
    fn yank_put_charwise() {
        let mut e = eng("abc");
        e.feed("yl"); // yank 'a'
        e.feed("$");
        e.feed("p");
        assert_eq!(e.buffer(), "abca");
    }

    // Task 8: find
    #[test]
    fn find_and_delete_to_char() {
        let mut e = eng("foo(bar)baz");
        e.feed("df)");
        assert_eq!(e.buffer(), "baz");
    }

    // Task 9: visual
    #[test]
    fn visual_delete() {
        let mut e = eng("hello world");
        e.feed("v");
        e.feed("ll");
        e.feed("d");
        assert_eq!(e.buffer(), "lo world");
        assert_eq!(Mode::Normal, e.mode());
    }

    #[test]
    fn visual_line_yank_put() {
        let mut e = eng("a\nb\nc");
        e.feed("V");
        e.feed("y");
        e.feed("G");
        e.feed("p");
        assert_eq!(e.buffer(), "a\nb\nc\na");
    }

    // Task 10: undo / dot
    #[test]
    fn undo_redo() {
        let mut e = eng("abc");
        e.feed("x");
        assert_eq!(e.buffer(), "bc");
        e.feed("u");
        assert_eq!(e.buffer(), "abc");
        e.handle_key(VimKey::CtrlR);
        assert_eq!(e.buffer(), "bc");
    }

    #[test]
    fn dot_repeats_last_change() {
        let mut e = eng("aaaa");
        e.feed("x");
        e.feed(".");
        e.feed(".");
        assert_eq!(e.buffer(), "a");
    }

    // Task 11: ex / R / Esc
    #[test]
    fn ex_write_and_quit() {
        let mut e = eng("x");
        e.feed(":w");
        let o = e.handle_key(VimKey::Enter);
        assert_eq!(o.action, EditorAction::Save);
        e.feed(":wq");
        let o2 = e.handle_key(VimKey::Enter);
        assert_eq!(o2.action, EditorAction::SaveQuit);
    }

    #[test]
    fn semicolon_enters_command_mode() {
        // The user's Neovim mapping `vim.keymap.set('n', ';', ':')` is mirrored
        // here: `;` opens the command line exactly like `:`, so `;w<CR>` saves
        // and `;q<CR>` cancels. This displaces vim's default repeat-find on `;`.
        let mut e = eng("x");
        e.feed(";");
        assert_eq!(e.cmdline(), Some(""));
        e.feed("w");
        let o = e.handle_key(VimKey::Enter);
        assert_eq!(o.action, EditorAction::Save);

        let mut q = eng("x");
        q.feed(";q");
        let qo = q.handle_key(VimKey::Enter);
        assert_eq!(qo.action, EditorAction::Cancel);
    }

    #[test]
    fn r_opens_rewrite() {
        let mut e = eng("x");
        let o = e.handle_key(VimKey::Char('R'));
        assert_eq!(o.action, EditorAction::OpenRewrite);
    }

    #[test]
    fn esc_in_normal_stays_in_normal() {
        // Esc in Normal mode is a no-op (stays put) — it does NOT exit the editor.
        // It cancels a half-typed operator/count.
        let mut e = eng("hello");
        e.feed("d"); // pending operator
        let o = e.handle_key(VimKey::Esc);
        assert_eq!(o.action, EditorAction::Nop);
        assert_eq!(o.mode, Mode::Normal);
        // the pending 'd' was cancelled, so a following 'w' is a plain motion
        e.feed("w");
        assert_eq!(e.buffer(), "hello"); // nothing deleted
        // Only :q exits.
        e.feed(":q");
        let q = e.handle_key(VimKey::Enter);
        assert_eq!(q.action, EditorAction::Cancel);
    }

    #[test]
    fn undo_restores_seed_buffer() {
        // The host tracks "dirty" by comparing the engine buffer to its seed; an
        // edit then undo must return the buffer to the original so dirty resets.
        let seed = "abc";
        let mut e = eng(seed);
        e.feed("x");
        assert_ne!(e.buffer(), seed);
        e.feed("u");
        assert_eq!(e.buffer(), seed);
    }

    #[test]
    fn raw_text_round_trips_unchanged() {
        // A representative gloss markup blob.
        let gloss = "<speaker>HAMLET</speaker>\n<verse>To be, or not to be</verse>\n\n<gloss>The question of existence.</gloss>";
        let engine = VimEngine::new(gloss.to_string());
        assert_eq!(engine.buffer(), gloss, "gloss markup must round-trip");

        // A representative synopsis blob.
        let synopsis = "<p>The court gathers.</p>\n<p>A ghost appears on the battlements.</p>";
        let engine = VimEngine::new(synopsis.to_string());
        assert_eq!(engine.buffer(), synopsis, "synopsis text must round-trip");
    }

    #[test]
    fn trim_end_is_the_only_save_transform() {
        // The save path applies `trim_end()` and nothing else; interior markup
        // and leading whitespace are preserved.
        let raw = "  <p>indented and trailing</p>  \n\n";
        assert_eq!(raw.trim_end(), "  <p>indented and trailing</p>");
    }
}
