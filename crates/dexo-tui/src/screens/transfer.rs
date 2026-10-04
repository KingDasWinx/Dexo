use std::time::Instant;

use dexo_app::transfer::{Detection, ErrorStrategy, ExportProgress, RejectedRow};

use crate::widgets::form::{FooterFocus, footer_line};
use crate::widgets::text_input::TextInput;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum TransferMode {
    #[default]
    Export,
    Import,
    Backup,
    Restore,
}

impl TransferMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Export => "export",
            Self::Import => "import",
            Self::Backup => "backup",
            Self::Restore => "restore",
        }
    }
}

/// The things a transfer dialog asks for; which of them a mode shows is `fields`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransferField {
    File,
    Format,
    /// The table an import writes into, or an SQL export inserts into.
    Table,
    OnError,
}

/// A question the dialog asks before it starts, with Replace or Restore and Cancel.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransferConfirm {
    /// The file already exists.
    Replace,
    /// A restore writes into the database.
    Restore,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TransferScreen {
    pub open: bool,
    pub mode: TransferMode,
    pub path: TextInput,
    pub format: String,
    /// Set once the person picks a format; until then the file's extension decides it.
    pub format_chosen: bool,
    pub table: TextInput,
    pub preview: Vec<String>,
    pub progress: ExportProgress,
    pub rejects: Vec<RejectedRow>,
    pub strategy: ErrorStrategy,
    pub running: bool,
    pub operation: Option<crate::runtime::OperationId>,
    pub error: Option<String>,
    pub message: Option<String>,
    /// The field that has the focus, as an index into `fields()`, while the footer is
    /// on `Input`.
    pub focus: usize,
    pub confirm: Option<TransferConfirm>,
    pub footer: FooterFocus,
    pub scroll: usize,
    /// What the dialog works on, said in its first line: the rows, or the database.
    pub subject: String,
    /// The rows that will be exported are the loaded part of a longer result.
    pub partial: bool,
    pub started: Option<Instant>,
}

impl Default for TransferScreen {
    fn default() -> Self {
        Self {
            open: false,
            mode: TransferMode::Export,
            path: Default::default(),
            format: "csv".into(),
            format_chosen: false,
            table: Default::default(),
            preview: Vec::new(),
            progress: ExportProgress { rows: 0, bytes: 0 },
            rejects: Vec::new(),
            strategy: ErrorStrategy::Stop,
            running: false,
            operation: None,
            error: None,
            message: None,
            focus: 0,
            confirm: None,
            footer: FooterFocus::Input,
            scroll: 0,
            subject: String::new(),
            partial: false,
            started: None,
        }
    }
}

/// What a line of the dialog is, for the mouse and the cursor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransferLineKind {
    Text,
    /// The field at this index of `fields()`.
    Field(usize),
    Footer,
}

/// The cursor of the focused text field, in the columns of its line.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Caret {
    /// The field's index in `fields()`.
    pub field: usize,
    pub column: usize,
    /// Columns of the text a selection covers, from the start of the value.
    pub selected: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TransferLine {
    pub kind: TransferLineKind,
    pub text: String,
}

/// Width the dialog's lines are cut to when no terminal says otherwise.
const DEFAULT_WIDTH: usize = 70;

/// The button at the end of the File line; Ctrl+O does the same.
pub const BROWSE: &str = " [Browse]";

/// Columns a field's line spends before its value: the marker and the label.
pub const FIELD_HEAD: usize = 12;

impl TransferScreen {
    pub fn sample_preview() -> Self {
        Self {
            open: true,
            mode: TransferMode::Import,
            path: "orders.csv".into(),
            format: "csv".into(),
            table: "orders".into(),
            subject: "local".into(),
            preview: vec!["id,name".into(), "1,ok".into(), "2,BAD".into()],
            strategy: ErrorStrategy::RejectFile,
            ..Self::default()
        }
    }

    pub fn sample_progress() -> Self {
        Self {
            open: true,
            mode: TransferMode::Export,
            path: "out.csv".into(),
            format: "csv".into(),
            subject: "10,000 rows from the results".into(),
            progress: ExportProgress {
                rows: 10_000,
                bytes: 80_000,
            },
            running: true,
            ..Self::default()
        }
    }

    pub fn sample_rejects() -> Self {
        Self {
            open: true,
            mode: TransferMode::Import,
            path: "orders.csv".into(),
            format: "csv".into(),
            table: "orders".into(),
            subject: "local".into(),
            preview: vec!["preview ready".into()],
            progress: ExportProgress { rows: 2, bytes: 0 },
            rejects: vec![RejectedRow {
                line: 3,
                safe_error: "invalid value".into(),
                original_fields: vec!["BAD".into()],
            }],
            strategy: ErrorStrategy::RejectFile,
            ..Self::default()
        }
    }

    pub fn from_detection(path: &str, detection: &Detection) -> Self {
        Self {
            open: true,
            mode: TransferMode::Import,
            path: path.into(),
            format: if detection.delimiter == b'\t' {
                "tsv"
            } else {
                "csv"
            }
            .into(),
            preview: detection.sample.iter().map(|row| row.join(",")).collect(),
            ..Self::default()
        }
    }

    pub fn title(&self) -> &'static str {
        match self.mode {
            TransferMode::Export => "Export data",
            TransferMode::Import => "Import data",
            TransferMode::Backup => "Native backup",
            TransferMode::Restore => "Native restore",
        }
    }

    /// The button that starts it, named for what it does.
    pub fn submit_label(&self) -> &'static str {
        match (self.confirm, self.mode) {
            (Some(TransferConfirm::Replace), _) => "Replace",
            (_, TransferMode::Export) => "Export",
            (_, TransferMode::Import) => "Import",
            (_, TransferMode::Backup) => "Back up",
            (_, TransferMode::Restore) => "Restore",
        }
    }

    /// The fields the mode asks for, in the order the arrows walk them.
    pub fn fields(&self) -> Vec<TransferField> {
        match self.mode {
            TransferMode::Export if self.format == "sql" => {
                vec![
                    TransferField::File,
                    TransferField::Format,
                    TransferField::Table,
                ]
            }
            TransferMode::Export => vec![TransferField::File, TransferField::Format],
            TransferMode::Import => vec![
                TransferField::File,
                TransferField::Format,
                TransferField::Table,
                TransferField::OnError,
            ],
            TransferMode::Backup | TransferMode::Restore => vec![TransferField::File],
        }
    }

    pub fn focused_field(&self) -> Option<TransferField> {
        (self.footer == FooterFocus::Input)
            .then(|| self.fields().get(self.focus).copied())
            .flatten()
    }

    /// Where the dialog starts from, for the mode it opens in.
    pub fn reset(&mut self, mode: TransferMode) {
        *self = Self {
            open: true,
            mode,
            ..Self::default()
        };
    }

    /// Follows the file's extension when the person has not picked a format.
    pub fn sync_format_with_path(&mut self) {
        if self.format_chosen {
            return;
        }
        if let Some(format) = format_of_extension(self.path.as_str()) {
            if self.mode == TransferMode::Import && format == "sql" {
                return;
            }
            self.format = format.into();
        }
    }

    /// The next format, or the previous one; an import never lands on SQL, which is a
    /// script to run and not data to read. The file's extension follows an export's.
    pub fn cycle_format(&mut self, forward: bool) {
        let all: &[&str] = if self.mode == TransferMode::Import {
            &["csv", "tsv", "json", "jsonl"]
        } else {
            &["csv", "tsv", "json", "jsonl", "sql"]
        };
        let at = all
            .iter()
            .position(|name| *name == self.format)
            .unwrap_or(0);
        let next = if forward {
            (at + 1) % all.len()
        } else {
            (at + all.len() - 1) % all.len()
        };
        self.format = all[next].into();
        self.format_chosen = true;
        if self.mode == TransferMode::Export && format_of_extension(self.path.as_str()).is_some() {
            let path =
                std::path::Path::new(self.path.as_str()).with_extension(self.format.as_str());
            self.path.set_text(path.display().to_string());
        }
    }

    pub fn cycle_strategy(&mut self, forward: bool) {
        use ErrorStrategy::{RejectFile, Skip, Stop};
        self.strategy = match (self.strategy, forward) {
            (Stop, true) | (RejectFile, false) => Skip,
            (Skip, true) | (Stop, false) => RejectFile,
            (RejectFile, true) | (Skip, false) => Stop,
        };
    }

    /// Columns the value of `field` may take on a line `width` wide.
    fn value_room(&self, field: TransferField, width: usize) -> usize {
        let room = width.saturating_sub(FIELD_HEAD).max(1);
        if field == TransferField::File {
            room.saturating_sub(BROWSE.chars().count()).max(1)
        } else {
            room
        }
    }

    /// Where the terminal's cursor goes in the focused text field of a dialog `width`
    /// wide, and how many columns of its text a selection covers.
    pub fn caret(&self, width: usize) -> Option<Caret> {
        let field = self.focused_field()?;
        let input = match field {
            TransferField::File => &self.path,
            TransferField::Table => &self.table,
            TransferField::Format | TransferField::OnError => return None,
        };
        let room = self.value_room(field, width);
        let (shown, at) = input.window(room);
        Some(Caret {
            field: self.focus,
            column: FIELD_HEAD + at,
            selected: if input.is_selected() {
                unicode_width::UnicodeWidthStr::width(shown.trim_end())
            } else {
                0
            },
        })
    }

    pub fn lines(&self) -> Vec<String> {
        self.layout(DEFAULT_WIDTH)
            .into_iter()
            .map(|line| line.text)
            .collect()
    }

    /// The dialog's lines cut to `width`, the footer last.
    pub fn layout(&self, width: usize) -> Vec<TransferLine> {
        let mut lines = Vec::new();
        let mut text = |text: String| {
            lines.push(TransferLine {
                kind: TransferLineKind::Text,
                text: cut(&text, width),
            })
        };
        if let Some(confirm) = self.confirm {
            let file = file_name(self.path.as_str());
            text(match confirm {
                TransferConfirm::Replace => format!("{file} exists already."),
                TransferConfirm::Restore => format!("Restore {file} into {}?", self.subject),
            });
            text(String::new());
            text(match confirm {
                TransferConfirm::Replace => {
                    "Its content is lost: the export takes its place.".to_string()
                }
                TransferConfirm::Restore => {
                    "The backup is written into that database; Dexo cannot undo it.".to_string()
                }
            });
            text(String::new());
            lines.push(TransferLine {
                kind: TransferLineKind::Footer,
                text: footer_line(self.submit_label(), self.footer),
            });
            return lines;
        }
        text(self.heading());
        text(String::new());
        let focused = self.focused_field();
        let fields = self.fields();
        let mut rows = Vec::new();
        for (index, field) in fields.iter().enumerate() {
            rows.push((index, *field, focused == Some(*field)));
        }
        for (index, field, is_focused) in rows {
            let label = match field {
                TransferField::File => "File",
                TransferField::Format => "Format",
                TransferField::Table => "Table",
                TransferField::OnError => "On error",
            };
            let head = format!(
                "{} {:<10}",
                if is_focused { ">" } else { " " },
                format!("{label}:")
            );
            let room = self.value_room(field, width);
            // A field with the focus shows the part of its text the cursor is in, and the
            // terminal's cursor is put there; the others show the end of theirs.
            let typed = |input: &TextInput| {
                if is_focused {
                    input.window(room).0
                } else {
                    format!("{:<room$}", fit_tail(input.as_str(), room))
                }
            };
            let value = match field {
                // The button sits at the end of the line, so the file's name gets what
                // is left of it.
                TransferField::File => format!("{}{BROWSE}", typed(&self.path)),
                TransferField::Table => typed(&self.table),
                TransferField::Format => {
                    if is_focused {
                        format!("{}   (Left/Right or Ctrl+F changes it)", self.format)
                    } else {
                        self.format.clone()
                    }
                }
                TransferField::OnError => {
                    let what = match self.strategy {
                        ErrorStrategy::Stop => "Stop at the first bad row",
                        ErrorStrategy::Skip => "Skip the bad rows",
                        ErrorStrategy::RejectFile => "Skip the bad rows, write them to a file",
                    };
                    if is_focused {
                        format!("{what}   (Left/Right)")
                    } else {
                        what.to_string()
                    }
                }
            };
            lines.push(TransferLine {
                kind: TransferLineKind::Field(index),
                text: cut(&format!("{head}{value}"), width),
            });
        }
        let mut plain = |text: String| {
            lines.push(TransferLine {
                kind: TransferLineKind::Text,
                text: cut(&text, width),
            })
        };
        if let Some(note) = self.note() {
            plain(String::new());
            for line in wrap(&note, width) {
                plain(line);
            }
        }
        let status = self.status();
        if !status.is_empty() {
            plain(String::new());
        }
        for line in status {
            for wrapped in wrap(&line, width) {
                plain(wrapped);
            }
        }
        if !self.preview.is_empty() {
            plain("preview:".into());
            for line in &self.preview {
                plain(line.clone());
            }
        }
        for reject in &self.rejects {
            plain(format!("Line {}: {}", reject.line, reject.safe_error));
        }
        lines.push(TransferLine {
            kind: TransferLineKind::Footer,
            text: if self.running {
                // Only a way out while it runs: starting again would write twice.
                " >[Cancel]".into()
            } else {
                footer_line(self.submit_label(), self.footer)
            },
        });
        lines
    }

    /// The first line: what the dialog does, to what.
    fn heading(&self) -> String {
        match self.mode {
            TransferMode::Export => format!("Export {} to a file.", self.subject),
            TransferMode::Import => format!(
                "Import a CSV, TSV, JSON or JSON Lines file into a table on {}.",
                self.subject
            ),
            TransferMode::Backup => format!("Back up {}.", self.subject),
            TransferMode::Restore => format!("Restore {} from a backup file.", self.subject),
        }
    }

    /// A line of what to expect, under the fields.
    fn note(&self) -> Option<String> {
        match self.mode {
            TransferMode::Export if self.partial => Some(
                "The result is longer than what is loaded: only the loaded rows are exported."
                    .into(),
            ),
            TransferMode::Export | TransferMode::Import => None,
            TransferMode::Backup => Some(
                "A name ending in .sql is written as a SQL script, any other as an archive.".into(),
            ),
            TransferMode::Restore => Some("Reads an archive (pg_dump -Fc) or a SQL script.".into()),
        }
    }

    /// What is happening or happened: the progress, the error, the message.
    fn status(&self) -> Vec<String> {
        let mut status = Vec::new();
        if self.running {
            let seconds = self
                .started
                .map_or(0, |started| started.elapsed().as_secs());
            let verb = match self.mode {
                TransferMode::Export => "Exporting",
                TransferMode::Import => "Importing",
                TransferMode::Backup => "Backing up",
                TransferMode::Restore => "Restoring",
            };
            let mut done = Vec::new();
            if self.progress.rows > 0 {
                done.push(rows_of(self.progress.rows));
            }
            if self.progress.bytes > 0 {
                done.push(size_of(self.progress.bytes));
            }
            status.push(format!(
                "{verb}... {}{seconds}s. Esc stops it.",
                if done.is_empty() {
                    String::new()
                } else {
                    format!("{}, ", done.join(", "))
                }
            ));
        }
        if let Some(error) = &self.error {
            status.push(error.clone());
        }
        if let Some(message) = &self.message {
            status.push(message.clone());
        }
        status
    }
}

/// `csv` for `a/b.CSV`, and so on; `None` for a name that says nothing.
pub fn format_of_extension(path: &str) -> Option<&'static str> {
    let extension = std::path::Path::new(path).extension()?.to_str()?;
    match extension.to_ascii_lowercase().as_str() {
        "csv" => Some("csv"),
        "tsv" | "tab" => Some("tsv"),
        "json" => Some("json"),
        "jsonl" | "ndjson" => Some("jsonl"),
        "sql" => Some("sql"),
        _ => None,
    }
}

/// `1 row`, `1,000 rows`.
pub fn rows_of(rows: u64) -> String {
    let digits = rows.to_string();
    let mut grouped = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    if rows == 1 {
        format!("{grouped} row")
    } else {
        format!("{grouped} rows")
    }
}

/// `132 bytes`, `45 KB`, `1.1 MB`.
pub fn size_of(bytes: u64) -> String {
    match bytes {
        0..1024 => format!("{bytes} bytes"),
        1024..1_048_576 => format!("{} KB", bytes / 1024),
        _ => {
            let tenths = bytes * 10 / 1_048_576;
            format!("{}.{} MB", tenths / 10, tenths % 10)
        }
    }
}

/// The name of the file in a path, for a sentence.
pub fn file_name(path: &str) -> String {
    std::path::Path::new(path).file_name().map_or_else(
        || path.to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// `text` in at most `width` characters, an ellipsis marking what was cut.
fn cut(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_string();
    }
    let mut out: String = text.chars().take(width.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// A path as long as `width` allows: the end of it, which is the part that tells files
/// apart, with an ellipsis for the start.
fn fit_tail(text: &str, width: usize) -> String {
    let count = text.chars().count();
    if count <= width {
        return text.to_string();
    }
    let tail: String = text.chars().skip(count - width.saturating_sub(1)).collect();
    format!("…{tail}")
}

/// `text` broken into lines of at most `width` characters, at spaces where it can.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    for paragraph in text.lines() {
        let mut line = String::new();
        for word in paragraph.split_whitespace() {
            let mut word = word.to_string();
            while word.chars().count() > width {
                let head: String = word.chars().take(width).collect();
                word = word.chars().skip(width).collect();
                if !line.is_empty() {
                    lines.push(std::mem::take(&mut line));
                }
                lines.push(head);
            }
            if line.is_empty() {
                line = word;
            } else if line.chars().count() + 1 + word.chars().count() <= width {
                line.push(' ');
                line.push_str(&word);
            } else {
                lines.push(std::mem::replace(&mut line, word));
            }
        }
        if !line.is_empty() {
            lines.push(line);
        }
    }
    // A tool's whole error output is not a line of a dialog.
    if lines.len() > 6 {
        lines.truncate(6);
        lines.push("...".into());
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::{
        TransferConfirm, TransferField, TransferMode, TransferScreen, fit_tail,
        format_of_extension, rows_of, size_of, wrap,
    };
    use crate::widgets::form::FooterFocus;

    #[test]
    fn preview_progress_and_rejects_render() {
        assert!(
            TransferScreen::sample_preview()
                .lines()
                .join("\n")
                .contains("preview")
        );
        assert!(
            TransferScreen::sample_progress()
                .lines()
                .join("\n")
                .contains("10,000 rows")
        );
        assert!(
            TransferScreen::sample_rejects()
                .lines()
                .join("\n")
                .contains("Line 3: invalid value")
        );
    }

    /// The dialog says what it is, in words: labelled fields and no `key=value`.
    #[test]
    fn the_dialog_shows_labelled_fields_and_no_internal_text() {
        let mut export = TransferScreen::default();
        export.reset(TransferMode::Export);
        export.subject = "1 row from the results".into();
        export.path.set_text("/tmp/out.csv");
        let text = export.lines().join("\n");
        assert!(
            text.starts_with("Export 1 row from the results to a file."),
            "{text}"
        );
        for want in ["> File:", "  Format:   csv", "[Export]", "[Cancel]"] {
            assert!(text.contains(want), "{want} missing in\n{text}");
        }
        for never in ["running=", "strategy=", "bytes=", "progress rows"] {
            assert!(!text.contains(never), "{never} in\n{text}");
        }

        let mut import = TransferScreen::default();
        import.reset(TransferMode::Import);
        import.subject = "pg-dev".into();
        let text = import.lines().join("\n");
        for want in [
            "File:",
            "Format:",
            "Table:",
            "On error: Stop at the first bad row",
            "[Import]",
        ] {
            assert!(text.contains(want), "{want} missing in\n{text}");
        }
        // A backup or a restore has a file and nothing else to fill in.
        let mut backup = TransferScreen::default();
        backup.reset(TransferMode::Backup);
        assert_eq!(backup.fields(), vec![TransferField::File]);
        assert!(!backup.lines().join("\n").contains("Format:"));
    }

    #[test]
    fn a_running_transfer_offers_only_a_way_out_and_says_how_to_stop() {
        let mut screen = TransferScreen::sample_progress();
        screen.progress.bytes = 2048;
        let text = screen.lines().join("\n");
        assert!(text.contains("Exporting... 10,000 rows, 2 KB"), "{text}");
        assert!(text.contains("Esc stops it"), "{text}");
        let footer = screen.lines().last().cloned().unwrap();
        assert!(
            footer.contains("[Cancel]") && !footer.contains("Export"),
            "{footer}"
        );
    }

    #[test]
    fn the_extension_picks_the_format_until_the_person_does() {
        let mut screen = TransferScreen::default();
        screen.reset(TransferMode::Export);
        screen.path.set_text("out.TSV");
        screen.sync_format_with_path();
        assert_eq!(screen.format, "tsv");
        screen.cycle_format(true);
        assert_eq!(screen.format, "json");
        // The name follows an export's format, so file and content agree.
        assert_eq!(screen.path.as_str(), "out.json");
        screen.path.set_text("out.csv");
        screen.sync_format_with_path();
        assert_eq!(screen.format, "json", "the person's pick is not overruled");
        assert_eq!(format_of_extension("a/b.ndjson"), Some("jsonl"));
        assert_eq!(format_of_extension("noext"), None);
    }

    #[test]
    fn an_import_never_lands_on_sql() {
        let mut screen = TransferScreen::default();
        screen.reset(TransferMode::Import);
        for _ in 0..8 {
            screen.cycle_format(true);
            assert_ne!(screen.format, "sql");
        }
        screen.path.set_text("dump.sql");
        screen.format_chosen = false;
        screen.format = "csv".into();
        screen.sync_format_with_path();
        assert_eq!(screen.format, "csv");
    }

    #[test]
    fn a_replace_question_names_the_file() {
        let mut screen = TransferScreen::default();
        screen.reset(TransferMode::Export);
        screen.path.set_text("/tmp/precious.txt");
        screen.confirm = Some(TransferConfirm::Replace);
        screen.footer = FooterFocus::Cancel;
        let text = screen.lines().join("\n");
        assert!(text.contains("precious.txt exists already."), "{text}");
        assert!(
            text.contains("[Replace]") && text.contains(">[Cancel]"),
            "{text}"
        );
    }

    #[test]
    fn numbers_and_text_are_made_for_reading() {
        assert_eq!(rows_of(1), "1 row");
        assert_eq!(rows_of(10_000), "10,000 rows");
        assert_eq!(size_of(132), "132 bytes");
        assert_eq!(size_of(1_153_434), "1.1 MB");
        assert_eq!(fit_tail("/a/very/long/folder/name.csv", 12), "…er/name.csv");
        let wrapped = wrap("pg_restore: error: could not execute query", 20);
        assert!(
            wrapped.iter().all(|line| line.chars().count() <= 20),
            "{wrapped:?}"
        );
    }
}
