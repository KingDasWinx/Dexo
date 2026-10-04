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
    /// An Insert session's command and count (`3ia`), which the count repeats at Esc,
    /// and the text before it: what it inserted is the difference, repeated as text --
    /// replaying its keys without completion put in something else.
    insert: Option<(char, usize)>,
    insert_before: Option<String>,
    /// The text the last change inserted, which `.` puts in again.
    last_change_text: Option<String>,
    /// The document all of this is about. Another one becoming active starts afresh: an
    /// Insert session's undo depth or a Visual anchor means nothing in it.
    document: Option<String>,
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

/// The mode the active document is in. The state is the last document's that took a
/// key: another document starts in Normal, and its status line and cursor say so
/// before the first key -- showing the old document's INSERT, `dd` deleted a line.
pub fn mode(model: &Model) -> Mode {
    if model.vim.document.as_deref() == Some(model.active_document().id.as_str()) {
        model.vim.mode
    } else {
        Mode::Normal
    }
}

/// The keys typed toward a command in the active document, if any.
pub fn pending(model: &Model) -> &str {
    if model.vim.document.as_deref() == Some(model.active_document().id.as_str()) {
        &model.vim.pending
    } else {
        ""
    }
}

/// Whether a `:` or `/` line is open in the active document.
pub fn prompt_open(model: &Model) -> bool {
    active(model)
        && model.vim.prompt.is_some()
        && model.vim.document.as_deref() == Some(model.active_document().id.as_str())
}

/// A block cursor everywhere but Insert mode, as Vim draws it.
pub fn block_cursor(model: &Model) -> bool {
    active(model) && mode(model) != Mode::Insert && model.vim.prompt.is_none()
}

/// What Visual mode has selected, as the operators will take it: both ends included,
/// whichever way it was made -- and whole lines in Visual-line mode.
pub fn display_selection(model: &Model) -> Option<Range<usize>> {
    // The mode first: this runs every frame, and copying the text for nothing did too.
    if !active(model) || !matches!(mode(model), Mode::Visual | Mode::VisualLine) {
        return None;
    }
    let chars: Vec<char> = model.active_document().text().chars().collect();
    let cursor = model.active_document().cursor();
    let (first, last) = ordered(model.vim.visual_anchor, cursor);
    match model.vim.mode {
        Mode::VisualLine => Some(line_start(&chars, first)..line_end(&chars, last)),
        Mode::Visual => Some(first..(last + 1).min(chars.len()).max(first)),
        Mode::Normal | Mode::Insert => None,
    }
}

pub fn handle_key(model: &mut Model, key: KeyEvent) -> Outcome {
    let document = model.active_document().id.clone();
    if model.vim.document.as_deref() != Some(document.as_str()) {
        let register = model.vim.register.take();
        let last_change = std::mem::take(&mut model.vim.last_change);
        let last_search = model.vim.last_search.take();
        model.vim = VimState {
            register,
            last_change,
            last_search,
            document: Some(document),
            ..VimState::default()
        };
        model.active_document_mut().anchor = None;
    }
    if model.vim.prompt.is_some() {
        return prompt_key(model, key);
    }
    if model.vim.mode == Mode::Insert {
        if key.code == KeyCode::Esc {
            model.editor.completion_open = false;
            leave_insert(model);
            return Outcome::Done;
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
            // Ctrl with an arrow, Home or End moves by word or to the document's ends,
            // as in any editor.
            KeyCode::Left
            | KeyCode::Right
            | KeyCode::Up
            | KeyCode::Down
            | KeyCode::Home
            | KeyCode::End => {
                model.vim.pending.clear();
                model.vim.pending_keys.clear();
                Outcome::Pass
            }
            // Other Ctrl chords the keymap did not take are not Vim's either -- and
            // Normal mode does not hand them to the plain editor, where Ctrl+Backspace
            // would delete.
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

/// After the plain editor moved the cursor: back onto a character in Normal mode, and
/// in Visual mode the selection still runs from where it started -- an arrow key let
/// go of it.
pub fn after_pass(model: &mut Model) {
    if !active(model) {
        return;
    }
    match model.vim.mode {
        Mode::Normal => clamp_normal(model),
        Mode::Visual | Mode::VisualLine => {
            let anchor = model.vim.visual_anchor;
            model.active_document_mut().anchor = Some(anchor);
        }
        Mode::Insert => {}
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
    /// An operator over a text object: `diw`, `caw`.
    OperateObject(char, usize, bool),
    /// A text object picked in Visual mode: `iw`, `aw`.
    Object(bool),
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
                "" | "g" | "i" | "a" => Parsed::Incomplete,
                "iw" | "iW" => Parsed::OperateObject(first, total, false),
                "aw" | "aW" => Parsed::OperateObject(first, total, true),
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
        'i' | 'a' if visual => match after {
            "" => Parsed::Incomplete,
            "w" | "W" => Parsed::Object(first == 'a'),
            _ => Parsed::Invalid,
        },
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
            visual_operate(model, op, false);
            Outcome::Done
        }
        Parsed::Operate(op, count, motion) => {
            let changes = op != 'y';
            begin_change(model, changes, &keys);
            operate(model, op, count, motion);
            finish_change(model, changes, op == 'c');
            Outcome::Done
        }
        Parsed::OperateObject(op, count, around) => {
            let changes = op != 'y';
            begin_change(model, changes, &keys);
            let chars: Vec<char> = model.active_document().text().chars().collect();
            let range = word_object(&chars, model.active_document().cursor(), around, count);
            apply(model, op, range, false);
            finish_change(model, changes, op == 'c');
            Outcome::Done
        }
        Parsed::Object(around) => {
            let chars: Vec<char> = model.active_document().text().chars().collect();
            let range = word_object(&chars, model.active_document().cursor(), around, 1);
            if !range.is_empty() {
                model.vim.mode = Mode::Visual;
                model.vim.visual_anchor = range.start;
                set_cursor(model, range.end - 1);
            }
            Outcome::Done
        }
        // `o` goes to the other end; the insert commands are not Visual's.
        Parsed::Simple(_, 'o') if visual => {
            let cursor = model.active_document().cursor();
            let anchor = model.vim.visual_anchor;
            model.vim.visual_anchor = cursor;
            set_cursor(model, anchor);
            Outcome::Done
        }
        Parsed::Simple(_, 'i' | 'a' | 'I' | 'A' | 'O') if visual => Outcome::Done,
        // The capitals act on whole lines, whichever Visual mode it is.
        Parsed::Simple(_, ch) if visual && "xXdDcCyY".contains(ch) => {
            let op = match ch {
                'x' | 'D' | 'X' => 'd',
                'C' => 'c',
                'Y' => 'y',
                other => other,
            };
            visual_operate(model, op, ch.is_ascii_uppercase());
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
            model.vim.insert = Some((ch, count));
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
        // A count given to `.` takes the place of the change's own: `2dd` then `3.`
        // deletes three lines, not six.
        '.' => {
            let digits = &keys[..keys.len().saturating_sub(1)];
            let change = model.vim.last_change.clone();
            let replayed: Vec<KeyEvent> = if digits.is_empty() {
                change
            } else {
                // Every count of the change goes -- `d2w`'s too -- or `3.` deleted six.
                let is_count = |key: &&KeyEvent| matches!(key.code, KeyCode::Char(ch) if ch.is_ascii_digit() && ch != '0');
                let mut body = change.iter().skip_while(is_count);
                let command: Vec<KeyEvent> = body.next().into_iter().copied().collect();
                let rest: Vec<KeyEvent> = match command.first().map(|key| key.code) {
                    Some(KeyCode::Char('d' | 'c' | 'y')) => {
                        body.skip_while(is_count).copied().collect()
                    }
                    _ => body.copied().collect(),
                };
                digits.iter().copied().chain(command).chain(rest).collect()
            };
            replay(model, &replayed);
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
    let inserted = model
        .vim
        .insert_before
        .take()
        .map(|before| inserted_text(&before, &model.active_document().text()));
    // `3ia<Esc>` puts the text in three times; `3o` opens three lines with it.
    if let Some((how, count)) = model.vim.insert.take()
        && count > 1
        && let Some(text) = &inserted
    {
        for _ in 1..count {
            if matches!(how, 'o' | 'O') {
                enter_insert(model, how);
            }
            type_text(model, text);
        }
    }
    crate::screens::editor::end_typing(model);
    if let Some(depth) = model.vim.change_depth.take() {
        model.active_document_mut().sql.merge_undo_since(depth);
    }
    if let Some(mut keys) = model.vim.recording.take() {
        keys.push(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        model.vim.last_change = keys;
        model.vim.last_change_text = inserted;
    }
    model.vim.mode = Mode::Normal;
    model.vim.insert_before = None;
    // Vim steps back onto the last character typed.
    let doc = model.active_document();
    let chars: Vec<char> = doc.text().chars().collect();
    let cursor = doc.cursor();
    if cursor > line_start(&chars, cursor) {
        set_cursor(model, cursor - 1);
    }
    clamp_normal(model);
}

/// What an Insert session put in: the text after it, less what it shares with the text
/// before at either end.
fn inserted_text(before: &str, after: &str) -> String {
    let before: Vec<char> = before.chars().collect();
    let after: Vec<char> = after.chars().collect();
    let prefix = before
        .iter()
        .zip(&after)
        .take_while(|(a, b)| a == b)
        .count();
    let room = before.len().min(after.len()) - prefix;
    let suffix = before
        .iter()
        .rev()
        .zip(after.iter().rev())
        .take(room)
        .take_while(|(a, b)| a == b)
        .count();
    after[prefix..after.len() - suffix].iter().collect()
}

/// `text` put in at the cursor, the cursor after it, as typing it would.
fn type_text(model: &mut Model, text: &str) {
    let cursor = model.active_document().cursor();
    replace(model, cursor..cursor, text);
    set_cursor(model, cursor + text.chars().count());
}

/// Into Insert mode, remembering the text so Esc can tell what was inserted.
fn start_insert(model: &mut Model) {
    model.vim.mode = Mode::Insert;
    model.vim.insert_before = Some(model.active_document().text());
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
    start_insert(model);
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
            let on_blank = chars.get(cursor).is_some_and(|ch| class(*ch) == 0);
            let (mut to, mut linewise, inclusive) =
                if op == 'c' && motion == Motion::WordForward && !on_blank {
                    // `cw` changes to the end of the word, as Vim does -- the word it is on,
                    // even from its last character -- unless it starts on a blank.
                    (change_word_end(&chars, cursor, count), false, true)
                } else if motion == Motion::WordForward {
                    (operator_word_end(&chars, cursor, count), false, false)
                } else {
                    target(&chars, cursor, motion, count)
                };
            // An exclusive motion that ends at the start of a later line, from at or
            // before the first non-blank of its own, takes whole lines: `dw` on an empty
            // line deletes it.
            if !linewise
                && !inclusive
                && to > cursor
                && to == line_start(&chars, to)
                && cursor <= first_non_blank(&chars, line_start(&chars, cursor))
            {
                linewise = true;
                to -= 1;
            }
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

fn visual_operate(model: &mut Model, op: char, whole_lines: bool) {
    let chars: Vec<char> = model.active_document().text().chars().collect();
    let cursor = model.active_document().cursor();
    let anchor = model.vim.visual_anchor;
    let linewise = whole_lines || model.vim.mode == Mode::VisualLine;
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
    // Nothing to act on -- `x` on an empty line: the register keeps what it had.
    if range.is_empty() && !linewise {
        if op == 'c' {
            start_insert(model);
        }
        return;
    }
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
            start_insert(model);
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
        // The change's Insert session is its text, put in as it was, not its keys.
        if key.code == KeyCode::Esc && model.vim.mode == Mode::Insert {
            if let Some(text) = model.vim.last_change_text.clone() {
                type_text(model, &text);
            }
            leave_insert(model);
            continue;
        }
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
        // `:q` asks about unsaved work the way closing a tab does; `:q!` drops it.
        "q" => Outcome::Then(vec![Action::CloseDocument]),
        "q!" => Outcome::Then(vec![Action::ResolveCloseActive(
            crate::model::CloseChoice::Discard,
        )]),
        // Saved first, closed once the save lands -- or kept, if it does not.
        "wq" => Outcome::Then(vec![Action::ResolveCloseActive(
            crate::model::CloseChoice::Save,
        )]),
        // `:x` writes only what changed.
        "x" if model.active_document().is_dirty() => {
            Outcome::Then(vec![Action::ResolveCloseActive(
                crate::model::CloseChoice::Save,
            )])
        }
        "x" => Outcome::Then(vec![Action::CloseDocument]),
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
        // An empty line is a word of its own, as in Vim.
        if chars[at] == '\n' && chars.get(at + 1) == Some(&'\n') {
            return at + 1;
        }
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

/// Where `w` takes an operator: the cursor's `w`, except that the last word moved over
/// ends the text at its line's end -- `dw` on a line's last word keeps the line break,
/// `d3w` across a line break takes the words before it -- and from an empty line the
/// next line's start is the end.
fn operator_word_end(chars: &[char], cursor: usize, count: usize) -> usize {
    let mut at = cursor;
    for step in 1..=count.max(1) {
        let from = at;
        if step == count.max(1) && chars.get(from) == Some(&'\n') && from == line_start(chars, from)
        {
            return from + 1;
        }
        at = next_word_start(chars, from);
        if step == count.max(1)
            && let Some(newline) = chars[from..at].iter().position(|ch| *ch == '\n')
        {
            return from + newline;
        }
    }
    at
}

/// The last character `cw` changes: the end of the word the cursor is on, even from its
/// last character, then of the words after it for a count.
fn change_word_end(chars: &[char], cursor: usize, count: usize) -> usize {
    let on_end = chars
        .get(cursor + 1)
        .is_none_or(|next| class(*next) != class(chars[cursor]));
    let mut at = cursor;
    let steps = if on_end {
        count.max(1) - 1
    } else {
        count.max(1)
    };
    for _ in 0..steps {
        at = word_end(chars, at);
    }
    at
}

/// `iw` and `aw`: the run of one class the cursor is on -- a word, or blanks -- within
/// its line, `count` of them; `aw` takes the blanks after it too, or those before it
/// when none follow.
fn word_object(chars: &[char], cursor: usize, around: bool, count: usize) -> Range<usize> {
    let (first, last) = (line_start(chars, cursor), line_end(chars, cursor));
    if cursor >= last {
        return cursor..cursor;
    }
    let class_at = |at: usize| class(chars[at]);
    let mut start = cursor;
    while start > first && class_at(start - 1) == class_at(cursor) {
        start -= 1;
    }
    // Each run is one: `2iw` is a word and the blanks after it, as in Vim.
    let mut end = cursor;
    for _ in 0..count.max(1) {
        if end >= last {
            break;
        }
        let here = class_at(end);
        while end < last && class_at(end) == here {
            end += 1;
        }
    }
    if around && class_at(cursor) != 0 {
        let mut after = end;
        while after < last && class_at(after) == 0 {
            after += 1;
        }
        if after > end {
            end = after;
        } else {
            while start > first && class_at(start - 1) == 0 {
                start -= 1;
            }
        }
    }
    start..end
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

    /// Visual mode acts on what it shows: both ends, whichever way it was made, arrows
    /// keeping the selection, and the insert commands staying out of it.
    #[test]
    fn visual_mode_operates_on_what_it_shows() {
        let mut model = vim("abcdef");
        model.active_document_mut().sql.set_cursor(3).unwrap();
        keys(&mut model, "vhh");
        assert_eq!(super::display_selection(&model), Some(1..4));
        keys(&mut model, "d");
        assert_eq!(text(&model), "aef");

        let mut model = vim("abcdef");
        keys(&mut model, "v");
        update(
            &mut model,
            Action::Key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE)),
        );
        update(
            &mut model,
            Action::Key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE)),
        );
        keys(&mut model, "d");
        assert_eq!(text(&model), "def");

        // `iw` picks the word; `X` takes whole lines, as in Vim.
        let mut model = vim("name rest");
        keys(&mut model, "viwd");
        assert_eq!(model.vim.mode, Mode::Normal);
        assert_eq!(text(&model), " rest");

        let mut model = vim("abc\nxyz");
        keys(&mut model, "vlX");
        assert_eq!(text(&model), "xyz");
        assert_eq!(model.vim.mode, Mode::Normal);
    }

    /// Switching documents starts Vim afresh there: an Insert session in one never
    /// merges the other's undo steps, and a Visual anchor never selects in it.
    #[test]
    fn vim_state_stays_with_its_document() {
        let mut model = vim("first");
        keys(&mut model, "V");
        let mut other = crate::model::EditorDocument::with_text("second\nlines");
        other.id = "other".into();
        model.documents.push(other);
        model.active_document = 1;
        assert_eq!(super::display_selection(&model), None);
        keys(&mut model, "d");
        assert_eq!(text(&model), "second\nlines");
        assert_eq!(model.vim.mode, Mode::Normal);
    }

    /// `:wq` saves and closes once the save lands; `:q!` closes without asking.
    #[test]
    fn write_quit_and_quit_bang() {
        let mut model = vim("select 1");
        model.active_document_mut().path = Some(std::path::PathBuf::from("/tmp/q.sql"));
        keys(&mut model, ":wq\n");
        assert!(model.close_prompt.is_none());
        assert!(model.pending_document_close.is_some());

        let mut model = vim("select 2");
        model.active_document_mut().id = "dirty".into();
        keys(&mut model, ":q!\n");
        assert!(model.close_prompt.is_none());
        assert!(model.documents.iter().all(|doc| doc.id != "dirty"));
    }

    /// Vim's own edges: `dw` on a line's last word, `cw` on blanks, `w` onto an empty
    /// line, a count for `.`, and counts on the insert commands.
    #[test]
    fn motions_and_counts_as_vim_has_them() {
        let mut model = vim("one two\nthree");
        model.active_document_mut().sql.set_cursor(4).unwrap();
        keys(&mut model, "dw");
        assert_eq!(text(&model), "one \nthree");

        let mut model = vim("one   two");
        model.active_document_mut().sql.set_cursor(3).unwrap();
        keys(&mut model, "cwX\u{1b}");
        assert_eq!(text(&model), "oneXtwo");

        let mut model = vim("a\n\nb");
        keys(&mut model, "w");
        assert_eq!(model.active_document().cursor(), 2);

        let mut model = vim("1\n2\n3\n4\n5\n6");
        keys(&mut model, "2dd3.");
        assert_eq!(text(&model), "6");

        let mut model = vim("");
        keys(&mut model, "3ia\u{1b}");
        assert_eq!(text(&model), "aaa");
        keys(&mut model, "2ox\u{1b}");
        assert_eq!(text(&model), "aaa\nx\nx");
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

    /// `d3w` across a line break takes the words before it, `dw` on an empty line
    /// deletes it, and `dw` on a line's last word keeps the line break.
    #[test]
    fn delete_words_like_vim() {
        let mut model = vim("a b\nc d e");
        keys(&mut model, "d3w");
        assert_eq!(text(&model), "d e");
        let mut model = vim("a\n\nb");
        model.active_document_mut().sql.set_cursor(2).unwrap();
        keys(&mut model, "dw");
        assert_eq!(text(&model), "a\nb");
        let mut model = vim("a b\nc");
        model.active_document_mut().sql.set_cursor(2).unwrap();
        keys(&mut model, "dw");
        assert_eq!(text(&model), "a \nc");
    }

    /// `cw` on a word's last character changes only that character.
    #[test]
    fn change_word_from_its_last_character() {
        let mut model = vim("a.id");
        keys(&mut model, "cwx\u{1b}");
        assert_eq!(text(&model), "x.id");
    }

    /// A count given to `.` replaces every count of the change: `d2w` then `3.`
    /// deletes three words.
    #[test]
    fn a_count_on_dot_replaces_the_changes_own() {
        let mut model = vim("a b c d e f g h");
        keys(&mut model, "d2w3.");
        assert_eq!(text(&model), "f g h");
    }

    /// A repeated Insert session puts its text in again, not its keys: with
    /// completion in the way the keys typed something else.
    #[test]
    fn repeated_inserts_put_the_text_in() {
        let mut model = vim("");
        keys(&mut model, "3ia\u{1b}");
        assert_eq!(text(&model), "aaa");
        let mut model = vim("x");
        keys(&mut model, "2oy\u{1b}");
        assert_eq!(text(&model), "x\ny\ny");
        let mut model = vim("one\ntwo");
        keys(&mut model, "Ahi\u{1b}j.");
        assert_eq!(text(&model), "onehi\ntwohi");
    }

    /// Visual `iw` and `aw` pick a word, and the capitals act on whole lines.
    #[test]
    fn visual_text_objects_and_capitals() {
        let mut model = vim("select name from t");
        model.active_document_mut().sql.set_cursor(8).unwrap();
        keys(&mut model, "viwd");
        assert_eq!(text(&model), "select  from t");
        let mut model = vim("select name from t");
        model.active_document_mut().sql.set_cursor(8).unwrap();
        keys(&mut model, "vawd");
        assert_eq!(text(&model), "select from t");
        let mut model = vim("select name from t");
        model.active_document_mut().sql.set_cursor(8).unwrap();
        keys(&mut model, "ciwid\u{1b}");
        assert_eq!(text(&model), "select id from t");
        let mut model = vim("one\ntwo\nthree");
        model.active_document_mut().sql.set_cursor(5).unwrap();
        keys(&mut model, "vX");
        assert_eq!(text(&model), "one\nthree");
    }

    /// An empty `x` leaves the register as it was; Ctrl with an arrow moves.
    #[test]
    fn an_empty_delete_keeps_the_register_and_ctrl_arrows_move() {
        let mut model = vim("ab\n\ncd");
        keys(&mut model, "x");
        model.active_document_mut().sql.set_cursor(2).unwrap();
        keys(&mut model, "x");
        model.active_document_mut().sql.set_cursor(3).unwrap();
        keys(&mut model, "p");
        assert_eq!(text(&model), "b\n\ncad");
        let mut model = vim("select name from t");
        update(
            &mut model,
            Action::Key(KeyEvent::new(KeyCode::Right, KeyModifiers::CONTROL)),
        );
        assert!(model.active_document().cursor() > 0);
    }

    /// Ctrl+W on the `:` line closed the document: the keymap had it before the line.
    /// It deletes the word before the cursor, as in Vim.
    #[test]
    fn ctrl_w_on_the_command_line_deletes_a_word() {
        let mut model = vim("select 1");
        let documents = model.documents.len();
        keys(&mut model, ":s/a/b");
        update(
            &mut model,
            Action::Key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL)),
        );
        assert_eq!(model.documents.len(), documents, "the document closed");
        let prompt = model.vim.prompt.as_ref().expect("the line stays open");
        assert_eq!(prompt.input.as_str(), "s/a/");
    }

    /// Another document starts in Normal mode, and says so before its first key.
    #[test]
    fn another_document_starts_in_normal_mode() {
        let mut model = vim("a");
        keys(&mut model, "i");
        assert_eq!(super::mode(&model), Mode::Insert);
        let mut other = crate::model::EditorDocument::with_text("b\nc");
        other.id = "other".into();
        model.documents.push(other);
        model.set_active_document(1);
        assert_eq!(super::mode(&model), Mode::Normal);
        let status = crate::render::render_to_string(&model, 100, 30);
        assert!(status.contains("-- NORMAL --"), "{status}");
    }
}
