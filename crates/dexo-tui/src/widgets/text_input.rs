use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TextInput {
    text: String,
    cursor: usize,
}

impl TextInput {
    pub fn new(text: impl Into<String>) -> Self {
        let text = text.into();
        let cursor = text.chars().count();
        Self { text, cursor }
    }

    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
    }

    pub fn set_text(&mut self, text: impl Into<String>) {
        let text = text.into();
        self.cursor = text.chars().count();
        self.text = text;
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

    pub fn handle_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Left if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.move_word(-1);
                true
            }
            KeyCode::Right if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.move_word(1);
                true
            }
            KeyCode::Left => {
                self.move_char(-1);
                true
            }
            KeyCode::Right => {
                self.move_char(1);
                true
            }
            KeyCode::Home => {
                self.cursor = 0;
                true
            }
            KeyCode::End => {
                self.cursor = self.len();
                true
            }
            KeyCode::Backspace => {
                self.backspace();
                true
            }
            KeyCode::Delete => {
                self.delete();
                true
            }
            KeyCode::Char(ch)
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
            {
                self.insert(ch);
                true
            }
            _ => false,
        }
    }

    pub fn labeled_line(&self, label: &str, focused: bool) -> String {
        let marker = if focused { ">" } else { " " };
        format!("{marker} {label}{}", self.rendered_value(focused))
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
        if self.cursor == 0 {
            return;
        }
        let start = char_byte_index(&self.text, self.cursor - 1);
        let end = char_byte_index(&self.text, self.cursor);
        self.text.replace_range(start..end, "");
        self.cursor -= 1;
    }

    fn delete(&mut self) {
        if self.cursor >= self.len() {
            return;
        }
        let start = char_byte_index(&self.text, self.cursor);
        let end = char_byte_index(&self.text, self.cursor + 1);
        self.text.replace_range(start..end, "");
        self.cursor = self.cursor.min(self.len());
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

fn char_byte_index(text: &str, char_index: usize) -> usize {
    if char_index == 0 {
        return 0;
    }
    text.char_indices()
        .nth(char_index)
        .map(|(index, _)| index)
        .unwrap_or(text.len())
}

fn word_jump(text: &str, cursor: usize, delta: i32) -> usize {
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    let mut index = cursor.min(len);
    let is_word = |ch: char| ch.is_ascii_alphanumeric() || ch == '_';
    if delta < 0 {
        if index == 0 {
            return 0;
        }
        index -= 1;
        while index > 0 && !is_word(chars[index]) {
            index -= 1;
        }
        while index > 0 && is_word(chars[index - 1]) {
            index -= 1;
        }
        index
    } else {
        while index < len && is_word(chars[index]) {
            index += 1;
        }
        while index < len && !is_word(chars[index]) {
            index += 1;
        }
        index
    }
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
