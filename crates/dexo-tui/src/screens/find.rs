//! Find and replace in the active document: the find bar, and Vim's `/ n N` and `:s`,
//! all on this one engine. Patterns are literal text.

use std::ops::Range;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::model::Model;
use crate::widgets::text_input::TextInput;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FindOptions {
    pub case_sensitive: bool,
    pub whole_word: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum FindField {
    #[default]
    Query,
    Replace,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct FindState {
    pub open: bool,
    /// The replace row is showing (Ctrl+H rather than Ctrl+F).
    pub replacing: bool,
    pub query: TextInput,
    pub replacement: TextInput,
    pub field: FindField,
    pub options: FindOptions,
}

/// Every match of `query` in `text`, as char ranges, left to right and not overlapping.
pub fn find_all(text: &str, query: &str, options: FindOptions) -> Vec<Range<usize>> {
    if query.is_empty() {
        return Vec::new();
    }
    let fold = |ch: char| -> char {
        if options.case_sensitive {
            ch
        } else {
            // One char in, one char out, so char positions stay put; the few letters
            // whose lowercase is longer (İ) keep their first char.
            ch.to_lowercase().next().unwrap_or(ch)
        }
    };
    let haystack: Vec<char> = text.chars().map(fold).collect();
    let needle: Vec<char> = query.chars().map(fold).collect();
    let word = |at: usize| haystack.get(at).is_some_and(|ch| is_word_char(*ch));
    let mut found = Vec::new();
    let mut at = 0;
    while at + needle.len() <= haystack.len() {
        let end = at + needle.len();
        let touches_word = (at > 0 && word(at - 1)) || word(end);
        let bounded = !options.whole_word || !touches_word;
        if haystack[at..end] == needle[..] && bounded {
            found.push(at..end);
            at = end;
        } else {
            at += 1;
        }
    }
    found
}

/// The match to go to from `cursor`: the first starting at or after it going forward,
/// the last starting before it going back, wrapping around either way.
pub fn nearest(matches: &[Range<usize>], cursor: usize, forward: bool) -> Option<usize> {
    if matches.is_empty() {
        return None;
    }
    if forward {
        Some(
            matches
                .iter()
                .position(|range| range.start >= cursor)
                .unwrap_or(0),
        )
    } else {
        Some(
            matches
                .iter()
                .rposition(|range| range.start < cursor)
                .unwrap_or(matches.len() - 1),
        )
    }
}

/// Rows the bar takes at the bottom of the editor pane.
pub fn bar_rows(model: &Model) -> usize {
    match (model.find.open, model.find.replacing) {
        (false, _) => 0,
        (true, false) => 1,
        (true, true) => 2,
    }
}

/// The active document's matches for what the bar holds.
pub fn matches(model: &Model) -> Vec<Range<usize>> {
    find_all(
        &model.active_document().text(),
        model.find.query.as_str(),
        model.find.options,
    )
}

/// Which match the selection is, when it is one -- the match the bar calls current.
pub fn current(model: &Model, matches: &[Range<usize>]) -> Option<usize> {
    let selection = model.active_document().selection()?;
    matches.iter().position(|range| *range == selection)
}

/// Ctrl+F, or Ctrl+H with `replace`. A selection on one line becomes the query.
pub fn open(model: &mut Model, replace: bool) {
    let doc = model.active_document();
    if let Some(range) = doc.selection() {
        let picked: String = doc
            .text()
            .chars()
            .skip(range.start)
            .take(range.len())
            .collect();
        if !picked.contains('\n') {
            model.find.query.set_text(picked);
        }
    }
    model.find.open = true;
    model.find.replacing = replace;
    model.find.field = if replace && !model.find.query.is_empty() {
        FindField::Replace
    } else {
        FindField::Query
    };
    seek(model);
}

pub fn close(model: &mut Model) {
    model.find.open = false;
}

/// The keys the bar owns while it is open. Anything else -- the palette, running the
/// statement -- goes on to the keymap, but never into the document's text.
pub fn handle_key(model: &mut Model, key: KeyEvent) -> bool {
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
    match key.code {
        KeyCode::Esc => close(model),
        KeyCode::Enter | KeyCode::F(3) if shift => step(model, false),
        KeyCode::Enter if model.find.field == FindField::Replace => replace_current(model),
        KeyCode::Enter | KeyCode::F(3) | KeyCode::Down => step(model, true),
        KeyCode::Up => step(model, false),
        KeyCode::Tab | KeyCode::BackTab if model.find.replacing => {
            model.find.field = match model.find.field {
                FindField::Query => FindField::Replace,
                FindField::Replace => FindField::Query,
            };
        }
        KeyCode::Char('c') if alt => {
            model.find.options.case_sensitive = !model.find.options.case_sensitive;
            seek(model);
        }
        KeyCode::Char('w') if alt => {
            model.find.options.whole_word = !model.find.options.whole_word;
            seek(model);
        }
        KeyCode::Char('a') if alt && model.find.replacing => replace_all(model),
        // The way to the replace row where Ctrl+H is Ctrl+Backspace.
        KeyCode::Char('r') if alt => {
            model.find.replacing = !model.find.replacing;
            model.find.field = if model.find.replacing {
                FindField::Replace
            } else {
                FindField::Query
            };
        }
        _ => {
            let field = model.find.field;
            let input = match field {
                FindField::Query => &mut model.find.query,
                FindField::Replace => &mut model.find.replacement,
            };
            let before = input.as_str().to_string();
            if !input.handle_key(key) {
                return false;
            }
            if field == FindField::Query && model.find.query.as_str() != before {
                seek(model);
            }
        }
    }
    true
}

/// Selects the first match at or after where the search stands -- the current match's
/// start, or the cursor -- the way typing into the bar walks the document.
fn seek(model: &mut Model) {
    let found = matches(model);
    let doc = model.active_document();
    let from = doc.selection().map_or(doc.cursor(), |range| range.start);
    match nearest(&found, from, true) {
        Some(index) => select(model, found[index].clone()),
        // Nothing to show: drop a selection a shorter query left behind.
        None => {
            if found.is_empty() && !model.find.query.is_empty() {
                model.active_document_mut().anchor = None;
            }
        }
    }
}

/// The next match, or the previous one, from the current match or the cursor.
pub fn step(model: &mut Model, forward: bool) {
    let found = matches(model);
    let index = match current(model, &found) {
        Some(index) if forward => Some((index + 1) % found.len()),
        Some(index) => Some((index + found.len() - 1) % found.len()),
        None => {
            let cursor = model.active_document().cursor();
            nearest(&found, cursor, forward)
        }
    };
    if let Some(index) = index {
        select(model, found[index].clone());
    }
}

fn select(model: &mut Model, range: Range<usize>) {
    crate::screens::editor::end_typing(model);
    let doc = model.active_document_mut();
    doc.anchor = Some(range.start);
    let _ = doc.sql.set_cursor(range.end);
    crate::screens::editor::follow_cursor(model);
}

/// Replaces the current match and moves on to the next; with none current, finds one.
fn replace_current(model: &mut Model) {
    let found = matches(model);
    let Some(index) = current(model, &found) else {
        step(model, true);
        return;
    };
    let replacement = model.find.replacement.as_str().to_string();
    let range = found[index].clone();
    edit(model, |doc| {
        let _ = doc.sql.replace_chars(range.clone(), &replacement);
    });
    let after = range.start + replacement.chars().count();
    let found = matches(model);
    if let Some(next) = nearest(&found, after, true) {
        select(model, found[next].clone());
    } else {
        let _ = model.active_document_mut().sql.set_cursor(after);
    }
}

/// Every match at once, as one undo step.
fn replace_all(model: &mut Model) {
    let found = matches(model);
    if found.is_empty() {
        model.messages.info("Nothing to replace.".into());
        return;
    }
    let replacement = model.find.replacement.as_str().to_string();
    edit(model, |doc| {
        // From the end, so the ranges still ahead keep their positions.
        for range in found.iter().rev() {
            let _ = doc.sql.replace_chars(range.clone(), &replacement);
        }
    });
    model
        .messages
        .info(format!("Replaced {} matches.", found.len()));
}

/// One undo step around `change`, with no selection left over.
fn edit(model: &mut Model, change: impl FnOnce(&mut crate::model::EditorDocument)) {
    crate::screens::editor::end_typing(model);
    let doc = model.active_document_mut();
    doc.anchor = None;
    doc.sql.begin_group();
    change(doc);
    doc.sql.end_group();
    crate::screens::editor::refresh_intelligence(model, false);
}

fn is_word_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::{FindOptions, find_all, nearest};
    use crate::action::Action;
    use crate::model::{Focus, Model};
    use crate::update::update;

    fn press(model: &mut Model, code: KeyCode, modifiers: KeyModifiers) {
        update(model, Action::Key(KeyEvent::new(code, modifiers)));
    }

    fn editor_with(text: &str) -> Model {
        let mut model = Model {
            focus: Focus::Editor,
            ..Model::default()
        };
        model.active_document_mut().sql.insert(0, text).unwrap();
        model.active_document_mut().sql.set_cursor(0).unwrap();
        model
    }

    /// Replace all is one undo step, and the bar's keys never reach the text.
    #[test]
    fn replace_all_is_one_undo_step() {
        let mut model = editor_with("select ação, ação from t");
        model.keys_disambiguated = true;
        press(&mut model, KeyCode::Char('h'), KeyModifiers::CONTROL);
        assert!(model.find.open && model.find.replacing);
        for ch in "ação".chars() {
            press(&mut model, KeyCode::Char(ch), KeyModifiers::NONE);
        }
        press(&mut model, KeyCode::Tab, KeyModifiers::NONE);
        for ch in "x".chars() {
            press(&mut model, KeyCode::Char(ch), KeyModifiers::NONE);
        }
        press(&mut model, KeyCode::Char('a'), KeyModifiers::ALT);
        assert_eq!(model.active_document().text(), "select x, x from t");
        press(&mut model, KeyCode::Esc, KeyModifiers::NONE);
        assert!(!model.find.open);
        press(&mut model, KeyCode::Char('z'), KeyModifiers::CONTROL);
        assert_eq!(model.active_document().text(), "select ação, ação from t");
    }

    /// A query wider than the pane shows its end, where the typing is, at small sizes.
    #[test]
    fn a_long_query_shows_where_the_typing_is() {
        let mut model = editor_with("select 1");
        model.keys_disambiguated = true;
        press(&mut model, KeyCode::Char('f'), KeyModifiers::CONTROL);
        for ch in "abcdefghijklmnopqrstuvwxyz0123456789zz".chars() {
            press(&mut model, KeyCode::Char(ch), KeyModifiers::NONE);
        }
        let screen = crate::render::render_to_string(&model, 40, 8);
        assert!(screen.contains("6789zz"), "{screen}");
    }

    /// Where ^H is Ctrl+Backspace, Ctrl+H deletes a word instead of opening the bar.
    #[test]
    fn ctrl_h_is_replace_only_where_keys_are_unambiguous() {
        let mut model = editor_with("select name");
        model.active_document_mut().sql.set_cursor(11).unwrap();
        press(&mut model, KeyCode::Char('h'), KeyModifiers::CONTROL);
        assert!(!model.find.open);
        assert_eq!(model.active_document().text(), "select ");
    }

    #[test]
    fn matches_are_char_ranges_and_respect_case_and_words() {
        let text = "Ação ação AÇÃO; select ação_x";
        let any = FindOptions::default();
        assert_eq!(
            find_all(text, "ação", any),
            vec![0..4, 5..9, 10..14, 23..27]
        );
        let exact = FindOptions {
            case_sensitive: true,
            ..any
        };
        assert_eq!(find_all(text, "ação", exact), vec![5..9, 23..27]);
        let words = FindOptions {
            whole_word: true,
            ..any
        };
        assert_eq!(find_all(text, "ação", words), vec![0..4, 5..9, 10..14]);
        assert!(find_all(text, "", any).is_empty());
        // Matches never overlap.
        assert_eq!(find_all("aaaa", "aa", any), vec![0..2, 2..4]);
    }

    #[test]
    fn nearest_wraps_both_ways() {
        let matches = vec![2..4, 8..10];
        assert_eq!(nearest(&matches, 0, true), Some(0));
        assert_eq!(nearest(&matches, 2, true), Some(0));
        assert_eq!(nearest(&matches, 3, true), Some(1));
        assert_eq!(nearest(&matches, 9, true), Some(0));
        assert_eq!(nearest(&matches, 8, false), Some(0));
        assert_eq!(nearest(&matches, 2, false), Some(1));
        assert_eq!(nearest(&[], 0, true), None);
    }
}
