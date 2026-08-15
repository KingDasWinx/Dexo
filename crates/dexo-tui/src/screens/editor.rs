use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::model::{EditorDocument, Model};

pub fn handle_key(model: &mut Model, key: KeyEvent) -> bool {
    if model.focus != crate::model::Focus::Editor {
        return false;
    }
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
    match key.code {
        KeyCode::Char(ch) if ctrl && (ch == 'z' || ch == 'Z') => {
            undo(model);
            true
        }
        KeyCode::Char(ch) if ctrl && (ch == 'y' || ch == 'Y') => {
            redo(model);
            true
        }
        KeyCode::Char('a') if ctrl => {
            select_all(model);
            true
        }
        KeyCode::Char(ch) if !ctrl => {
            insert_text(model, &ch.to_string());
            true
        }
        KeyCode::Enter => {
            insert_newline(model);
            true
        }
        KeyCode::Tab => {
            insert_text(model, "    ");
            true
        }
        KeyCode::Backspace => {
            backspace(model);
            true
        }
        KeyCode::Delete => {
            delete(model);
            true
        }
        KeyCode::Left => {
            move_chars(model, -1, shift, ctrl);
            true
        }
        KeyCode::Right => {
            move_chars(model, 1, shift, ctrl);
            true
        }
        KeyCode::Home => {
            move_line_edge(model, true, shift);
            true
        }
        KeyCode::End => {
            move_line_edge(model, false, shift);
            true
        }
        KeyCode::Up => {
            move_vertical(model, -1, shift);
            true
        }
        KeyCode::Down => {
            move_vertical(model, 1, shift);
            true
        }
        _ => false,
    }
}

fn insert_text(model: &mut Model, text: &str) {
    let doc = model.active_document_mut();
    let range = doc.selection();
    if range.is_some() && doc.typing {
        doc.sql.end_group();
        doc.typing = false;
    }
    if !doc.typing {
        doc.sql.end_group();
        doc.sql.begin_group();
        doc.typing = true;
    }
    let _ = if let Some(range) = range {
        doc.anchor = None;
        doc.sql.replace_chars(range, text)
    } else {
        doc.sql.insert(doc.sql.cursor(), text)
    };
    reveal_cursor(doc);
}

fn insert_newline(model: &mut Model) {
    let indent = current_line_indent(model);
    end_typing(model);
    let doc = model.active_document_mut();
    let _ = doc.sql.insert(doc.sql.cursor(), &format!("\n{indent}"));
    reveal_cursor(doc);
}

fn backspace(model: &mut Model) {
    end_typing(model);
    let doc = model.active_document_mut();
    if let Some(range) = doc.selection() {
        doc.anchor = None;
        let _ = doc.sql.delete(range);
    } else {
        let cursor = doc.sql.cursor();
        if cursor > 0 {
            let _ = doc.sql.delete(cursor - 1..cursor);
        }
    }
    reveal_cursor(doc);
}

fn delete(model: &mut Model) {
    end_typing(model);
    let doc = model.active_document_mut();
    if let Some(range) = doc.selection() {
        doc.anchor = None;
        let _ = doc.sql.delete(range);
    } else {
        let cursor = doc.sql.cursor();
        let len = doc.sql.text().chars().count();
        if cursor < len {
            let _ = doc.sql.delete(cursor..cursor + 1);
        }
    }
    reveal_cursor(doc);
}

fn undo(model: &mut Model) {
    end_typing(model);
    let doc = model.active_document_mut();
    let _ = doc.sql.undo();
    reveal_cursor(doc);
}

fn redo(model: &mut Model) {
    end_typing(model);
    let doc = model.active_document_mut();
    let _ = doc.sql.redo();
    reveal_cursor(doc);
}

fn select_all(model: &mut Model) {
    end_typing(model);
    let doc = model.active_document_mut();
    let len = doc.sql.text().chars().count();
    doc.anchor = Some(0);
    let _ = doc.sql.set_cursor(len);
}

fn move_chars(model: &mut Model, delta: i32, shift: bool, word: bool) {
    end_typing(model);
    let doc = model.active_document_mut();
    let text = doc.sql.text();
    let len = text.chars().count();
    let mut cursor = doc.sql.cursor();
    if word {
        cursor = word_jump(&text, cursor, delta);
    } else if delta < 0 {
        cursor = cursor.saturating_sub(1);
    } else {
        cursor = (cursor + 1).min(len);
    }
    apply_move(doc, cursor, shift);
    reveal_cursor(doc);
}

fn move_line_edge(model: &mut Model, home: bool, shift: bool) {
    end_typing(model);
    let doc = model.active_document_mut();
    let text = doc.sql.text();
    let (line_start, line_end) = line_bounds(&text, doc.sql.cursor());
    apply_move(doc, if home { line_start } else { line_end }, shift);
    reveal_cursor(doc);
}

fn move_vertical(model: &mut Model, delta: i32, shift: bool) {
    end_typing(model);
    let doc = model.active_document_mut();
    let text = doc.sql.text();
    let (line, col) = line_col(&text, doc.sql.cursor());
    let next_line = if delta < 0 {
        line.saturating_sub(1)
    } else {
        line + 1
    };
    let cursor = cursor_at(&text, next_line, col);
    apply_move(doc, cursor, shift);
    reveal_cursor(doc);
}

fn apply_move(doc: &mut EditorDocument, cursor: usize, shift: bool) {
    if shift {
        if doc.anchor.is_none() {
            doc.anchor = Some(doc.sql.cursor());
        }
        let _ = doc.sql.set_cursor(cursor);
    } else {
        doc.anchor = None;
        let _ = doc.sql.set_cursor(cursor);
    }
}

pub(crate) fn end_typing(model: &mut Model) {
    let doc = model.active_document_mut();
    if doc.typing {
        doc.sql.end_group();
        doc.typing = false;
    }
}

pub(crate) fn line_col_of(text: &str, cursor: usize) -> (usize, usize) {
    line_col(text, cursor)
}

fn current_line_indent(model: &Model) -> String {
    let doc = model.active_document();
    let text = doc.sql.text();
    let (start, _) = line_bounds(&text, doc.sql.cursor());
    text.chars()
        .skip(start)
        .take_while(|ch| *ch == ' ' || *ch == '\t')
        .collect()
}

fn word_jump(text: &str, cursor: usize, delta: i32) -> usize {
    // ponytail: O(n) char scan per keystroke; switch to rope line/char APIs if files get huge.
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    let mut i = cursor.min(len);
    let is_word = |ch: char| ch.is_ascii_alphanumeric() || ch == '_';
    if delta < 0 {
        if i == 0 {
            return 0;
        }
        i -= 1;
        while i > 0 && !is_word(chars[i]) {
            i -= 1;
        }
        while i > 0 && is_word(chars[i - 1]) {
            i -= 1;
        }
        i
    } else {
        while i < len && is_word(chars[i]) {
            i += 1;
        }
        while i < len && !is_word(chars[i]) {
            i += 1;
        }
        i
    }
}

fn line_bounds(text: &str, cursor: usize) -> (usize, usize) {
    let chars: Vec<char> = text.chars().collect();
    let mut start = cursor.min(chars.len());
    while start > 0 && chars[start - 1] != '\n' {
        start -= 1;
    }
    let mut end = cursor.min(chars.len());
    while end < chars.len() && chars[end] != '\n' {
        end += 1;
    }
    (start, end)
}

fn line_col(text: &str, cursor: usize) -> (usize, usize) {
    let mut line = 0;
    let mut col = 0;
    for (index, ch) in text.chars().enumerate() {
        if index == cursor {
            return (line, col);
        }
        if ch == '\n' {
            line += 1;
            col = 0;
        } else {
            col += 1;
        }
    }
    (line, col)
}

fn cursor_at(text: &str, line: usize, col: usize) -> usize {
    let mut current_line = 0;
    let mut index = 0;
    let mut line_start = 0;
    for ch in text.chars() {
        if current_line == line && index - line_start >= col {
            return index;
        }
        if ch == '\n' {
            if current_line == line {
                return index;
            }
            current_line += 1;
            line_start = index + 1;
        }
        index += 1;
    }
    if current_line < line {
        return text.chars().count();
    }
    text.chars().count()
}

pub fn reveal_cursor(doc: &mut EditorDocument) {
    let text = doc.sql.text();
    let (line, col) = line_col(&text, doc.sql.cursor());
    if line < doc.viewport_line {
        doc.viewport_line = line;
    }
    if line >= doc.viewport_line + 12 {
        doc.viewport_line = line.saturating_sub(11);
    }
    if col < doc.viewport_column {
        doc.viewport_column = col;
    }
    if col >= doc.viewport_column + 80 {
        doc.viewport_column = col.saturating_sub(79);
    }
}
