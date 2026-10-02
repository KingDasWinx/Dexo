use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TextInput {
    text: String,
    cursor: usize,
    /// The whole text is selected: what is typed next replaces it.
    selected: bool,
}

impl TextInput {
    pub fn new(text: impl Into<String>) -> Self {
        let text = text.into();
        let cursor = text.chars().count();
        Self {
            text,
            cursor,
            selected: false,
        }
    }

    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
        self.selected = false;
    }

    /// Overwrites the text before letting it go, for an input that holds a secret.
    pub fn wipe(&mut self) {
        use secrecy::zeroize::Zeroize;
        self.text.zeroize();
        self.clear();
    }

    pub fn set_text(&mut self, text: impl Into<String>) {
        let text = text.into();
        self.cursor = text.chars().count();
        self.text = text;
        self.selected = false;
    }

    /// Selects the whole text, as Ctrl+A does: typing replaces it, Backspace or Delete
    /// clears it, and a move leaves it where it was.
    pub fn select_all(&mut self) {
        self.selected = !self.text.is_empty();
        self.cursor = self.len();
    }

    pub fn is_selected(&self) -> bool {
        self.selected
    }

    pub fn as_str(&self) -> &str {
        &self.text
    }

    pub fn trim(&self) -> &str {
        self.text.trim()
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    pub fn len(&self) -> usize {
        self.text.chars().count()
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// Inserts `text` at the cursor, as one line: a pasted line break becomes a space.
    pub fn insert_text(&mut self, text: &str) {
        if std::mem::take(&mut self.selected) {
            self.clear();
        }
        for ch in text.chars() {
            if ch == '\n' {
                self.insert(' ');
            } else if !ch.is_control() {
                self.insert(ch);
            }
        }
    }

    /// What a field `width` columns wide shows of the text, padded to that width, and
    /// the column the cursor is at in it. Counted in display columns, so a wide
    /// character neither pushes the cursor off nor overflows; the view starts only as
    /// far in as the cursor needs, so a cursor at the start shows the start.
    pub fn window(&self, width: usize) -> (String, usize) {
        use unicode_width::UnicodeWidthChar;
        let width = width.max(1);
        let chars: Vec<char> = self.text.chars().collect();
        let cursor = self.cursor.min(chars.len());
        let columns = |ch: &char| ch.width().unwrap_or(0);
        let mut start = 0;
        let mut at: usize = chars[..cursor].iter().map(columns).sum();
        while at >= width && start < cursor {
            at -= columns(&chars[start]);
            start += 1;
        }
        let mut shown = String::new();
        let mut used = 0;
        for ch in &chars[start..] {
            let wide = columns(ch);
            if used + wide > width {
                break;
            }
            shown.push(*ch);
            used += wide;
        }
        shown.push_str(&" ".repeat(width - used));
        (shown, at)
    }

    /// Whether `key` edits or moves in an input. A field whose keys would otherwise reach
    /// the keymap -- a bar, a prompt on the status line -- hands it these first, so Ctrl+A,
    /// Ctrl+W and the word keys act on the text rather than on the window around it.
    pub fn owns(key: &KeyEvent) -> bool {
        edit_for(key).is_some()
    }

    /// Applies `key` to the text; false for a key that is not an input's.
    pub fn handle_key(&mut self, key: KeyEvent) -> bool {
        let Some(edit) = edit_for(&key) else {
            return false;
        };
        if edit == Edit::SelectAll {
            self.select_all();
            return true;
        }
        if std::mem::take(&mut self.selected) {
            match edit {
                Edit::Insert(_) => self.clear(),
                Edit::DeleteBack
                | Edit::DeleteForward
                | Edit::DeleteWordBack
                | Edit::DeleteWordForward => {
                    self.clear();
                    return true;
                }
                Edit::Left | Edit::WordLeft | Edit::Home => {
                    self.cursor = 0;
                    return true;
                }
                Edit::Right | Edit::WordRight | Edit::End => {
                    self.cursor = self.len();
                    return true;
                }
                Edit::SelectAll => {}
            }
        }
        match edit {
            Edit::SelectAll => {}
            Edit::Insert(ch) => self.insert(ch),
            Edit::DeleteBack => self.backspace(),
            Edit::DeleteForward => self.delete(),
            Edit::DeleteWordBack => {
                let start = word_jump(&self.text, self.cursor, -1);
                self.remove(start, self.cursor);
            }
            Edit::DeleteWordForward => {
                let end = word_jump(&self.text, self.cursor, 1);
                self.remove(self.cursor, end);
            }
            Edit::Left => self.move_char(-1),
            Edit::Right => self.move_char(1),
            Edit::WordLeft => self.move_word(-1),
            Edit::WordRight => self.move_word(1),
            Edit::Home => self.cursor = 0,
            Edit::End => self.cursor = self.len(),
        }
        true
    }

    pub fn labeled_line(&self, label: &str, focused: bool) -> String {
        let marker = if focused { ">" } else { " " };
        format!("{marker} {label}{}", self.rendered_value(focused))
    }

    /// [`Self::labeled_line`] in `width` display columns, the marker and label included:
    /// the value scrolls with the cursor instead of running off the dialog.
    pub fn labeled_line_within(&self, label: &str, focused: bool, width: usize) -> String {
        let marker = if focused { ">" } else { " " };
        self.inline_line_within(&format!("{marker} {label}"), focused, width)
    }

    pub fn inline_line(&self, label: &str, focused: bool) -> String {
        format!("{label}{}", self.rendered_value(focused))
    }

    /// [`Self::inline_line`] in `width` display columns, label included: a long value
    /// scrolls with the cursor instead of running it off the field.
    pub fn inline_line_within(&self, label: &str, focused: bool, width: usize) -> String {
        let room = width.saturating_sub(unicode_width::UnicodeWidthStr::width(label));
        if !focused {
            return format!("{label}{}", crate::model::truncate_cell(&self.text, room));
        }
        // One column for the block cursor.
        let (shown, at) = self.window(room.saturating_sub(1).max(1));
        let mut chars: Vec<char> = Vec::new();
        let mut column = 0;
        let mut placed = false;
        for ch in shown.chars() {
            if !placed && column >= at {
                chars.push('█');
                placed = true;
            }
            chars.push(ch);
            column += unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        }
        if !placed {
            chars.push('█');
        }
        format!(
            "{label}{}",
            chars.into_iter().collect::<String>().trim_end()
        )
    }

    fn rendered_value(&self, show_cursor: bool) -> String {
        if !show_cursor {
            return self.text.clone();
        }
        let chars: Vec<char> = self.text.chars().collect();
        let cursor = self.cursor.min(chars.len());
        let mut out = String::new();
        out.extend(&chars[..cursor]);
        out.push('█');
        out.extend(&chars[cursor..]);
        out
    }

    fn insert(&mut self, ch: char) {
        let byte = char_byte_index(&self.text, self.cursor);
        self.text.insert(byte, ch);
        self.cursor += 1;
    }

    fn backspace(&mut self) {
        self.remove(self.cursor.saturating_sub(1), self.cursor);
    }

    fn delete(&mut self) {
        self.remove(self.cursor, self.cursor + 1);
    }

    /// Removes the characters from `start` to `end`, leaving the cursor where they were.
    fn remove(&mut self, start: usize, end: usize) {
        let end = end.min(self.len());
        if start >= end {
            return;
        }
        let range = char_byte_index(&self.text, start)..char_byte_index(&self.text, end);
        self.text.replace_range(range, "");
        self.cursor = start;
    }

    fn move_char(&mut self, delta: i32) {
        if delta < 0 {
            self.cursor = self.cursor.saturating_sub(1);
        } else {
            self.cursor = (self.cursor + 1).min(self.len());
        }
    }

    fn move_word(&mut self, delta: i32) {
        self.cursor = word_jump(&self.text, self.cursor, delta);
    }
}

impl From<&str> for TextInput {
    fn from(text: &str) -> Self {
        Self::new(text)
    }
}

impl From<String> for TextInput {
    fn from(text: String) -> Self {
        Self::new(text)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Edit {
    SelectAll,
    Insert(char),
    DeleteBack,
    DeleteForward,
    DeleteWordBack,
    DeleteWordForward,
    Left,
    Right,
    WordLeft,
    WordRight,
    Home,
    End,
}

/// What `key` does to an input. A letter typed with Ctrl or Alt is a shortcut, never
/// text; Alt with an arrow is left to the keymap, which moves between documents with it.
fn edit_for(key: &KeyEvent) -> Option<Edit> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let plain = key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT;
    Some(match key.code {
        KeyCode::Char('a') if key.modifiers == KeyModifiers::CONTROL => Edit::SelectAll,
        // The shell's word delete, beside Ctrl+Backspace and Alt+Backspace.
        KeyCode::Char('w') if key.modifiers == KeyModifiers::CONTROL => Edit::DeleteWordBack,
        KeyCode::Char(ch) if plain => Edit::Insert(ch),
        KeyCode::Backspace if ctrl || alt => Edit::DeleteWordBack,
        KeyCode::Backspace => Edit::DeleteBack,
        KeyCode::Delete if ctrl => Edit::DeleteWordForward,
        KeyCode::Delete if !alt => Edit::DeleteForward,
        KeyCode::Left if ctrl => Edit::WordLeft,
        KeyCode::Right if ctrl => Edit::WordRight,
        KeyCode::Left if !alt => Edit::Left,
        KeyCode::Right if !alt => Edit::Right,
        KeyCode::Home => Edit::Home,
        KeyCode::End => Edit::End,
        _ => return None,
    })
}

fn char_byte_index(text: &str, char_index: usize) -> usize {
    if char_index == 0 {
        return 0;
    }
    text.char_indices()
        .nth(char_index)
        .map(|(index, _)| index)
        .unwrap_or(text.len())
}

/// Where Ctrl+Left (`delta < 0`) or Ctrl+Right lands from `cursor`, in characters.
/// The text is walked a grapheme at a time, so an accent typed as a combining mark --
/// `e` then U+0301 -- stays with its letter instead of ending the word there.
fn word_jump(text: &str, cursor: usize, delta: i32) -> usize {
    use unicode_segmentation::UnicodeSegmentation;
    // Each grapheme's first character, and whether it is part of a word: `ç`, `ã` and
    // `é` are letters, where an ASCII test ended a word at each of them.
    let mut starts = Vec::new();
    let mut words = Vec::new();
    let mut chars = 0;
    for grapheme in text.graphemes(true) {
        starts.push(chars);
        words.push(
            grapheme
                .chars()
                .next()
                .is_some_and(|ch| ch.is_alphanumeric() || ch == '_'),
        );
        chars += grapheme.chars().count();
    }
    let len = words.len();
    let mut index = starts.partition_point(|start| *start < cursor);
    if delta < 0 {
        if index == 0 {
            return 0;
        }
        index -= 1;
        while index > 0 && !words[index] {
            index -= 1;
        }
        while index > 0 && words[index - 1] {
            index -= 1;
        }
    } else {
        while index < len && words[index] {
            index += 1;
        }
        while index < len && !words[index] {
            index += 1;
        }
    }
    starts.get(index).copied().unwrap_or(chars)
}

#[cfg(test)]
mod tests {
    use super::TextInput;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn ctrl(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::CONTROL)
    }

    #[test]
    fn inserts_and_deletes_around_cursor() {
        let mut input = TextInput::new("ab");
        input.handle_key(key(KeyCode::Left));
        input.handle_key(key(KeyCode::Char('x')));
        assert_eq!(input.as_str(), "axb");
        input.handle_key(key(KeyCode::Delete));
        assert_eq!(input.as_str(), "ax");
        input.handle_key(key(KeyCode::Backspace));
        assert_eq!(input.as_str(), "a");
        input.handle_key(key(KeyCode::Backspace));
        assert_eq!(input.as_str(), "");
    }

    #[test]
    fn home_end_and_word_motion() {
        let mut input = TextInput::new("query file");
        input.handle_key(key(KeyCode::Home));
        assert_eq!(input.cursor(), 0);
        input.handle_key(ctrl(KeyCode::Right));
        assert_eq!(input.cursor(), "query ".chars().count());
        input.handle_key(key(KeyCode::End));
        input.handle_key(ctrl(KeyCode::Left));
        assert_eq!(input.cursor(), "query ".chars().count());
    }

    #[test]
    fn focused_line_shows_block_cursor() {
        let mut input = TextInput::new("my-file.sql");
        input.handle_key(key(KeyCode::End));
        input.handle_key(ctrl(KeyCode::Left));
        assert_eq!(input.labeled_line("name:", true), "> name:my-file.█sql");
    }

    /// Ctrl+A did nothing in a single-line input.
    #[test]
    fn select_all_is_replaced_by_typing_and_cleared_by_backspace() {
        let mut input = TextInput::new("query-2.sql");
        input.handle_key(ctrl(KeyCode::Char('a')));
        assert!(input.is_selected());
        input.handle_key(key(KeyCode::Char('r')));
        input.handle_key(key(KeyCode::Char('x')));
        assert_eq!(input.as_str(), "rx");
        assert!(!input.is_selected());

        input.select_all();
        input.handle_key(key(KeyCode::Backspace));
        assert_eq!(input.as_str(), "");

        let mut input = TextInput::new("abc");
        input.select_all();
        input.handle_key(key(KeyCode::Left));
        assert_eq!((input.as_str(), input.cursor()), ("abc", 0));
        input.select_all();
        input.insert_text("pasted");
        assert_eq!(input.as_str(), "pasted");
        assert!(!TextInput::new("").is_selected());
    }

    #[test]
    fn a_word_keeps_its_accented_letters() {
        let mut input = TextInput::new("relatório mensal");
        input.handle_key(key(KeyCode::Home));
        input.handle_key(ctrl(KeyCode::Right));
        assert_eq!(input.cursor(), "relatório ".chars().count());
        input.handle_key(key(KeyCode::End));
        input.handle_key(ctrl(KeyCode::Left));
        input.handle_key(ctrl(KeyCode::Left));
        assert_eq!(input.cursor(), 0);
    }

    /// Ctrl+Backspace deleted one character; it, Alt+Backspace and Ctrl+W delete back to
    /// where Ctrl+Left lands, and Ctrl+Delete forward to where Ctrl+Right does.
    #[test]
    fn word_keys_delete_whole_words() {
        let alt = |code| KeyEvent::new(code, KeyModifiers::ALT);
        let mut input = TextInput::new("select relatório mensal");
        input.handle_key(ctrl(KeyCode::Backspace));
        assert_eq!(input.as_str(), "select relatório ");
        input.handle_key(alt(KeyCode::Backspace));
        assert_eq!(input.as_str(), "select ");
        input.handle_key(ctrl(KeyCode::Char('w')));
        assert_eq!((input.as_str(), input.cursor()), ("", 0));

        let mut input = TextInput::new("cafe\u{301}s com leite");
        input.handle_key(key(KeyCode::Home));
        input.handle_key(ctrl(KeyCode::Delete));
        assert_eq!((input.as_str(), input.cursor()), ("com leite", 0));
        input.handle_key(key(KeyCode::End));
        input.handle_key(ctrl(KeyCode::Delete));
        assert_eq!(input.as_str(), "com leite");

        input.select_all();
        input.handle_key(ctrl(KeyCode::Backspace));
        assert_eq!(input.as_str(), "");
    }

    /// A letter typed with Ctrl or Alt is a shortcut: the input neither takes it as
    /// text nor drops its selection for it.
    #[test]
    fn a_shortcut_is_not_text_and_keeps_the_selection() {
        let mut input = TextInput::new("abc");
        input.select_all();
        assert!(!input.handle_key(ctrl(KeyCode::Char('s'))));
        assert!(!input.handle_key(KeyEvent::new(KeyCode::Left, KeyModifiers::ALT)));
        assert!(input.is_selected());
        assert_eq!(input.as_str(), "abc");
        assert!(TextInput::owns(&ctrl(KeyCode::Char('w'))));
        assert!(!TextInput::owns(&ctrl(KeyCode::Char('p'))));
    }

    /// An accent typed as a combining mark (NFD) ended the word at it.
    #[test]
    fn a_word_keeps_its_combining_accents() {
        let mut input = TextInput::new("cafe\u{301}s x");
        input.handle_key(key(KeyCode::Home));
        input.handle_key(ctrl(KeyCode::Right));
        assert_eq!(input.cursor(), "cafe\u{301}s ".chars().count());
        input.handle_key(key(KeyCode::End));
        input.handle_key(ctrl(KeyCode::Left));
        input.handle_key(ctrl(KeyCode::Left));
        assert_eq!(input.cursor(), 0);

        let mut input = TextInput::new("cafe\u{301} x");
        input.handle_key(key(KeyCode::Home));
        input.handle_key(ctrl(KeyCode::Right));
        assert_eq!(input.cursor(), "cafe\u{301} ".chars().count());
        input.handle_key(ctrl(KeyCode::Left));
        assert_eq!(input.cursor(), 0);
    }

    #[test]
    fn ctrl_left_jumps_to_previous_word() {
        let mut input = TextInput::new("query-1.sql");
        input.handle_key(key(KeyCode::End));
        input.handle_key(ctrl(KeyCode::Left));
        assert_eq!(input.cursor(), "query-1.".chars().count());
    }
}

#[cfg(test)]
mod window_tests {
    use super::TextInput;

    /// The view counts display columns: wide characters keep the cursor on them, and
    /// the view moves only as far as the cursor needs.
    #[test]
    fn the_window_follows_the_cursor_in_display_columns() {
        let mut input = TextInput::new("日本語 name = 1");
        assert_eq!(input.window(8), ("ame = 1 ".to_string(), 7));
        for _ in 0..20 {
            input.handle_key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Left,
                crossterm::event::KeyModifiers::NONE,
            ));
        }
        assert_eq!(input.window(8), ("日本語 n".to_string(), 0));
        input.set_text("ab");
        assert_eq!(input.window(5), ("ab   ".to_string(), 2));
        let mut long = TextInput::new("abcdefghij");
        assert_eq!(long.window(4).1, 3);
        for _ in 0..10 {
            long.handle_key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Left,
                crossterm::event::KeyModifiers::NONE,
            ));
        }
        assert_eq!(long.window(4), ("abcd".to_string(), 0));
        let mut pasted = TextInput::default();
        pasted.insert_text("a = 1 and\nb = 2");
        assert_eq!(pasted.as_str(), "a = 1 and b = 2");
    }
}
