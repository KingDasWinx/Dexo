use std::ops::Range;

use thiserror::Error;

#[derive(Debug, Error, Eq, PartialEq)]
pub enum SqlError {
    #[error("edit range is invalid")]
    InvalidRange,
    #[error("range is not on a char boundary")]
    NotCharBoundary,
    #[error("format would change SQL tokens")]
    FormatUnsafe,
}

#[derive(Clone, Debug)]
struct Inverse {
    start: usize,
    inserted: String,
    deleted: String,
}

#[derive(Clone, Debug, Default)]
pub struct SqlDocument {
    rope: ropey::Rope,
    revision: u64,
    undo: Vec<Vec<Inverse>>,
    redo: Vec<Vec<Inverse>>,
    open_group: bool,
    cursor: usize,
    selection: Option<Range<usize>>,
}

impl SqlDocument {
    pub fn new(text: impl AsRef<str>) -> Self {
        let rope = ropey::Rope::from_str(text.as_ref());
        let cursor = rope.len_chars();
        Self {
            rope,
            revision: 0,
            undo: Vec::new(),
            redo: Vec::new(),
            open_group: false,
            cursor,
            selection: None,
        }
    }

    pub fn text(&self) -> String {
        self.rope.to_string()
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn selection(&self) -> Option<Range<usize>> {
        self.selection.clone()
    }

    pub fn set_cursor(&mut self, index: usize) -> Result<(), SqlError> {
        if index > self.rope.len_chars() {
            return Err(SqlError::InvalidRange);
        }
        self.cursor = index;
        self.selection = None;
        Ok(())
    }

    pub fn set_selection(&mut self, range: Range<usize>) -> Result<(), SqlError> {
        self.validate_char_range(&range)?;
        self.cursor = range.end;
        self.selection = Some(range);
        Ok(())
    }

    pub fn begin_group(&mut self) {
        if !self.open_group {
            self.undo.push(Vec::new());
            self.open_group = true;
        }
    }

    pub fn end_group(&mut self) {
        if self.open_group {
            if self.undo.last().is_some_and(|group| group.is_empty()) {
                self.undo.pop();
            }
            self.open_group = false;
        }
    }

    pub fn replace_chars(&mut self, range: Range<usize>, text: &str) -> Result<(), SqlError> {
        self.validate_char_range(&range)?;
        let deleted = self.rope.slice(range.clone()).to_string();
        self.rope.remove(range.clone());
        self.rope.insert(range.start, text);
        let inverse = Inverse {
            start: range.start,
            inserted: text.to_string(),
            deleted,
        };
        self.push_undo(inverse);
        self.redo.clear();
        self.revision += 1;
        self.cursor = range.start + text.chars().count();
        self.selection = None;
        Ok(())
    }

    pub fn replace_bytes(&mut self, range: Range<usize>, text: &str) -> Result<(), SqlError> {
        let bytes = self.rope.len_bytes();
        if range.end < range.start || range.end > bytes {
            return Err(SqlError::InvalidRange);
        }
        let contents = self.text();
        if !contents.is_char_boundary(range.start) || !contents.is_char_boundary(range.end) {
            return Err(SqlError::NotCharBoundary);
        }
        let start = self.rope.byte_to_char(range.start);
        let end = self.rope.byte_to_char(range.end);
        self.replace_chars(start..end, text)
    }

    pub fn insert(&mut self, index: usize, text: &str) -> Result<(), SqlError> {
        self.replace_chars(index..index, text)
    }

    pub fn delete(&mut self, range: Range<usize>) -> Result<(), SqlError> {
        self.replace_chars(range, "")
    }

    pub fn undo(&mut self) -> Result<(), SqlError> {
        let Some(group) = self.undo.pop() else {
            return Ok(());
        };
        for inverse in group.iter().rev() {
            let end = inverse.start + inverse.inserted.chars().count();
            self.rope.remove(inverse.start..end);
            self.rope.insert(inverse.start, &inverse.deleted);
            self.cursor = inverse.start + inverse.deleted.chars().count();
        }
        self.redo.push(group);
        self.revision += 1;
        self.selection = None;
        Ok(())
    }

    pub fn redo(&mut self) -> Result<(), SqlError> {
        let Some(group) = self.redo.pop() else {
            return Ok(());
        };
        for inverse in &group {
            let end = inverse.start + inverse.deleted.chars().count();
            self.rope.remove(inverse.start..end);
            self.rope.insert(inverse.start, &inverse.inserted);
            self.cursor = inverse.start + inverse.inserted.chars().count();
        }
        self.undo.push(group);
        self.revision += 1;
        self.selection = None;
        Ok(())
    }

    fn push_undo(&mut self, inverse: Inverse) {
        if self.open_group {
            self.undo.last_mut().expect("open undo group").push(inverse);
        } else {
            self.undo.push(vec![inverse]);
        }
    }

    fn validate_char_range(&self, range: &Range<usize>) -> Result<(), SqlError> {
        if range.end < range.start || range.end > self.rope.len_chars() {
            return Err(SqlError::InvalidRange);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{SqlDocument, SqlError};

    #[test]
    fn unicode_edit_undo_redo_is_lossless() {
        let mut doc = SqlDocument::new("select 'ação'\n");
        doc.replace_chars(8..12, "café").unwrap();
        assert_eq!(doc.text(), "select 'café'\n");
        doc.undo().unwrap();
        assert_eq!(doc.text(), "select 'ação'\n");
        doc.redo().unwrap();
        assert_eq!(doc.text(), "select 'café'\n");
    }

    #[test]
    fn byte_range_must_be_char_boundary() {
        let mut doc = SqlDocument::new("ação");
        let err = doc.replace_bytes(1..2, "x").unwrap_err();
        assert_eq!(err, SqlError::NotCharBoundary);
        assert_eq!(doc.text(), "ação");
    }

    #[test]
    fn revision_increases_monotonically() {
        let mut doc = SqlDocument::new("ab");
        let start = doc.revision();
        doc.insert(2, "c").unwrap();
        doc.undo().unwrap();
        assert!(doc.revision() > start);
    }
}

#[cfg(test)]
mod proptests {
    use super::SqlDocument;

    proptest::proptest! {
        #[test]
        fn insert_delete_undo_matches_string_model(
            seed in "[a-zà-ü]{0,16}",
            inserts in proptest::collection::vec(("[a-zà-ü]{0,8}", 0..20usize), 0..8)
        ) {
            let mut doc = SqlDocument::new(&seed);
            let mut model = seed.clone();
            let mut snapshots = vec![model.clone()];
            for (text, at) in inserts {
                let at = at.min(model.chars().count());
                doc.insert(at, &text).unwrap();
                let mut chars: Vec<char> = model.chars().collect();
                let insert_chars: Vec<char> = text.chars().collect();
                chars.splice(at..at, insert_chars);
                model = chars.into_iter().collect();
                snapshots.push(model.clone());
                assert_eq!(doc.text(), model);
            }
            while snapshots.len() > 1 {
                snapshots.pop();
                doc.undo().unwrap();
                assert_eq!(doc.text(), *snapshots.last().unwrap());
            }
        }
    }
}
