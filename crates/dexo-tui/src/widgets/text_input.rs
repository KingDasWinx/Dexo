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
