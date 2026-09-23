use crate::mouse::{HitMap, HitTarget, register_label};
use crate::screens::schema_editor::{FormField, SchemaEditor};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum FooterFocus {
    #[default]
    Input,
    Submit,
    Cancel,
}

impl FooterFocus {
    pub fn next(self) -> Self {
        match self {
            Self::Input => Self::Submit,
            Self::Submit => Self::Cancel,
            Self::Cancel => Self::Input,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            Self::Input => Self::Cancel,
            Self::Submit => Self::Input,
            Self::Cancel => Self::Submit,
        }
    }
}

/// What a key meant to a dialog footer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FooterKey {
    Submit,
    Cancel,
    /// The focus moved between the input and the two buttons.
    Moved,
    /// Not a footer key. The dialog's own handling decides -- typing, usually.
    Pass,
}

/// The keys every Submit/Cancel dialog answers alike.
///
/// Esc cancels from anywhere. Tab and Down walk input -> Submit -> Cancel, BackTab and
/// Up walk back. Left and Right step between the two buttons once one of them has the
/// focus; on the input they belong to the text. Enter submits, unless Cancel is the
/// focused button.
///
/// Each dialog used to spell this out for itself, with Tab and BackTab only, so the
/// arrows did nothing on six of the eight and the seventh forgot Left and Right.
pub fn footer_key(focus: &mut FooterFocus, key: &KeyEvent) -> FooterKey {
    match key.code {
        KeyCode::Esc => FooterKey::Cancel,
        KeyCode::Tab | KeyCode::Down => {
            *focus = focus.next();
            FooterKey::Moved
        }
        KeyCode::BackTab | KeyCode::Up => {
            *focus = focus.prev();
            FooterKey::Moved
        }
        KeyCode::Left | KeyCode::Right if *focus != FooterFocus::Input => {
            *focus = match *focus {
                FooterFocus::Submit => FooterFocus::Cancel,
                _ => FooterFocus::Submit,
            };
            FooterKey::Moved
        }
        KeyCode::Enter if *focus == FooterFocus::Cancel => FooterKey::Cancel,
        KeyCode::Enter => FooterKey::Submit,
        _ => FooterKey::Pass,
    }
}

pub fn footer_line(submit: &str, focus: FooterFocus) -> String {
    format!(
        "{}[{submit}]  {}[Cancel]",
        if focus == FooterFocus::Submit {
            ">"
        } else {
            " "
        },
        if focus == FooterFocus::Cancel {
            ">"
        } else {
            " "
        },
    )
}

pub fn register_footer(hits: &mut HitMap, line: Rect, text: &str, submit: &str) {
    let submit_label = format!("[{submit}]");
    register_label(hits, line, text, &submit_label, HitTarget::FooterSubmit);
    register_label(hits, line, text, "[Cancel]", HitTarget::FooterCancel);
}

pub fn render_lines(editor: &SchemaEditor) -> Vec<String> {
    editor.lines()
}

pub fn focused_field(editor: &SchemaEditor) -> Option<&FormField> {
    editor.fields.get(editor.focus)
}

#[cfg(test)]
mod tests {
    use super::{FooterFocus, focused_field, footer_line, render_lines};
    use crate::screens::schema_editor::SchemaEditor;

    #[test]
    fn footer_marks_the_focused_action() {
        assert_eq!(
            footer_line("Submit", FooterFocus::Input),
            " [Submit]   [Cancel]"
        );
        assert_eq!(
            footer_line("Submit", FooterFocus::Submit),
            ">[Submit]   [Cancel]"
        );
        assert_eq!(
            footer_line("Save", FooterFocus::Cancel),
            " [Save]  >[Cancel]"
        );
    }

    #[test]
    fn focus_stays_on_selected_field() {
        let mut editor = SchemaEditor::table_form("public.t");
        editor.focus = 1;
        assert_eq!(focused_field(&editor).unwrap().label, "columns");
        assert!(
            render_lines(&editor)
                .iter()
                .any(|line| line.starts_with("> columns:"))
        );
        editor.focus_prev();
        assert_eq!(focused_field(&editor).unwrap().label, "target");
    }
}
