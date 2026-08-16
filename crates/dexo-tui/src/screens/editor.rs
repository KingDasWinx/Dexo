use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use dexo_driver_api::DbValue;
use dexo_sql::{
    CompletionItem, Dialect, FakeCatalog, HighlightSpan, HistoryPolicy, ParserService, Snippet,
    complete, expand_placeholders, format_sql, named_parameters,
};

use crate::model::{EditorDocument, Model};

#[derive(Clone, Debug, PartialEq)]
pub struct ParameterValue {
    pub name: String,
    pub value: DbValue,
    pub sensitive: bool,
}

pub struct EditorState {
    parser: ParserService,
    last_sql: String,
    pub highlights: Vec<HighlightSpan>,
    pub parameters: Vec<ParameterValue>,
    pub completions: Vec<CompletionItem>,
    pub format_preview: Option<String>,
    pub completion_open: bool,
    pub completion_selected: usize,
    pub parameter_prompt: bool,
    pub parameter_index: usize,
    pub parameter_draft: String,
    pub snippets: Vec<Snippet>,
    pub snippet_open: bool,
    pub snippet_selected: usize,
    pub history: Vec<String>,
    pub history_open: bool,
    pub history_selected: usize,
    pub history_policy: HistoryPolicy,
    catalog: FakeCatalog,
}

impl std::fmt::Debug for EditorState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EditorState")
            .field("highlights", &self.highlights)
            .field("parameters", &self.parameters)
            .field("completions", &self.completions)
            .finish_non_exhaustive()
    }
}

impl Clone for EditorState {
    fn clone(&self) -> Self {
        Self {
            parser: ParserService::postgres(),
            last_sql: self.last_sql.clone(),
            highlights: self.highlights.clone(),
            parameters: self.parameters.clone(),
            completions: self.completions.clone(),
            format_preview: self.format_preview.clone(),
            completion_open: self.completion_open,
            completion_selected: self.completion_selected,
            parameter_prompt: self.parameter_prompt,
            parameter_index: self.parameter_index,
            parameter_draft: self.parameter_draft.clone(),
            snippets: self.snippets.clone(),
            snippet_open: self.snippet_open,
            snippet_selected: self.snippet_selected,
            history: self.history.clone(),
            history_open: self.history_open,
            history_selected: self.history_selected,
            history_policy: self.history_policy,
            catalog: self.catalog.clone(),
        }
    }
}

impl PartialEq for EditorState {
    fn eq(&self, other: &Self) -> bool {
        self.highlights == other.highlights
            && self.parameters == other.parameters
            && self.completions == other.completions
            && self.format_preview == other.format_preview
            && self.snippets == other.snippets
            && self.history == other.history
    }
}

impl EditorState {
    pub fn reset_parse(&mut self) {
        self.last_sql.clear();
    }
}

impl Default for EditorState {
    fn default() -> Self {
        Self {
            parser: ParserService::postgres(),
            last_sql: String::new(),
            highlights: Vec::new(),
            parameters: Vec::new(),
            completions: Vec::new(),
            format_preview: None,
            completion_open: false,
            completion_selected: 0,
            parameter_prompt: false,
            parameter_index: 0,
            parameter_draft: String::new(),
            snippets: Vec::new(),
            snippet_open: false,
            snippet_selected: 0,
            history: Vec::new(),
            history_open: false,
            history_selected: 0,
            history_policy: HistoryPolicy::SqlOnly,
            catalog: FakeCatalog::table("public.users", ["id", "email"]),
        }
    }
}

pub fn refresh_intelligence(model: &mut Model, with_completion: bool) {
    let sql = model.active_document().text();
    let cursor = model.active_document().cursor();
    let byte_cursor = sql.chars().take(cursor).map(char::len_utf8).sum();
    let old = std::mem::take(&mut model.editor.last_sql);
    let parsed = model.editor.parser.parse_edited(&old, &sql);
    model.editor.last_sql = sql.clone();
    model.editor.highlights = parsed.highlights;
    model.editor.parameters = named_parameters(&sql)
        .into_iter()
        .map(|parameter| ParameterValue {
            sensitive: is_sensitive_name(&parameter.name),
            name: parameter.name,
            value: DbValue::Null,
        })
        .collect();
    if with_completion {
        // ponytail: complete after FROM when present so table names survive a trailing :param token.
        let at = sql
            .to_ascii_lowercase()
            .find(" from ")
            .map(|index| index + 6)
            .unwrap_or(byte_cursor);
        let objects = model.explorer.flatten();
        model.editor.completions = if objects.is_empty() {
            complete(
                &sql,
                at.min(sql.len()),
                &model.editor.catalog,
                Dialect::Postgres,
            )
        } else {
            let snapshot = dexo_app::SnapshotCatalog::new(objects);
            complete(&sql, at.min(sql.len()), &snapshot, Dialect::Postgres)
        };
        model.editor.completion_open = true;
        model.editor.completion_selected = 0;
    }
}

fn is_sensitive_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.contains("password") || lower.contains("secret") || lower.contains("token")
}

pub fn apply_format(model: &mut Model) {
    let sql = model.active_document().text();
    match format_sql(&sql, Dialect::Postgres) {
        Ok(formatted) => {
            model.editor.format_preview = Some(formatted.clone());
            model.set_sql(&formatted);
            refresh_intelligence(model, false);
        }
        Err(error) => model.messages.push(error.to_string()),
    }
}

pub fn insert_active_snippet(model: &mut Model) {
    if model.editor.snippets.is_empty() {
        return;
    }
    if model.editor.snippets.len() == 1 {
        insert_snippet_at(model, 0);
        return;
    }
    model.editor.snippet_open = true;
    model.editor.snippet_selected = 0;
}

pub fn insert_snippet_at(model: &mut Model, index: usize) {
    let Some(snippet) = model.editor.snippets.get(index).cloned() else {
        return;
    };
    model.editor.snippet_open = false;
    insert_text(model, &expand_placeholders(&snippet.body));
    refresh_intelligence(model, false);
}

pub fn accept_completion(model: &mut Model) {
    let index = model.editor.completion_selected;
    let Some(item) = model.editor.completions.get(index).cloned() else {
        return;
    };
    insert_text(model, &item.label);
    model.editor.completion_open = false;
    refresh_intelligence(model, false);
}

pub fn move_completion(model: &mut Model, delta: i32) {
    if model.editor.completions.is_empty() {
        return;
    }
    let max = model.editor.completions.len() as i32 - 1;
    model.editor.completion_selected =
        (model.editor.completion_selected as i32 + delta).clamp(0, max) as usize;
}

pub fn submit_parameters(model: &mut Model) {
    if !model.editor.parameter_draft.is_empty() {
        let index = model.editor.parameter_index;
        if let Some(parameter) = model.editor.parameters.get_mut(index) {
            parameter.value = DbValue::Text(std::mem::take(&mut model.editor.parameter_draft));
        }
    }
    let next = model.editor.parameter_index + 1;
    if next < model.editor.parameters.len()
        && model
            .editor
            .parameters
            .iter()
            .any(|parameter| matches!(parameter.value, DbValue::Null))
    {
        model.editor.parameter_index = next;
        model.editor.parameter_prompt = true;
        return;
    }
    model.editor.parameter_prompt = false;
    model.editor.parameter_index = 0;
}

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
        KeyCode::Enter if model.editor.completion_open => {
            accept_completion(model);
            true
        }
        KeyCode::Enter => {
            insert_newline(model);
            true
        }
        KeyCode::Tab if model.editor.completion_open => {
            accept_completion(model);
            true
        }
        KeyCode::Up if model.editor.completion_open => {
            move_completion(model, -1);
            true
        }
        KeyCode::Down if model.editor.completion_open => {
            move_completion(model, 1);
            true
        }
        KeyCode::Esc if model.editor.completion_open => {
            model.editor.completion_open = false;
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
    let mut line_start = 0;
    for (index, ch) in text.chars().enumerate() {
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
    }
    if current_line < line {
        return text.chars().count();
    }
    text.chars().count()
}

pub fn handle_history_key(model: &mut Model, key: KeyEvent) -> bool {
    match key.code {
        KeyCode::Esc => {
            model.editor.history_open = false;
            true
        }
        KeyCode::Up => {
            model.editor.history_selected = model.editor.history_selected.saturating_sub(1);
            true
        }
        KeyCode::Down => {
            if model.editor.history_selected + 1 < model.editor.history.len() {
                model.editor.history_selected += 1;
            }
            true
        }
        _ => false,
    }
}

pub fn pick_history(model: &mut Model) -> bool {
    let Some(sql) = model
        .editor
        .history
        .get(model.editor.history_selected)
        .cloned()
    else {
        model.editor.history_open = false;
        return false;
    };
    model.editor.history_open = false;
    model.set_sql(&sql);
    true
}

pub fn handle_snippet_key(model: &mut Model, key: KeyEvent) -> bool {
    match key.code {
        KeyCode::Esc => {
            model.editor.snippet_open = false;
            true
        }
        KeyCode::Up => {
            model.editor.snippet_selected = model.editor.snippet_selected.saturating_sub(1);
            true
        }
        KeyCode::Down => {
            if model.editor.snippet_selected + 1 < model.editor.snippets.len() {
                model.editor.snippet_selected += 1;
            }
            true
        }
        KeyCode::Enter => {
            insert_snippet_at(model, model.editor.snippet_selected);
            true
        }
        _ => false,
    }
}

pub fn handle_parameter_key(model: &mut Model, key: KeyEvent) -> bool {
    match key.code {
        KeyCode::Esc => {
            model.editor.parameter_prompt = false;
            true
        }
        KeyCode::Backspace => {
            model.editor.parameter_draft.pop();
            true
        }
        KeyCode::Char(ch) if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT => {
            model.editor.parameter_draft.push(ch);
            true
        }
        KeyCode::Enter => {
            submit_parameters(model);
            true
        }
        _ => false,
    }
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
