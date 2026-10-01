//! Vim mode, when the keymap profile is `vim`: Normal, Insert, Visual and Visual-line.
//! `update` gives it each editor key after the keymap has had its chance at Ctrl, Alt
//! and function-key chords, so the palette, running a statement and quitting still work
//! in every mode. In Insert mode it lets the keys through to the plain editor.

use std::ops::Range;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::action::Action;
use crate::model::Model;
use crate::widgets::text_input::TextInput;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Mode {
    #[default]
    Normal,
    Insert,
    Visual,
    VisualLine,
}

impl Mode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "NORMAL",
            Self::Insert => "INSERT",
            Self::Visual => "VISUAL",
            Self::VisualLine => "VISUAL LINE",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct VimState {
    pub mode: Mode,
    /// The command typed so far: a count, an operator, a `g`.
    pub pending: String,
    /// The keys `pending` came from, which become the last change if it is one.
    pending_keys: Vec<KeyEvent>,
    register: Option<Register>,
    /// The keys of the last change, which `.` plays again.
    last_change: Vec<KeyEvent>,
    /// The change being made, while it goes on into Insert mode.
    recording: Option<Vec<KeyEvent>>,
    /// The undo depth when the change began: its edits end up as one undo step.
    change_depth: Option<usize>,
    /// Where Visual mode started.
    visual_anchor: usize,
    /// `:` or `/` being typed on the status line.
    pub prompt: Option<Prompt>,
    last_search: Option<(String, bool)>,
    replaying: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Prompt {
    pub kind: char,
    pub input: TextInput,
}

#[derive(Clone, Debug, PartialEq)]
struct Register {
    text: String,
    linewise: bool,
}

/// What `update` does with a key after Vim has seen it.
pub enum Outcome {
    /// Not Vim's: the plain editor takes it (typing in Insert mode).
    Pass,
    Done,
    /// Done, and then these -- `:w` saves, `:q` closes.
    Then(Vec<Action>),
}

pub fn active(model: &Model) -> bool {
    model.keymap.name == "vim"
}

/// A block cursor everywhere but Insert mode, as Vim draws it.
pub fn block_cursor(model: &Model) -> bool {
    active(model) && model.vim.mode != Mode::Insert && model.vim.prompt.is_none()
}

/// What Visual-line mode has selected: whole lines, whatever column the cursor is in.
pub fn display_selection(model: &Model) -> Option<Range<usize>> {
    if !active(model) || model.vim.mode != Mode::VisualLine {
        return None;
    }
    let chars: Vec<char> = model.active_document().text().chars().collect();
    let cursor = model.active_document().cursor();
    let (first, last) = ordered(model.vim.visual_anchor, cursor);
    Some(line_start(&chars, first)..line_end(&chars, last))
}

pub fn handle_key(model: &mut Model, key: KeyEvent) -> Outcome {
    if model.vim.prompt.is_some() {
        return prompt_key(model, key);
    }
    if model.vim.mode == Mode::Insert {
        if key.code == KeyCode::Esc {
            model.editor.completion_open = false;
            leave_insert(model);
            return Outcome::Done;
        }
        if let Some(recording) = &mut model.vim.recording
            && !model.vim.replaying
        {
            recording.push(key);
        }
        return Outcome::Pass;
    }
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    if ctrl {
        return match key.code {
            KeyCode::Char('r') => {
                let count = take_count(model);
                history(model, count, false);
                Outcome::Done
            }
            // Ctrl chords the keymap did not take are not Vim's either -- and Normal mode
            // does not hand them to the plain editor, where Ctrl+Backspace would delete.
            _ => Outcome::Done,
        };
    }
    match key.code {
        KeyCode::Esc => {
            model.vim.pending.clear();
            model.vim.pending_keys.clear();
            if matches!(model.vim.mode, Mode::Visual | Mode::VisualLine) {
                leave_visual(model);
            }
            Outcome::Done
        }
        KeyCode::Char(ch) => {
            model.vim.pending.push(ch);
            model.vim.pending_keys.push(key);
            command(model)
        }
        // The arrows and friends move as in any editor; Normal mode stays on a char.
        KeyCode::Left
        | KeyCode::Right
        | KeyCode::Up
        | KeyCode::Down
        | KeyCode::Home
        | KeyCode::End
        | KeyCode::PageUp
        | KeyCode::PageDown => {
            model.vim.pending.clear();
            model.vim.pending_keys.clear();
            Outcome::Pass
        }
        // Nothing in Normal mode types.
        _ => Outcome::Done,
    }
}

/// After the plain editor moved the cursor in Normal mode: back onto a character.
pub fn after_pass(model: &mut Model) {
    if active(model) && model.vim.mode == Mode::Normal {
        clamp_normal(model);
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Motion {
    Left,
    Right,
    Down,
    Up,
    WordForward,
    WordBack,
    WordEnd,
    LineStart,
    FirstNonBlank,
    LineEnd,
    FileStart,
    FileEnd,
}

enum Parsed {
    Incomplete,
    Invalid,
    Move(usize, Motion),
    Operate(char, usize, Option<Motion>),
    Simple(usize, char),
}

fn motion_of(ch: char) -> Option<Motion> {
    Some(match ch {
        'h' => Motion::Left,
        'l' | ' ' => Motion::Right,
        'j' => Motion::Down,
        'k' => Motion::Up,
        'w' => Motion::WordForward,
        'b' => Motion::WordBack,
        'e' => Motion::WordEnd,
        '0' => Motion::LineStart,
        '^' => Motion::FirstNonBlank,
        '$' => Motion::LineEnd,
        'G' => Motion::FileEnd,
        _ => return None,
    })
}

/// A count, and what is left after it. `0` alone is a motion, not a count.
fn split_count(text: &str) -> (Option<usize>, &str) {
    let digits = text
        .char_indices()
        .take_while(|(at, ch)| ch.is_ascii_digit() && !(*at == 0 && *ch == '0'))
        .count();
    let count = text[..digits].parse().ok();
    (count, &text[digits..])
}

fn parse(text: &str, visual: bool) -> Parsed {
    let (count, rest) = split_count(text);
    let count_or_one = count.unwrap_or(1);
    let mut chars = rest.chars();
    let Some(first) = chars.next() else {
        return Parsed::Incomplete;
    };
    let after = chars.as_str();
    match first {
        // The selection is what a Visual-mode operator acts on: no motion to wait for.
        'd' | 'c' | 'y' if visual && after.is_empty() => Parsed::Operate(first, count_or_one, None),
        'd' | 'c' | 'y' => {
            if after.is_empty() {
                return Parsed::Incomplete;
            }
            let (inner, rest) = split_count(after);
            let total = count_or_one * inner.unwrap_or(1);
            match rest {
                "" => Parsed::Incomplete,
                "g" => Parsed::Incomplete,
                "gg" => {
                    Parsed::Operate(first, count.or(inner).unwrap_or(0), Some(Motion::FileStart))
                }
                same if same.len() == 1 && same.starts_with(first) => {
                    Parsed::Operate(first, total, None)
                }
                motion if motion.chars().count() == 1 => {
                    match motion.chars().next().and_then(motion_of) {
                        Some(Motion::FileEnd) => Parsed::Operate(
                            first,
                            count.or(inner).unwrap_or(0),
                            Some(Motion::FileEnd),
                        ),
                        Some(motion) => Parsed::Operate(first, total, Some(motion)),
                        None => Parsed::Invalid,
                    }
                }
                _ => Parsed::Invalid,
            }
        }
        'g' => match after {
            "" => Parsed::Incomplete,
            "g" => Parsed::Move(count.unwrap_or(0), Motion::FileStart),
            _ => Parsed::Invalid,
        },
        'G' if after.is_empty() => Parsed::Move(count.unwrap_or(0), Motion::FileEnd),
        ch if after.is_empty() => match motion_of(ch) {
            Some(motion) => Parsed::Move(count_or_one, motion),
            None if "xpPuiaIAoOvV.nN/:DCYX".contains(ch) => Parsed::Simple(count_or_one, ch),
            None => Parsed::Invalid,
        },
        _ => Parsed::Invalid,
    }
}

fn take_count(model: &mut Model) -> usize {
    let (count, _) = split_count(&model.vim.pending);
    model.vim.pending.clear();
    model.vim.pending_keys.clear();
    count.unwrap_or(1)
}

fn command(model: &mut Model) -> Outcome {
    let visual = matches!(model.vim.mode, Mode::Visual | Mode::VisualLine);
    let parsed = parse(&model.vim.pending, visual);
    if matches!(parsed, Parsed::Incomplete) {
        return Outcome::Done;
    }
    let keys = std::mem::take(&mut model.vim.pending_keys);
    model.vim.pending.clear();
    let outcome = match parsed {
        Parsed::Incomplete | Parsed::Invalid => Outcome::Done,
        Parsed::Move(count, motion) => {
            move_cursor(model, motion, count);
            Outcome::Done
        }
        // In Visual mode an operator acts on the selection at once.
        Parsed::Operate(op, _, _) if visual => {
            visual_operate(model, op);
            Outcome::Done
        }
        Parsed::Operate(op, count, motion) => {
            let changes = op != 'y';
            begin_change(model, changes, &keys);
            operate(model, op, count, motion);
            finish_change(model, changes, op == 'c');
            Outcome::Done
        }
        Parsed::Simple(_, ch) if visual && "xdDcCyY".contains(ch) => {
            let op = match ch {
                'x' | 'D' | 'X' => 'd',
                'C' => 'c',
                'Y' => 'y',
                other => other,
            };
            visual_operate(model, op);
            Outcome::Done
        }
        Parsed::Simple(count, ch) => simple(model, count, ch, &keys),
    };
    if model.vim.mode == Mode::Normal {
        clamp_normal(model);
    }
    if matches!(model.vim.mode, Mode::Visual | Mode::VisualLine) {
        let anchor = model.vim.visual_anchor;
        model.active_document_mut().anchor = Some(anchor);
    }
    crate::screens::editor::follow_cursor(model);
    outcome
}

fn simple(model: &mut Model, count: usize, ch: char, keys: &[KeyEvent]) -> Outcome {
    match ch {
        'x' | 'X' | 'D' | 'C' | 'Y' => {
            let (op, motion) = match ch {
                'x' => ('d', Motion::Right),
                'X' => ('d', Motion::Left),
                'D' => ('d', Motion::LineEnd),
                'C' => ('c', Motion::LineEnd),
                _ => ('y', Motion::Down),
            };
            if ch == 'Y' {
                operate(model, 'y', count, None);
                return Outcome::Done;
            }
            begin_change(model, true, keys);
            operate(model, op, count, Some(motion));
            finish_change(model, true, op == 'c');
        }
        'p' | 'P' => {
            begin_change(model, true, keys);
            for _ in 0..count {
                put(model, ch == 'p');
            }
            finish_change(model, true, false);
        }
        'u' => history(model, count, true),
        'i' | 'a' | 'I' | 'A' | 'o' | 'O' => {
            begin_change(model, true, keys);
            enter_insert(model, ch);
        }
        'v' | 'V' => {
            let mode = if ch == 'v' {
                Mode::Visual
            } else {
                Mode::VisualLine
            };
            if model.vim.mode == mode {
                leave_visual(model);
            } else {
                if model.vim.mode == Mode::Normal {
                    model.vim.visual_anchor = model.active_document().cursor();
                }
                model.vim.mode = mode;
            }
        }
        '.' => {
            let keys = model.vim.last_change.clone();
            for _ in 0..count {
                replay(model, &keys);
            }
        }
        'n' | 'N' => {
            if let Some((pattern, forward)) = model.vim.last_search.clone() {
                for _ in 0..count {
                    search(model, &pattern, forward == (ch == 'n'));
                }
            }
        }
        '/' | ':' => {
            model.vim.prompt = Some(Prompt {
                kind: ch,
                input: TextInput::default(),
            });
        }
        _ => {}
    }
    Outcome::Done
}

/// `u` and Ctrl+R, leaving the cursor where the change began, as Vim does -- not at the
/// end of the text it put back.
fn history(model: &mut Model, count: usize, undo: bool) {
    let before: Vec<char> = model.active_document().text().chars().collect();
    for _ in 0..count {
        if undo {
            crate::screens::editor::undo(model);
        } else {
            crate::screens::editor::redo(model);
        }
    }
    let after: Vec<char> = model.active_document().text().chars().collect();
    let changed = before
        .iter()
        .zip(&after)
        .take_while(|(a, b)| a == b)
        .count();
    if before != after {
        set_cursor(model, changed);
    }
    clamp_normal(model);
}

/// Starts a change: its keys are what `.` replays, and its edits one undo step.
fn begin_change(model: &mut Model, changes: bool, keys: &[KeyEvent]) {
    if !changes {
        return;
    }
    crate::screens::editor::end_typing(model);
    let depth = model.active_document().sql.undo_depth();
    model.vim.change_depth = Some(depth);
    if !model.vim.replaying {
        model.vim.recording = Some(keys.to_vec());
    }
}

/// Ends a change that stayed in Normal mode; one that went on into Insert ends at Esc.
fn finish_change(model: &mut Model, changes: bool, into_insert: bool) {
    if !changes || into_insert {
        return;
    }
    if let Some(depth) = model.vim.change_depth.take() {
        model.active_document_mut().sql.merge_undo_since(depth);
    }
    if let Some(keys) = model.vim.recording.take() {
        model.vim.last_change = keys;
    }
    crate::screens::editor::refresh_intelligence(model, false);
}

fn leave_insert(model: &mut Model) {
    crate::screens::editor::end_typing(model);
    if let Some(depth) = model.vim.change_depth.take() {
        model.active_document_mut().sql.merge_undo_since(depth);
    }
    if let Some(mut keys) = model.vim.recording.take() {
        keys.push(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        model.vim.last_change = keys;
    }
    model.vim.mode = Mode::Normal;
    // Vim steps back onto the last character typed.
    let doc = model.active_document();
    let chars: Vec<char> = doc.text().chars().collect();
    let cursor = doc.cursor();
    if cursor > line_start(&chars, cursor) {
        set_cursor(model, cursor - 1);
    }
    clamp_normal(model);
}

fn leave_visual(model: &mut Model) {
    model.vim.mode = Mode::Normal;
    model.active_document_mut().anchor = None;
}

fn enter_insert(model: &mut Model, how: char) {
    let doc = model.active_document();
    let chars: Vec<char> = doc.text().chars().collect();
    let cursor = doc.cursor();
    let (start, end) = (line_start(&chars, cursor), line_end(&chars, cursor));
    let indent: String = chars[start..end]
        .iter()
        .take_while(|ch| ch.is_whitespace())
        .collect();
    match how {
        'a' if cursor < end => set_cursor(model, cursor + 1),
        'I' => set_cursor(model, first_non_blank(&chars, start)),
        'A' => set_cursor(model, end),
        'o' => {
            replace(model, end..end, &format!("\n{indent}"));
            set_cursor(model, end + 1 + indent.chars().count());
        }
        'O' => {
            replace(model, start..start, &format!("{indent}\n"));
            set_cursor(model, start + indent.chars().count());
        }
        _ => {}
    }
    model.active_document_mut().anchor = None;
    model.vim.mode = Mode::Insert;
}

fn move_cursor(model: &mut Model, motion: Motion, count: usize) {
    let chars: Vec<char> = model.active_document().text().chars().collect();
    let cursor = model.active_document().cursor();
    let (target, _, _) = target(&chars, cursor, motion, count);
    let target = if model.vim.mode == Mode::Normal && motion == Motion::LineEnd {
        // `$` rests on the last character, not past it.
        target.saturating_sub(1).max(line_start(&chars, target))
    } else {
        target
    };
    set_cursor(model, target);
}

/// Where `motion` takes the cursor, and how an operator reads the span to it: whole
/// lines, and whether the character it lands on is part of it.
fn target(chars: &[char], cursor: usize, motion: Motion, count: usize) -> (usize, bool, bool) {
    let len = chars.len();
    let start = line_start(chars, cursor);
    let end = line_end(chars, cursor);
    match motion {
        Motion::Left => (cursor.saturating_sub(count).max(start), false, false),
        Motion::Right => ((cursor + count).min(end), false, false),
        Motion::Down | Motion::Up => {
            let column = cursor - start;
            let mut line_begin = start;
            for _ in 0..count {
                if motion == Motion::Down {
                    let here_end = line_end(chars, line_begin);
                    if here_end >= len {
                        break;
                    }
                    line_begin = here_end + 1;
                } else {
                    if line_begin == 0 {
                        break;
                    }
                    line_begin = line_start(chars, line_begin - 1);
                }
            }
            let line_stop = line_end(chars, line_begin);
            ((line_begin + column).min(line_stop), true, false)
        }
        Motion::WordForward => {
            let mut at = cursor;
            for _ in 0..count {
                at = next_word_start(chars, at);
            }
            (at, false, false)
        }
        Motion::WordBack => {
            let mut at = cursor;
            for _ in 0..count {
                at = previous_word_start(chars, at);
            }
            (at, false, false)
        }
        Motion::WordEnd => {
            let mut at = cursor;
            for _ in 0..count {
                at = word_end(chars, at);
            }
            (at, false, true)
        }
        Motion::LineStart => (start, false, false),
        Motion::FirstNonBlank => (first_non_blank(chars, start), false, false),
        Motion::LineEnd => {
            let mut stop = end;
            for _ in 1..count {
                if stop >= len {
                    break;
                }
                stop = line_end(chars, stop + 1);
            }
            (stop, false, false)
        }
        // A count is a line number; none means the first or the last line.
        Motion::FileStart | Motion::FileEnd => {
            let lines = chars.iter().filter(|ch| **ch == '\n').count() + 1;
            let line = match (motion, count) {
                (_, n) if n > 0 => n.min(lines),
                (Motion::FileStart, _) => 1,
                _ => lines,
            };
            let begin = nth_line_start(chars, line - 1);
            (first_non_blank(chars, begin), true, false)
        }
    }
}

fn operate(model: &mut Model, op: char, count: usize, motion: Option<Motion>) {
    let chars: Vec<char> = model.active_document().text().chars().collect();
    let cursor = model.active_document().cursor();
    let (range, linewise) = match motion {
        // `dd`, `cc`, `yy`: count lines from this one.
        None => {
            let (target, _, _) = target(&chars, cursor, Motion::Down, count.saturating_sub(1));
            (line_span(&chars, cursor, target), true)
        }
        Some(motion) => {
            // `cw` changes to the end of the word, as Vim does.
            let motion = if op == 'c' && motion == Motion::WordForward {
                Motion::WordEnd
            } else {
                motion
            };
            let (to, linewise, inclusive) = target(&chars, cursor, motion, count);
            if linewise {
                (line_span(&chars, cursor, to), true)
            } else {
                let (from, to) = ordered(cursor, to);
                let to = if inclusive {
                    (to + 1).min(chars.len())
                } else {
                    to
                };
                (from..to, false)
            }
        }
    };
    apply(model, op, range, linewise);
}

fn visual_operate(model: &mut Model, op: char) {
    let chars: Vec<char> = model.active_document().text().chars().collect();
    let cursor = model.active_document().cursor();
    let anchor = model.vim.visual_anchor;
    let linewise = model.vim.mode == Mode::VisualLine;
    let range = if linewise {
        line_span(&chars, anchor, cursor)
    } else {
        let (from, to) = ordered(anchor, cursor);
        from..(to + 1).min(chars.len())
    };
    leave_visual(model);
    let changes = op != 'y';
    if changes {
        crate::screens::editor::end_typing(model);
        model.vim.change_depth = Some(model.active_document().sql.undo_depth());
    }
    apply(model, op, range, linewise);
    finish_change(model, changes, op == 'c');
}

/// `d`, `c` or `y` over `range`, which for a linewise span covers whole lines and the
/// line break that goes with them.
fn apply(model: &mut Model, op: char, range: Range<usize>, linewise: bool) {
    let chars: Vec<char> = model.active_document().text().chars().collect();
    let text: String = chars[range.clone()].iter().collect();
    let mut yanked = text.clone();
    if linewise && !yanked.ends_with('\n') {
        yanked.push('\n');
    }
    model.vim.register = Some(Register {
        text: if linewise { yanked } else { text },
        linewise,
    });
    match op {
        'y' => set_cursor(model, range.start),
        'd' => {
            replace(model, range.clone(), "");
            let chars: Vec<char> = model.active_document().text().chars().collect();
            let at = range.start.min(chars.len());
            if linewise {
                set_cursor(model, first_non_blank(&chars, line_start(&chars, at)));
            } else {
                set_cursor(model, at);
            }
        }
        'c' => {
            if linewise {
                // The lines go, and an empty one with the first's indent takes their place.
                let indent: String = chars[range.start..]
                    .iter()
                    .take_while(|ch| **ch == ' ' || **ch == '\t')
                    .collect();
                let keep_break = chars.get(range.end.saturating_sub(1)) == Some(&'\n');
                let replacement = if keep_break {
                    format!("{indent}\n")
                } else {
                    indent.clone()
                };
                replace(model, range.clone(), &replacement);
                set_cursor(model, range.start + indent.chars().count());
            } else {
                replace(model, range.clone(), "");
                set_cursor(model, range.start);
            }
            model.vim.mode = Mode::Insert;
        }
        _ => {}
    }
}

fn put(model: &mut Model, after: bool) {
    let Some(register) = model.vim.register.clone() else {
        return;
    };
    let chars: Vec<char> = model.active_document().text().chars().collect();
    let cursor = model.active_document().cursor();
    if register.linewise {
        let at = if after {
            let end = line_end(&chars, cursor);
            if end >= chars.len() {
                // The last line has no break after it to put the lines behind.
                let text = format!("\n{}", register.text.trim_end_matches('\n'));
                replace(model, end..end, &text);
                let chars: Vec<char> = model.active_document().text().chars().collect();
                set_cursor(model, first_non_blank(&chars, end + 1));
                return;
            }
            end + 1
        } else {
            line_start(&chars, cursor)
        };
        replace(model, at..at, &register.text);
        let chars: Vec<char> = model.active_document().text().chars().collect();
        set_cursor(model, first_non_blank(&chars, at));
    } else {
        let at = if after && cursor < line_end(&chars, cursor) {
            cursor + 1
        } else {
            cursor
        };
        replace(model, at..at, &register.text);
        set_cursor(model, at + register.text.chars().count().saturating_sub(1));
    }
}

/// Plays keys again for `.`, the way they came: through Vim, and through the editor
/// for what was typed in Insert mode, with completion kept out of it.
fn replay(model: &mut Model, keys: &[KeyEvent]) {
    model.vim.replaying = true;
    for key in keys {
        if matches!(handle_key(model, *key), Outcome::Pass) {
            crate::screens::editor::handle_key(model, *key);
        }
        model.editor.completion_open = false;
    }
    model.vim.replaying = false;
    if model.vim.mode == Mode::Insert {
        leave_insert(model);
    }
}

fn prompt_key(model: &mut Model, key: KeyEvent) -> Outcome {
    let Some(prompt) = &mut model.vim.prompt else {
        return Outcome::Done;
    };
    match key.code {
        KeyCode::Esc => model.vim.prompt = None,
        KeyCode::Backspace if prompt.input.is_empty() => model.vim.prompt = None,
        KeyCode::Enter => {
            let Some(prompt) = model.vim.prompt.take() else {
                return Outcome::Done;
            };
            let text = prompt.input.as_str().to_string();
            return if prompt.kind == '/' {
                if !text.is_empty() {
                    model.vim.last_search = Some((text.clone(), true));
                    search(model, &text, true);
                }
                Outcome::Done
            } else {
                ex(model, text.trim())
            };
        }
        _ => {
            prompt.input.handle_key(key);
        }
    }
    Outcome::Done
}

/// `/pattern`, `n` and `N`: the next match after the cursor, or before it, wrapping.
/// Case matters only when the pattern has a capital, as with Vim's `smartcase`.
fn search(model: &mut Model, pattern: &str, forward: bool) {
    use crate::screens::find::{FindOptions, find_all, nearest};
    let options = FindOptions {
        case_sensitive: pattern.chars().any(char::is_uppercase),
        whole_word: false,
    };
    let found = find_all(&model.active_document().text(), pattern, options);
    let cursor = model.active_document().cursor();
    let from = if forward { cursor + 1 } else { cursor };
    match nearest(&found, from, forward) {
        Some(index) => {
            set_cursor(model, found[index].start);
            crate::screens::editor::follow_cursor(model);
        }
        None => model.messages.warn(format!("Pattern not found: {pattern}")),
    }
}

/// The `:` commands: `w`, `q`, `wq`/`x`, a line number, and `s`/`%s` substitution.
fn ex(model: &mut Model, command: &str) -> Outcome {
    match command {
        "" => Outcome::Done,
        "w" => Outcome::Then(vec![Action::SaveActiveDocument]),
        "q" | "q!" => Outcome::Then(vec![Action::CloseDocument]),
        "wq" | "x" => Outcome::Then(vec![Action::SaveActiveDocument, Action::CloseDocument]),
        line if line.chars().all(|ch| ch.is_ascii_digit()) => {
            let line: usize = line.parse().unwrap_or(1);
            move_cursor(model, Motion::FileStart, line.max(1));
            Outcome::Done
        }
        _ => {
            let (whole, rest) = match command.strip_prefix('%') {
                Some(rest) => (true, rest),
                None => (false, command),
            };
            match rest.strip_prefix('s') {
                Some(spec) if !spec.is_empty() => substitute(model, spec, whole),
                _ => model
                    .messages
                    .error(format!("Not an editor command: {command}")),
            }
            Outcome::Done
        }
    }
}

/// `:s/old/new/[g]` on the cursor's line, `:%s` on every line; literal text, the first
/// match of each line unless `g`, all as one undo step.
fn substitute(model: &mut Model, spec: &str, whole: bool) {
    use crate::screens::find::{FindOptions, find_all};
    let mut parts = spec.chars();
    let Some(delimiter) = parts.next() else {
        return;
    };
    let fields: Vec<&str> = parts.as_str().splitn(3, delimiter).collect();
    let (old, new, flags) = match fields.as_slice() {
        [old, new, flags] => (*old, *new, *flags),
        [old, new] => (*old, *new, ""),
        _ => {
            model.messages.error("Usage: :s/old/new/[g]".into());
            return;
        }
    };
    if old.is_empty() {
        return;
    }
    let every = flags.contains('g');
    let chars: Vec<char> = model.active_document().text().chars().collect();
    let cursor = model.active_document().cursor();
    let span = if whole {
        0..chars.len()
    } else {
        line_start(&chars, cursor)..line_end(&chars, cursor)
    };
    let options = FindOptions {
        case_sensitive: true,
        whole_word: false,
    };
    let mut lines_seen = std::collections::HashSet::new();
    let found: Vec<Range<usize>> = find_all(&model.active_document().text(), old, options)
        .into_iter()
        .filter(|range| range.start >= span.start && range.end <= span.end)
        .filter(|range| every || lines_seen.insert(line_start(&chars, range.start)))
        .collect();
    if found.is_empty() {
        model.messages.warn(format!("Pattern not found: {old}"));
        return;
    }
    crate::screens::editor::end_typing(model);
    let depth = model.active_document().sql.undo_depth();
    for range in found.iter().rev() {
        replace(model, range.clone(), new);
    }
    model.active_document_mut().sql.merge_undo_since(depth);
    set_cursor(model, found[0].start);
    crate::screens::editor::refresh_intelligence(model, false);
    model
        .messages
        .info(format!("{} substitution(s)", found.len()));
}

fn replace(model: &mut Model, range: Range<usize>, text: &str) {
    // `x` on an empty line, `dh` at the start of one: nothing to change, and no undo
    // step or unsaved mark for it.
    if range.is_empty() && text.is_empty() {
        return;
    }
    let doc = model.active_document_mut();
    doc.anchor = None;
    let _ = doc.sql.replace_chars(range, text);
}

fn set_cursor(model: &mut Model, at: usize) {
    let doc = model.active_document_mut();
    let len = doc.sql.text().chars().count();
    let _ = doc.sql.set_cursor(at.min(len));
}

/// Normal mode sits on a character: never past the end of a line that has one.
fn clamp_normal(model: &mut Model) {
    let doc = model.active_document();
    let chars: Vec<char> = doc.text().chars().collect();
    let cursor = doc.cursor();
    let (start, end) = (line_start(&chars, cursor), line_end(&chars, cursor));
    if cursor >= end && end > start {
        set_cursor(model, end - 1);
    }
    model.active_document_mut().anchor = None;
}

fn ordered(a: usize, b: usize) -> (usize, usize) {
    (a.min(b), a.max(b))
}

/// From the start of `a`'s line to the end of `b`'s, and the line break after it -- or,
/// for the last line, the one before, so no empty line is left behind.
fn line_span(chars: &[char], a: usize, b: usize) -> Range<usize> {
    let (first, last) = ordered(a, b);
    let start = line_start(chars, first);
    let end = line_end(chars, last);
    if end < chars.len() {
        start..end + 1
    } else if start > 0 {
        start - 1..end
    } else {
        start..end
    }
}

fn line_start(chars: &[char], at: usize) -> usize {
    let mut at = at.min(chars.len());
    while at > 0 && chars[at - 1] != '\n' {
        at -= 1;
    }
    at
}

fn line_end(chars: &[char], at: usize) -> usize {
    let mut at = at.min(chars.len());
    while at < chars.len() && chars[at] != '\n' {
        at += 1;
    }
    at
}

fn nth_line_start(chars: &[char], line: usize) -> usize {
    if line == 0 {
        return 0;
    }
    chars
        .iter()
        .enumerate()
        .filter(|(_, ch)| **ch == '\n')
        .nth(line - 1)
        .map_or(chars.len(), |(at, _)| at + 1)
}

fn first_non_blank(chars: &[char], start: usize) -> usize {
    let end = line_end(chars, start);
    (start..end)
        .find(|at| !matches!(chars[*at], ' ' | '\t'))
        .unwrap_or(end)
}

/// Vim's word classes: blanks, keyword characters, and runs of anything else.
fn class(ch: char) -> u8 {
    if ch.is_whitespace() {
        0
    } else if ch.is_alphanumeric() || ch == '_' {
        1
    } else {
        2
    }
}

fn next_word_start(chars: &[char], at: usize) -> usize {
    let len = chars.len();
    let mut at = at;
    if at < len && class(chars[at]) != 0 {
        let here = class(chars[at]);
        while at < len && class(chars[at]) == here {
            at += 1;
        }
    }
    while at < len && class(chars[at]) == 0 {
        at += 1;
    }
    at
}

fn previous_word_start(chars: &[char], at: usize) -> usize {
    let mut at = at;
    while at > 0 && class(chars[at - 1]) == 0 {
        at -= 1;
    }
    if at == 0 {
        return 0;
    }
    let here = class(chars[at - 1]);
    while at > 0 && class(chars[at - 1]) == here {
        at -= 1;
    }
    at
}

fn word_end(chars: &[char], at: usize) -> usize {
    let len = chars.len();
    if len == 0 {
        return 0;
    }
    let mut at = (at + 1).min(len);
    while at < len && class(chars[at]) == 0 {
        at += 1;
    }
    if at >= len {
        return len - 1;
    }
    let here = class(chars[at]);
    while at + 1 < len && class(chars[at + 1]) == here {
        at += 1;
    }
    at
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::Mode;
    use crate::action::Action;
    use crate::model::{Focus, Model};
    use crate::update::update;

    fn vim(text: &str) -> Model {
        let mut model = Model {
            focus: Focus::Editor,
            keymap: crate::keymap::Keymap::vim_profile(),
            ..Model::default()
        };
        model.active_document_mut().sql.insert(0, text).unwrap();
        model.active_document_mut().sql.set_cursor(0).unwrap();
        model
    }

    fn keys(model: &mut Model, typed: &str) {
        for ch in typed.chars() {
            let key = match ch {
                '\u{1b}' => KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
                '\n' => KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
                '\u{12}' => KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL),
                ch => KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE),
            };
            update(model, Action::Key(key));
        }
    }

    fn text(model: &Model) -> String {
        model.active_document().text()
    }

    /// Keys the keymap leaves -- Ctrl+Backspace, Ctrl+Delete -- do not edit in Normal
    /// mode, an empty change marks nothing, and "nothing open" takes `i` like a document.
    #[test]
    fn normal_mode_edits_only_through_its_commands() {
        let mut model = vim("select name from t");
        model.active_document_mut().sql.set_cursor(10).unwrap();
        for code in [KeyCode::Backspace, KeyCode::Delete] {
            update(
                &mut model,
                Action::Key(KeyEvent::new(code, KeyModifiers::CONTROL)),
            );
        }
        assert_eq!(text(&model), "select name from t");

        let mut model = vim("a\n\nb");
        model.active_document_mut().sql.set_cursor(2).unwrap();
        let revision = model.active_document().sql.revision();
        keys(&mut model, "x");
        assert_eq!(model.active_document().sql.revision(), revision);

        let mut model = Model {
            focus: Focus::Editor,
            keymap: crate::keymap::Keymap::vim_profile(),
            ..Model::default()
        };
        model.documents = vec![crate::model::EditorDocument::placeholder()];
        model.active_document = 0;
        keys(&mut model, "iselect 1\u{1b}");
        assert_eq!(text(&model), "SELECT 1");
        assert!(!model.active_document().kind.is_placeholder());
    }

    #[test]
    fn normal_mode_moves_and_never_types() {
        let mut model = vim("select a, b\nfrom t\nwhere x");
        keys(&mut model, "wzq");
        assert_eq!(text(&model), "select a, b\nfrom t\nwhere x");
        assert_eq!(model.active_document().cursor(), 7);
        keys(&mut model, "jG$");
        assert_eq!(model.active_document().cursor(), 25);
        keys(&mut model, "gg0");
        assert_eq!(model.active_document().cursor(), 0);
    }

    #[test]
    fn operators_counts_and_one_undo_step_each() {
        let mut model = vim("one two three four\nsecond\nthird");
        keys(&mut model, "2dw");
        assert_eq!(text(&model), "three four\nsecond\nthird");
        keys(&mut model, "dd");
        assert_eq!(text(&model), "second\nthird");
        keys(&mut model, "u");
        assert_eq!(text(&model), "three four\nsecond\nthird");
        keys(&mut model, "cwfive\u{1b}");
        assert_eq!(text(&model), "five four\nsecond\nthird");
        keys(&mut model, "u");
        assert_eq!(text(&model), "three four\nsecond\nthird");
        keys(&mut model, "\u{12}");
        assert_eq!(text(&model), "five four\nsecond\nthird");
        keys(&mut model, "yyjp");
        assert_eq!(text(&model), "five four\nsecond\nfive four\nthird");
        keys(&mut model, "x.");
        assert_eq!(text(&model), "five four\nsecond\nve four\nthird");
    }

    #[test]
    fn insert_commands_and_the_dot_repeat() {
        let mut model = vim("a\nb");
        keys(&mut model, "Ax;\u{1b}");
        assert_eq!(text(&model), "ax;\nb");
        assert_eq!(model.vim.mode, Mode::Normal);
        keys(&mut model, "j.");
        assert_eq!(text(&model), "ax;\nbx;");
        keys(&mut model, "Otop\u{1b}");
        assert_eq!(text(&model), "ax;\ntop\nbx;");
        keys(&mut model, "u");
        assert_eq!(text(&model), "ax;\nbx;");
    }

    #[test]
    fn visual_modes_search_and_substitute() {
        let mut model = vim("alpha beta\ngamma alpha\nalpha");
        keys(&mut model, "vey");
        keys(&mut model, "$p");
        assert_eq!(text(&model), "alpha betaalpha\ngamma alpha\nalpha");
        keys(&mut model, "u");
        keys(&mut model, "jVd");
        assert_eq!(text(&model), "alpha beta\nalpha");
        keys(&mut model, "u");
        keys(&mut model, "gg/alpha\n");
        assert_eq!(model.active_document().cursor(), 17);
        keys(&mut model, "n");
        assert_eq!(model.active_document().cursor(), 23);
        keys(&mut model, ":%s/alpha/omega/g\n");
        assert_eq!(text(&model), "omega beta\ngamma omega\nomega");
        keys(&mut model, "u");
        assert_eq!(text(&model), "alpha beta\ngamma alpha\nalpha");
    }
}
