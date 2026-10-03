use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use dexo_driver_api::DbValue;
use dexo_sql::{
    CompletionItem, Dialect, FakeCatalog, HighlightSpan, HistoryPolicy, ParserService, Snippet,
    complete_with, format_sql, named_parameters,
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
    /// The document and revision `highlights` were built for. Anything else on screen
    /// -- another tab, a file that just loaded -- means they belong to other text.
    painted: Option<(String, u64)>,
    /// Document, revision, and cursor the open popup was computed for. When any of them
    /// moves without the popup being recomputed, it is answering a question nobody is
    /// asking any more.
    completion_at: Option<(String, u64, usize)>,
    pub highlights: Vec<HighlightSpan>,
    /// What the document has wrong, built with the highlights and current with them.
    pub diagnostics: Vec<dexo_sql::Diagnostic>,
    /// Where the server said the last run failed, in the document and revision it ran.
    pub server_diagnostic: Option<(String, u64, dexo_sql::Diagnostic)>,
    /// The catalog as the diagnostics read it, and the catalog revision and the session
    /// tables (see `session_tables_key`) it was built with.
    known: Option<((u64, u64), dexo_sql::KnownObjects)>,
    /// Keeps each statement's diagnostics until it changes.
    diagnoser: dexo_sql::Diagnoser,
    /// The document, revision and cursor the last pause tick saw, to tell a pause from
    /// typing: see [`settle_diagnostics`].
    settle_mark: Option<(String, u64, usize)>,
    pub parameters: Vec<ParameterValue>,
    pub completions: Vec<CompletionItem>,
    pub completion_open: bool,
    pub completion_selected: usize,
    pub completion_offset: usize,
    pub parameter_prompt: bool,
    pub parameter_index: usize,
    pub parameter_draft: crate::widgets::text_input::TextInput,
    pub parameter_footer: crate::widgets::form::FooterFocus,
    pub snippets: Vec<Snippet>,
    pub snippet_open: bool,
    pub snippet_selected: usize,
    pub snippet_pending: bool,
    /// Every connection's runs, newest first.
    pub history: Vec<dexo_storage::HistoryRow>,
    /// The pick among `history_lines`.
    pub history_selected: usize,
    pub history_confirm_clear: bool,
    /// What the clear confirmation asks about when History's Clear asked it: how many
    /// statements are shown, and the ids of their runs. None is the palette's Clear
    /// History, of the connection in use.
    pub history_clearing: Option<(usize, Vec<String>)>,
    /// The focus of the clear confirmation's two buttons.
    pub history_footer: crate::widgets::form::FooterFocus,
    /// What the history list is narrowed to: a text in the statement, a connection, how
    /// the run went.
    pub history_search: crate::screen::widgets::Search,
    pub history_connection: Option<String>,
    pub history_status: StatusFilter,
    pub history_policy: HistoryPolicy,
    catalog: FakeCatalog,
    /// The completion catalog, and the catalog and explorer revisions it was built from.
    /// Building it walks and clones every object the connection has loaded, so doing it
    /// for every character typed is a cost the editor cannot afford.
    catalog_key: Option<(u64, u64, String)>,
    catalog_snapshot: Option<dexo_app::SnapshotCatalog>,
    /// The bytes the open popup's items would replace, and whether accepting a table
    /// there should bring an alias with it. Both come from the analysis that built the
    /// list, so accepting cannot disagree with it about what is being replaced.
    completion_replace: std::ops::Range<usize>,
    /// The holes an inserted snippet left behind, in characters, and which one the
    /// cursor is on. Tab walks forward through them and Shift+Tab back.
    snippet_stops: Vec<std::ops::Range<usize>>,
    snippet_stop: usize,
    /// A search the popup would like answered from the catalog snapshot. Drained by
    /// `update`, which is the only place that can turn it into an effect.
    completion_request: Option<(String, u64, String)>,
    /// Tables whose columns the statement needs and nothing in memory has, waiting to be
    /// asked of the session; and every table already asked about, by generation and
    /// lowercased name, so a table that has no columns is asked once, not per key.
    columns_pending: Vec<dexo_driver_api::QualifiedName>,
    columns_asked: std::collections::HashSet<String>,
    /// Document, cursor, and revision the view was last scrolled for. The view follows
    /// the cursor only when one of them moves, so the wheel can look elsewhere.
    followed: Option<(String, usize, u64)>,
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
            painted: self.painted.clone(),
            completion_at: self.completion_at.clone(),
            highlights: self.highlights.clone(),
            diagnostics: self.diagnostics.clone(),
            server_diagnostic: self.server_diagnostic.clone(),
            known: self.known.clone(),
            diagnoser: self.diagnoser.clone(),
            settle_mark: self.settle_mark.clone(),
            parameters: self.parameters.clone(),
            completions: self.completions.clone(),
            completion_open: self.completion_open,
            completion_selected: self.completion_selected,
            completion_offset: self.completion_offset,
            parameter_prompt: self.parameter_prompt,
            parameter_index: self.parameter_index,
            parameter_draft: self.parameter_draft.clone(),
            parameter_footer: self.parameter_footer,
            snippets: self.snippets.clone(),
            snippet_open: self.snippet_open,
            snippet_selected: self.snippet_selected,
            snippet_pending: self.snippet_pending,
            history: self.history.clone(),
            history_selected: self.history_selected,
            history_confirm_clear: self.history_confirm_clear,
            history_clearing: self.history_clearing.clone(),
            history_footer: self.history_footer,
            history_search: self.history_search.clone(),
            history_connection: self.history_connection.clone(),
            history_status: self.history_status,
            history_policy: self.history_policy,
            catalog: self.catalog.clone(),
            catalog_key: None,
            catalog_snapshot: None,
            completion_replace: 0..0,
            snippet_stops: Vec::new(),
            snippet_stop: 0,
            completion_request: None,
            columns_pending: Vec::new(),
            columns_asked: std::collections::HashSet::new(),
            followed: None,
        }
    }
}

impl PartialEq for EditorState {
    fn eq(&self, other: &Self) -> bool {
        self.highlights == other.highlights
            && self.parameters == other.parameters
            && self.completions == other.completions
            && self.snippets == other.snippets
            && self.history == other.history
    }
}

impl EditorState {
    pub fn reset_parse(&mut self) {
        self.last_sql.clear();
    }

    /// Whether a syntax error is being kept from showing because the cursor is by it. The
    /// pause tick runs only while this holds.
    pub fn hides_errors(&self) -> bool {
        self.diagnoser.is_hiding()
    }
}

impl Default for EditorState {
    fn default() -> Self {
        Self {
            parser: ParserService::postgres(),
            last_sql: String::new(),
            painted: None,
            completion_at: None,
            highlights: Vec::new(),
            diagnostics: Vec::new(),
            server_diagnostic: None,
            known: None,
            diagnoser: dexo_sql::Diagnoser::default(),
            settle_mark: None,
            parameters: Vec::new(),
            completions: Vec::new(),
            completion_open: false,
            completion_selected: 0,
            completion_offset: 0,
            parameter_prompt: false,
            parameter_index: 0,
            parameter_draft: Default::default(),
            parameter_footer: crate::widgets::form::FooterFocus::Input,
            snippets: Vec::new(),
            snippet_open: false,
            snippet_selected: 0,
            snippet_pending: false,
            history: Vec::new(),
            history_selected: 0,
            history_confirm_clear: false,
            history_clearing: None,
            history_footer: crate::widgets::form::FooterFocus::Cancel,
            history_search: Default::default(),
            history_connection: None,
            history_status: StatusFilter::All,
            history_policy: HistoryPolicy::SqlOnly,
            catalog: FakeCatalog::default(),
            catalog_key: None,
            catalog_snapshot: None,
            completion_replace: 0..0,
            snippet_stops: Vec::new(),
            snippet_stop: 0,
            completion_request: None,
            columns_pending: Vec::new(),
            columns_asked: std::collections::HashSet::new(),
            followed: None,
        }
    }
}

pub(crate) fn editor_dialect(model: &Model) -> Dialect {
    dexo_app::dialect_for_driver(&model.connection.driver)
}

/// Rows and columns of text the editor pane shows, from the layout the frame is drawn
/// with.
fn text_area(model: &Model) -> Option<(usize, usize)> {
    if model.width == 0 || model.height == 0 {
        return None;
    }
    let plan = crate::layout::LayoutPlan::for_area_with_document_tabs(
        ratatui::layout::Rect::new(0, 0, model.width, model.height),
        Some(&model.effective_panes()),
        true,
    );
    let inner = ratatui::widgets::Block::bordered().inner(plan.content);
    let rows = (inner.height as usize).saturating_sub(crate::screens::find::bar_rows(model));
    let cols = inner.width.saturating_sub(crate::widgets::editor::GUTTER) as usize;
    (rows > 0 && cols > 0).then_some((rows, cols))
}

/// Scrolls just far enough to keep the cursor on screen, and only once it reaches an
/// edge: the arrows walk to the last row or column before the text moves, as in any
/// editor, and the view moves a row or a column at a time either way. A line that fits
/// in the pane shows from its first column. It used to assume a pane 12 rows by 80
/// columns, so a taller one started scrolling halfway down.
pub fn follow_cursor(model: &mut Model) {
    let doc = model.active_document();
    if doc.kind.is_table() || doc.kind.is_placeholder() {
        return;
    }
    let key = (doc.id.clone(), doc.cursor(), doc.sql.revision());
    if model.editor.followed.as_ref() == Some(&key) {
        return;
    }
    let Some((rows, cols)) = text_area(model) else {
        return;
    };
    model.editor.followed = Some(key);
    let doc = model.active_document_mut();
    let text = doc.sql.text();
    let (line, col) = line_col(&text, doc.sql.cursor());
    if line < doc.viewport_line {
        doc.viewport_line = line;
    } else if line >= doc.viewport_line + rows {
        doc.viewport_line = line + 1 - rows;
    }
    // The view scrolls in screen columns, which a wide character takes two of.
    let columns = |text: &str, chars: usize| -> usize {
        text.chars()
            .take(chars)
            .map(|ch| unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0))
            .sum()
    };
    let current = text.split('\n').nth(line).unwrap_or("");
    let x = columns(current, col);
    // A long line deleted back to a short one kept its start out of view, scrolled
    // back only as far as the cursor.
    if columns(current, usize::MAX) < cols {
        doc.viewport_column = 0;
    } else if x < doc.viewport_column {
        doc.viewport_column = x;
    } else if x >= doc.viewport_column + cols {
        doc.viewport_column = x + 1 - cols;
    }
}

/// Whether the highlights on screen were built for the text the active document holds,
/// and no error left out for the cursor is away from it now.
pub fn highlights_are_current(model: &Model) -> bool {
    let document = model.active_document();
    model
        .editor
        .painted
        .as_ref()
        .is_some_and(|(id, revision)| *id == document.id && *revision == document.sql.revision())
        && !model
            .editor
            .diagnoser
            .hidden_away_from(document.byte_cursor())
}

pub fn refresh_intelligence(model: &mut Model, with_completion: bool) {
    let sql = model.active_document().text();
    let byte_cursor = model.active_document().byte_cursor();
    let old = std::mem::take(&mut model.editor.last_sql);
    let parsed = model.editor.parser.parse_edited(&old, &sql);
    model.editor.last_sql = sql.clone();
    model.editor.highlights = parsed.highlights;
    let document = model.active_document();
    model.editor.painted = Some((document.id.clone(), document.sql.revision()));
    refresh_diagnostics(model, &sql, byte_cursor);
    model.editor.parameters = named_parameters(&sql, editor_dialect(model))
        .into_iter()
        .map(|parameter| ParameterValue {
            sensitive: is_sensitive_name(&parameter.name),
            name: parameter.name,
            value: DbValue::Null,
        })
        .collect();
    if with_completion {
        apply_completions(model, &sql, byte_cursor, false);
    }
}

pub(crate) fn session_tables(model: &Model) -> Vec<&String> {
    let (session, generation, tables) = &model.session_tables;
    if *session == model.active_session && *generation == model.session_generation {
        let mut tables: Vec<&String> = tables.iter().collect();
        tables.sort();
        tables
    } else {
        Vec::new()
    }
}

/// What the known objects were built from on the session's side: its tables, by name --
/// a count alone missed a table dropped and another created.
fn session_tables_key(model: &Model) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    session_tables(model).hash(&mut hasher);
    hasher.finish()
}

/// Underlines what the document has wrong: what does not parse always, and tables and
/// columns the catalog does not list once it has the whole database. A script too large
/// to check on every key is left alone.
pub fn refresh_diagnostics(model: &mut Model, sql: &str, byte_cursor: usize) {
    const LARGEST: usize = 512 * 1024;
    if sql.len() > LARGEST {
        model.editor.diagnostics.clear();
        return;
    }
    let complete = model.catalog_complete && model.catalog_connection == model.connection.name;
    if !complete {
        model.editor.known = None;
    } else if model.editor.known.as_ref().map(|(built, _)| *built)
        != Some((model.catalog_revision, session_tables_key(model)))
    {
        let mut known = dexo_app::catalog_service::known_objects(&model.catalog_objects);
        for table in session_tables(model) {
            known.add_table("", table);
        }
        model.editor.known = Some(((model.catalog_revision, session_tables_key(model)), known));
    }
    let dialect = editor_dialect(model);
    let editor = &mut model.editor;
    let known = editor.known.as_ref().map(|(_, known)| known);
    editor.diagnostics = editor.diagnoser.diagnose(sql, dialect, known, byte_cursor);
}

/// A syntax error by the cursor is not shown while typing, the statement may just not be
/// finished. When two ticks in a row find the document and the cursor as they were, it
/// is finished as far as the user goes, and the error shows: `selec 1` was left plain
/// however long it sat.
pub fn settle_diagnostics(model: &mut Model) {
    if !model.editor.hides_errors() || model.active_document().kind.is_table() {
        model.editor.settle_mark = None;
        return;
    }
    let doc = model.active_document();
    let mark = (doc.id.clone(), doc.sql.revision(), doc.cursor());
    if model.editor.settle_mark.as_ref() != Some(&mark) {
        model.editor.settle_mark = Some(mark);
        return;
    }
    // A cursor nowhere in the document: nothing is being typed.
    let sql = model.active_document().text();
    refresh_diagnostics(model, &sql, usize::MAX);
    model.editor.settle_mark = None;
}

/// The diagnostics on screen for the active document: its own, and the server's from
/// the last run while the text is as it ran. Byte ranges.
pub fn current_diagnostics(model: &Model) -> Vec<&dexo_sql::Diagnostic> {
    if !highlights_are_current(model) {
        return Vec::new();
    }
    let doc = model.active_document();
    let server = model
        .editor
        .server_diagnostic
        .as_ref()
        .filter(|(document, revision, _)| *document == doc.id && *revision == doc.sql.revision())
        .map(|(_, _, diagnostic)| diagnostic);
    model.editor.diagnostics.iter().chain(server).collect()
}

/// The message of the diagnostic the cursor is on, for the status line.
pub fn diagnostic_at_cursor(model: &Model) -> Option<&str> {
    let cursor = model.active_document().byte_cursor();
    current_diagnostics(model)
        .into_iter()
        .find(|diagnostic| {
            diagnostic
                .byte_range
                .as_ref()
                .is_some_and(|range| range.start <= cursor && cursor <= range.end)
        })
        .map(|diagnostic| diagnostic.message.as_str())
}

pub fn close_completion(model: &mut Model) {
    model.editor.completion_at = None;
    model.editor.completions.clear();
    model.editor.completion_open = false;
    model.editor.completion_selected = 0;
    model.editor.completion_offset = 0;
}

/// Rebuilds the completion catalog only when something it is built from actually
/// changed: a page of catalog objects arriving, or the user starring something.
fn sync_catalog(model: &mut Model) {
    let key = (
        model.catalog_revision,
        model.explorer.revision(),
        model.connection.name.clone(),
    );
    if model.editor.catalog_key.as_ref() == Some(&key) {
        return;
    }
    model.editor.catalog_key = Some(key);
    if model.catalog_objects.is_empty() || model.catalog_connection != model.connection.name {
        model.editor.catalog_snapshot = None;
        return;
    }
    let favorites = model.explorer.favorite_ids();
    let mut objects = model.catalog_objects.clone();
    for object in &mut objects {
        if favorites.contains(&object.id) {
            object
                .attributes
                .insert("favorite".into(), serde_json::json!(true));
        }
    }
    model.editor.catalog_snapshot = Some(dexo_app::SnapshotCatalog::new(objects));
}

fn apply_completions(model: &mut Model, sql: &str, byte_cursor: usize, live: bool) {
    let at = byte_cursor.min(sql.len());
    let dialect = editor_dialect(model);
    // One analysis, shared: deciding whether to open the popup and deciding what goes in
    // it have to agree about what is being typed.
    let context = dexo_sql::analyze(sql, at, dialect);
    // Inside a string literal or a comment nothing the catalog knows is an answer, and
    // a popup there reads as the editor not understanding what you are writing.
    if context.intent == dexo_sql::Intent::Suppressed {
        close_completion(model);
        return;
    }
    let origin = if live {
        dexo_sql::TriggerOrigin::Typing
    } else {
        dexo_sql::TriggerOrigin::Explicit
    };
    if !dexo_sql::should_open(model.settings.completion_trigger, &context, origin) {
        close_completion(model);
        return;
    }
    sync_catalog(model);
    let items = match &model.editor.catalog_snapshot {
        Some(snapshot) => complete_with(&context, snapshot),
        None => complete_with(&context, &model.editor.catalog),
    };
    model.editor.completion_replace = context.replace.clone();
    // Before the empty check: an empty list is exactly when the answer is elsewhere.
    request_more_objects(model, &context, items.len());
    request_columns(model, &context);
    if items.is_empty() {
        close_completion(model);
        // Nothing to show yet, but an answer is on its way: remember where it was asked,
        // so it can open here when it lands.
        if model.editor.completion_request.is_some() || !model.editor.columns_pending.is_empty() {
            let document = model.active_document();
            model.editor.completion_at = Some((
                document.id.clone(),
                document.sql.revision(),
                document.cursor(),
            ));
        }
        return;
    }
    model.editor.completions = items;
    model.editor.completion_open = true;
    model.editor.completion_selected = 0;
    model.editor.completion_offset = 0;
    let document = model.active_document();
    model.editor.completion_at = Some((
        document.id.clone(),
        document.sql.revision(),
        document.cursor(),
    ));
}

/// The popup belongs to one spot in one document. Moving the cursor, editing without
/// recomputing (Delete, undo, a paste), or switching documents leaves it behind -- the
/// dbx rule: it goes away, and typing brings it back.
pub fn completion_went_stale(model: &Model) -> bool {
    let document = model.active_document();
    model
        .editor
        .completion_at
        .as_ref()
        .is_none_or(|(id, revision, cursor)| {
            *id != document.id
                || *revision != document.sql.revision()
                || *cursor != document.cursor()
        })
}

/// Whether a late answer still has somewhere to go: the popup is open, or it was asked
/// for right here and had nothing to show yet.
fn awaiting_answer(model: &Model) -> bool {
    model.editor.completion_open
        || (model.editor.completion_at.is_some() && !completion_went_stale(model))
}

/// Recomputes the popup after the catalog grew, if it is still wanted where it was.
pub fn refresh_waiting_completion(model: &mut Model) {
    if model.focus != crate::model::Focus::Editor || !awaiting_answer(model) {
        return;
    }
    let sql = model.active_document().text();
    let at = model.active_document().byte_cursor();
    // An open popup was already asked for; a waiting one only opens if typing would.
    let live = !model.editor.completion_open;
    apply_completions(model, &sql, at, live);
}

/// Queues a column lookup for every table the statement names that completion holds no
/// columns for: the sidebar and the snapshot only know what they have walked.
fn request_columns(model: &mut Model, context: &dexo_sql::CursorContext) {
    use dexo_sql::Intent;
    model.editor.columns_pending.clear();
    if !matches!(
        context.intent,
        Intent::Column
            | Intent::JoinCondition
            | Intent::AliasColumn
            | Intent::InsertColumn
            | Intent::UpdateColumn
            | Intent::Schema
    ) || model.active_session.is_none()
        || !model.connection.ready
    {
        return;
    }
    let generation = model.session_generation;
    // `venda.` before any FROM names a table too. If it is a schema instead, the lookup
    // finds no columns, once.
    let named = (context.intent == Intent::Schema)
        .then(|| context.qualifier.split_last())
        .flatten()
        .map(|(name, rest)| dexo_sql::RowSource {
            kind: dexo_sql::RowSourceKind::Table,
            schema: rest.last().cloned(),
            name: name.clone(),
            alias: None,
            depth: 0,
        });
    for source in context.row_sources.iter().chain(named.as_ref()) {
        if !matches!(
            source.kind,
            dexo_sql::RowSourceKind::Table | dexo_sql::RowSourceKind::MutationTarget
        ) {
            continue;
        }
        let known = model
            .editor
            .catalog_snapshot
            .as_ref()
            .and_then(|snapshot| dexo_sql::completion::resolve_source(source, snapshot));
        if known
            .as_ref()
            .is_some_and(|table| !table.columns.is_empty())
        {
            continue;
        }
        // The catalog's own name for the table when it has one: it carries the schema a
        // bare `from users` leaves out. Without one the driver looks in the current one.
        let target = match known {
            Some(table) if !table.schema.is_empty() => {
                dexo_driver_api::QualifiedName::new(None::<String>, Some(table.schema), table.name)
            }
            _ => dexo_driver_api::QualifiedName::new(
                None::<String>,
                source.schema.clone(),
                source.name.clone(),
            ),
        };
        let key = format!(
            "{generation}:{}",
            target.display_unquoted().to_ascii_lowercase()
        );
        if model.editor.columns_asked.insert(key) {
            model.editor.columns_pending.push(target);
        }
    }
}

/// Folds columns the session answered into the catalog completion reads, under the
/// table object that owns them -- or one made up for it when the sidebar never loaded
/// that table.
pub fn absorb_completion_columns(
    model: &mut Model,
    target: &dexo_driver_api::QualifiedName,
    columns: &[String],
) {
    use dexo_driver_api::{CatalogObject, ObjectId, ObjectKind, QualifiedName};
    if columns.is_empty() {
        return;
    }
    let owner = model
        .catalog_objects
        .iter()
        .find(|object| {
            matches!(
                object.kind,
                ObjectKind::Table | ObjectKind::View | ObjectKind::MaterializedView
            ) && object
                .qualified_name
                .object()
                .eq_ignore_ascii_case(target.object())
                && target.schema().is_none_or(|schema| {
                    object
                        .qualified_name
                        .schema()
                        .is_some_and(|own| own.eq_ignore_ascii_case(schema))
                })
        })
        .map(|object| (object.id.clone(), object.qualified_name.clone()));
    let mut objects = Vec::new();
    let (parent, name) = match owner {
        Some(found) => found,
        None => {
            let id = ObjectId::new(format!("completion:{}", target.display_unquoted()));
            objects.push(CatalogObject::new(
                id.clone(),
                ObjectKind::Table,
                target.clone(),
                None,
            ));
            (id, target.clone())
        }
    };
    for column in columns {
        objects.push(CatalogObject::new(
            ObjectId::new(format!("{}/completion-column:{column}", parent.as_ref())),
            ObjectKind::Column,
            QualifiedName::new(
                None::<String>,
                name.schema().map(str::to_string),
                format!("{}.{column}", name.object()),
            ),
            Some(parent.clone()),
        ));
    }
    model.absorb_catalog(&objects);
}

/// Asks the snapshot for more names when the objects in memory did not fill the list.
/// The in-memory catalog only holds what has been expanded in the sidebar, so on a large
/// database the table you want is usually not in it yet. Never for columns:
/// `request_columns` asks the session for those.
fn request_more_objects(model: &mut Model, context: &dexo_sql::CursorContext, found: usize) {
    model.editor.completion_request = None;
    let names = matches!(
        context.intent,
        dexo_sql::Intent::Table | dexo_sql::Intent::Schema | dexo_sql::Intent::Routine
    );
    // A prefix of one character matches most of a catalog; a full list already answers
    // the question. Either way the disk read would buy nothing.
    if !names || context.prefix.chars().count() < 2 || found >= dexo_sql::rank::CAP {
        return;
    }
    if model.connection.name.is_empty() {
        return;
    }
    let document = model.active_document();
    model.editor.completion_request = Some((
        document.id.clone(),
        document.sql.revision(),
        context.prefix.clone(),
    ));
}

/// Turns a pending search into an effect. Separate from the analysis because only
/// `update` can emit effects.
pub fn take_completion_effects(model: &mut Model) -> Vec<crate::Effect> {
    let mut effects = Vec::new();
    if let Some(session) = model.active_session {
        let generation = model.session_generation;
        effects.extend(
            std::mem::take(&mut model.editor.columns_pending)
                .into_iter()
                .map(|target| crate::Effect::LoadCompletionColumns {
                    session,
                    generation,
                    target,
                }),
        );
    }
    if let Some((document, revision, query)) = model.editor.completion_request.take() {
        effects.push(crate::Effect::SearchCompletionObjects {
            connection_id: model.connection.name.clone(),
            database_name: crate::update::catalog_database(model),
            document,
            revision,
            query,
            limit: dexo_sql::rank::CAP,
        });
    }
    effects
}

/// Folds names that arrived from the snapshot into the open popup. They are late by
/// definition, so anything that moved on since the request was made discards them.
pub fn merge_completion_objects(
    model: &mut Model,
    document: &str,
    revision: u64,
    objects: Vec<dexo_driver_api::CatalogObject>,
) {
    if !awaiting_answer(model) {
        return;
    }
    let current = model.active_document();
    if current.id != document || current.sql.revision() != revision {
        return;
    }
    let sql = current.text();
    let at = current.byte_cursor();
    let dialect = editor_dialect(model);
    let context = dexo_sql::analyze(&sql, at, dialect);
    let snapshot = dexo_app::SnapshotCatalog::new(objects);
    // The same engine, over a different catalog: whatever the position wanted, it wants
    // from these too.
    let arriving = complete_with(&context, &snapshot);
    if arriving.is_empty() {
        return;
    }
    // The user may already be on an item; keep them on it by name. Restoring the index
    // instead would move the selection out from under them as the list reorders.
    let selected = model
        .editor
        .completions
        .get(model.editor.completion_selected)
        .map(|item| item.label.clone());
    let mut merged = std::mem::take(&mut model.editor.completions);
    merged.extend(arriving);
    model.editor.completions = dexo_sql::rank::finish(merged);
    model.editor.completion_selected = selected
        .and_then(|label| {
            model
                .editor
                .completions
                .iter()
                .position(|item| item.label == label)
        })
        .unwrap_or(0);
    model.editor.completion_offset = crate::palette::scroll_to_selection(
        model.editor.completion_selected,
        model.editor.completion_offset,
        model.editor.completions.len(),
        COMPLETION_ROWS,
    );
    if !model.editor.completion_open {
        model.editor.completion_open = true;
        model.editor.completion_replace = context.replace.clone();
    }
}

fn suggest_live(model: &mut Model) {
    let sql = model.active_document().text();
    let byte_cursor = model.active_document().byte_cursor();
    // `\dt` is a command name, not something to complete; its argument (`\d orders`)
    // is a table, and completes like one.
    let line_start = sql[..byte_cursor].rfind('\n').map_or(0, |at| at + 1);
    let typed = &sql[line_start..byte_cursor];
    if typed.trim_start().starts_with('\\') && !typed.trim_start().contains(char::is_whitespace) {
        close_completion(model);
        return;
    }
    apply_completions(model, &sql, byte_cursor, true);
}

fn is_sensitive_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.contains("password") || lower.contains("secret") || lower.contains("token")
}

/// Formats the selection, or the whole document without one, as one edit: it undoes
/// in one step and the document stays the same file on the same connection. It used to
/// replace the document with a new untitled one.
pub fn apply_format(model: &mut Model) {
    let selection = model.editor_selection();
    let doc = model.active_document();
    let text = doc.text();
    let range = selection.clone().unwrap_or(0..text.chars().count());
    let source: String = text.chars().skip(range.start).take(range.len()).collect();
    if source.trim().is_empty() {
        return;
    }
    let formatted = match format_sql(&source, editor_dialect(model)) {
        Ok(formatted) => formatted,
        Err(error) => {
            model.messages.error(error.to_string());
            return;
        }
    };
    if formatted == source {
        return;
    }
    end_typing(model);
    model.editor.snippet_stops.clear();
    let doc = model.active_document_mut();
    if doc.sql.replace_chars(range.clone(), &formatted).is_err() {
        return;
    }
    // A formatted selection stays selected; otherwise the cursor lands after the text,
    // as in dbx.
    doc.anchor = selection.map(|_| range.start);
    let _ = doc.sql.set_cursor(range.start + formatted.chars().count());
    refresh_intelligence(model, false);
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
    let expansion = dexo_sql::expand(&snippet.body);
    let at = model.active_document().cursor();
    insert_text(model, &expansion.text);
    // The holes are relative to the snippet; the document knows where it was put.
    model.editor.snippet_stops = expansion
        .stops
        .into_iter()
        .map(|stop| at + stop.start..at + stop.end)
        .collect();
    model.editor.snippet_stop = 0;
    select_snippet_stop(model);
    refresh_intelligence(model, false);
}

/// Puts the cursor on the current hole, selecting whatever default text is in it so
/// typing replaces it.
fn select_snippet_stop(model: &mut Model) {
    let Some(stop) = model
        .editor
        .snippet_stops
        .get(model.editor.snippet_stop)
        .cloned()
    else {
        return;
    };
    end_typing(model);
    let doc = model.active_document_mut();
    doc.anchor = if stop.start == stop.end {
        None
    } else {
        Some(stop.start)
    };
    let _ = doc.sql.set_cursor(stop.end);
}

/// Moves to the next hole, or leaves the snippet when there are none left. Returns
/// whether it did anything, so Tab can fall through to indenting.
fn move_snippet_stop(model: &mut Model, delta: i32) -> bool {
    if model.editor.snippet_stops.is_empty() {
        return false;
    }
    let next = model.editor.snippet_stop as i32 + delta;
    if next < 0 || next as usize >= model.editor.snippet_stops.len() {
        // Walking past the last hole leaves the snippet. The selection goes with it --
        // otherwise the next thing typed replaces the text in the hole just left.
        model.editor.snippet_stops.clear();
        model.editor.snippet_stop = 0;
        let doc = model.active_document_mut();
        doc.anchor = None;
        return true;
    }
    model.editor.snippet_stop = next as usize;
    select_snippet_stop(model);
    true
}

/// What accepting the highlighted item writes, where, and whether it is a call whose
/// cursor goes between the parentheses it brings.
fn completion_edit(model: &Model) -> Option<(std::ops::Range<usize>, String, bool)> {
    let item = model
        .editor
        .completions
        .get(model.editor.completion_selected)?;
    let dialect = editor_dialect(model);
    let text = match item.kind {
        // A join condition is already written out; quoting it would break it.
        dexo_sql::CompletionKind::Keyword | dexo_sql::CompletionKind::Snippet => item.label.clone(),
        kind => {
            let name = dialect.quote_if_needed(&item.label);
            match (&item.qualifier, kind) {
                // The alias is written as the statement has it; a schema is quoted if
                // its name needs it.
                (Some(alias), dexo_sql::CompletionKind::Column) => format!("{alias}.{name}"),
                (Some(schema), _) => format!("{}.{name}", dialect.quote_if_needed(schema)),
                (None, _) => name,
            }
        }
    };
    let range = model.editor.completion_replace.clone();
    // A function comes with its parentheses and the cursor between them, unless they
    // are already there.
    let call = item.kind == dexo_sql::CompletionKind::Function && {
        let sql = model.active_document().text();
        !sql[range.end.min(sql.len())..].starts_with('(')
    };
    let text = if call { format!("{text}()") } else { text };
    Some((range, text, call))
}

/// Whether accepting would write what is already there: a complete name typed out is
/// its own completion. Enter then is a new line, not a second press to get one.
pub fn accepting_changes_nothing(model: &Model) -> bool {
    let Some((range, text, _)) = completion_edit(model) else {
        return false;
    };
    let sql = model.active_document().text();
    sql.get(range).is_some_and(|written| written == text)
}

pub fn accept_completion(model: &mut Model) {
    let Some((range, text, call)) = completion_edit(model) else {
        return;
    };
    replace_range(model, range, &text);
    if call {
        let doc = model.active_document_mut();
        let inside = doc.sql.cursor().saturating_sub(1);
        let _ = doc.sql.set_cursor(inside);
    }
    model.editor.completion_open = false;
    model.editor.completions.clear();
    refresh_intelligence(model, false);
}

/// Rows the completion popup shows at once.
pub const COMPLETION_ROWS: usize = 8;

pub fn move_completion(model: &mut Model, delta: i32) {
    if model.editor.completions.is_empty() {
        return;
    }
    let max = model.editor.completions.len() as i32 - 1;
    model.editor.completion_selected =
        (model.editor.completion_selected as i32 + delta).clamp(0, max) as usize;
    model.editor.completion_offset = crate::palette::scroll_to_selection(
        model.editor.completion_selected,
        model.editor.completion_offset,
        model.editor.completions.len(),
        COMPLETION_ROWS,
    );
}

/// Replaces a byte range, which is what the analysis reports. Accepting used to
/// recompute the token being replaced from the raw text, with its own idea of where one
/// starts -- so a qualified or quoted name was replaced from the wrong place.
fn replace_range(model: &mut Model, range: std::ops::Range<usize>, text: &str) {
    let doc = model.active_document_mut();
    if !doc.typing {
        doc.sql.end_group();
        doc.sql.begin_group();
        doc.typing = true;
    }
    doc.anchor = None;
    let _ = doc.sql.replace_bytes(range, text);
}

/// Opens the parameter prompt on the first parameter with no value, or the first one
/// when all have one (Edit Parameters).
pub fn begin_parameter_prompt(model: &mut Model) {
    model.editor.parameter_index = model
        .editor
        .parameters
        .iter()
        .position(|parameter| matches!(parameter.value, DbValue::Null))
        .unwrap_or(0);
    load_parameter_draft(model);
    model.editor.parameter_footer = crate::widgets::form::FooterFocus::Input;
    model.editor.parameter_prompt = true;
}

/// The field starts on that parameter's own value, selected so typing replaces it, or
/// empty. It used to keep whatever the last parameter was answered with, anywhere, and
/// typing went on the end of it.
fn load_parameter_draft(model: &mut Model) {
    let value = match model
        .editor
        .parameters
        .get(model.editor.parameter_index)
        .map(|parameter| &parameter.value)
    {
        Some(DbValue::Text(text)) => text.clone(),
        _ => String::new(),
    };
    model.editor.parameter_draft.set_text(value);
    model.editor.parameter_draft.select_all();
}

pub fn submit_parameters(model: &mut Model) {
    if !model.editor.parameter_draft.is_empty() {
        let index = model.editor.parameter_index;
        if let Some(parameter) = model.editor.parameters.get_mut(index) {
            parameter.value = DbValue::Text(model.editor.parameter_draft.as_str().to_string());
        }
    }
    let next = model.editor.parameter_index + 1;
    if next < model.editor.parameters.len() {
        model.editor.parameter_index = next;
        load_parameter_draft(model);
        model.editor.parameter_prompt = true;
        return;
    }
    model.editor.parameter_prompt = false;
    model.editor.parameter_index = 0;
    model.editor.parameter_draft.clear();
}

pub fn handle_key(model: &mut Model, key: KeyEvent) -> bool {
    if model.focus != crate::model::Focus::Editor {
        return false;
    }
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    if model.keymap.name == "emacs" && emacs_key(model, key) {
        return true;
    }
    // Sideways keys mean "not this": they close the popup even when the cursor has
    // nowhere to go, like Right at the end of the text.
    let sideways = matches!(
        key.code,
        KeyCode::Left | KeyCode::Right | KeyCode::Home | KeyCode::End
    ) || (matches!(key.code, KeyCode::Up | KeyCode::Down)
        && !key.modifiers.is_empty());
    if sideways {
        close_completion(model);
    }
    match key.code {
        // A letter pressed with Ctrl or Alt is a command, never text: the ones nothing
        // is bound to do nothing, where they used to be typed into the document.
        KeyCode::Char(ch) if !ctrl && !alt => {
            let end = word_end(model);
            insert_text(model, &ch.to_string());
            if !(ch.is_alphanumeric() || matches!(ch, '_' | '$' | '.')) {
                capitalize_keyword(model, end);
            }
            suggest_live(model);
            true
        }
        KeyCode::Enter if model.editor.completion_open && !accepting_changes_nothing(model) => {
            accept_completion(model);
            true
        }
        KeyCode::Enter => {
            close_completion(model);
            let end = word_end(model);
            insert_newline(model);
            capitalize_keyword(model, end);
            true
        }
        KeyCode::Tab if model.editor.completion_open => {
            accept_completion(model);
            true
        }
        // Only bare arrows walk the list; Shift+Up still extends the selection.
        KeyCode::Up if model.editor.completion_open && key.modifiers.is_empty() => {
            move_completion(model, -1);
            true
        }
        KeyCode::Down if model.editor.completion_open && key.modifiers.is_empty() => {
            move_completion(model, 1);
            true
        }
        KeyCode::PageUp if model.editor.completion_open => {
            move_completion(model, -(COMPLETION_ROWS as i32));
            true
        }
        KeyCode::PageDown if model.editor.completion_open => {
            move_completion(model, COMPLETION_ROWS as i32);
            true
        }
        KeyCode::Esc if model.editor.completion_open => {
            model.editor.completion_open = false;
            true
        }
        KeyCode::PageUp => {
            page(model, -1, shift);
            true
        }
        KeyCode::PageDown => {
            page(model, 1, shift);
            true
        }
        KeyCode::Tab => {
            // Tab has three owners here, in this order: the open popup takes it, then an
            // active snippet, and only then does it indent.
            if !move_snippet_stop(model, 1) {
                let end = word_end(model);
                insert_text(model, "    ");
                capitalize_keyword(model, end);
            }
            true
        }
        KeyCode::BackTab => {
            move_snippet_stop(model, -1);
            true
        }
        // Ctrl (Alt on macOS) takes the word Ctrl+Left would cross. A terminal without
        // the kitty protocol sends Ctrl+Backspace as ^H; `update` turns that back into
        // Ctrl+Backspace before it gets here.
        KeyCode::Backspace => {
            backspace(model, ctrl || alt);
            suggest_live(model);
            true
        }
        KeyCode::Delete => {
            delete(model, ctrl || alt);
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
            move_line_edge(model, true, shift, ctrl);
            true
        }
        KeyCode::End => {
            move_line_edge(model, false, shift, ctrl);
            true
        }
        // Ctrl+Up and Ctrl+Down move the view and leave the cursor, as in VS Code.
        KeyCode::Up if ctrl && !shift => {
            scroll_view(model, -1);
            true
        }
        KeyCode::Down if ctrl && !shift => {
            scroll_view(model, 1);
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

/// Where an edit starts and how long the buffer was, so the snippet's remaining holes
/// can be moved with the text. Without this, typing into one hole leaves every later one
/// pointing at the wrong characters.
fn edit_mark(model: &Model) -> (usize, usize) {
    let doc = model.active_document();
    let at = match doc.selection() {
        Some(range) => range.start.min(doc.cursor()),
        None => doc.cursor(),
    };
    (at, doc.text().chars().count())
}

fn shift_snippet_stops(model: &mut Model, mark: (usize, usize)) {
    if model.editor.snippet_stops.is_empty() {
        return;
    }
    let (at, before) = mark;
    let after = model.active_document().text().chars().count();
    if after == before {
        return;
    }
    let shift = |value: &mut usize| {
        if *value >= at {
            *value = (*value as i64 + after as i64 - before as i64).max(at as i64) as usize;
        }
    };
    for stop in &mut model.editor.snippet_stops {
        shift(&mut stop.start);
        shift(&mut stop.end);
    }
}

/// Drops pasted text in whole. Character by character it was one dispatch, one
/// intelligence pass and one frame each -- and the completion popup it opened on the
/// way turned the next tab in the text into an accepted suggestion instead of
/// indentation.
pub fn paste(model: &mut Model, text: &str) -> bool {
    // With the find bar open, a paste is for the bar; with Vim's `:` or `/` open, for
    // that line.
    if model.focus != crate::model::Focus::Editor
        || model.active_document().kind.is_table()
        || model.find.open
        || (crate::screens::vim::active(model) && model.vim.prompt.is_some())
    {
        return false;
    }
    // The completion popup is the editor's own, not a modal with a claim on the paste:
    // pasting dismisses it. Anything else on top does own the keys, and the paste with
    // them, or the text lands in the buffer underneath where nobody sees it go.
    model.editor.completion_open = false;
    if crate::mouse::overlay_blocks_workbench(model) {
        return false;
    }
    // A paste is not typing: none of it should be completed or expanded, and the whole
    // of it belongs in one undo step.
    end_typing(model);
    insert_text(model, &text.replace("\r\n", "\n").replace('\r', "\n"));
    end_typing(model);
    true
}

fn insert_text(model: &mut Model, text: &str) {
    let mark = edit_mark(model);
    let doc = model.active_document_mut();
    let range = doc.selection();
    if range.is_none() {
        doc.anchor = None;
    }
    if range.is_some() && doc.typing {
        doc.sql.end_group();
        doc.typing = false;
    }
    if !doc.typing {
        doc.sql.end_group();
        doc.sql.begin_group();
        doc.typing = true;
    }
    // Select All on an empty document leaves an anchor beside the cursor, with nothing
    // between them. Left there, the first letter typed became a selection of its own and
    // the second one replaced it.
    doc.anchor = None;
    let _ = if let Some(range) = range {
        doc.sql.replace_chars(range, text)
    } else {
        doc.sql.insert(doc.sql.cursor(), text)
    };
    shift_snippet_stops(model, mark);
}

/// Where a word being finished by the next key ends, in characters -- the cursor, unless
/// that key replaces a selection.
fn word_end(model: &Model) -> Option<usize> {
    let doc = model.active_document();
    doc.selection().is_none().then(|| doc.cursor())
}

/// Writes the reserved word that ends at `end` in capitals, once a key has finished it:
/// `select ` becomes `SELECT `. Left as typed inside a string or a comment, after a dot
/// (`t.order` is a column), as part of a `:param` or `$1`, and in quotes. Same length,
/// so the cursor and every position the editor holds stay where they are.
fn capitalize_keyword(model: &mut Model, end: Option<usize>) {
    let Some(end) = end else {
        return;
    };
    let dialect = editor_dialect(model);
    let doc = model.active_document_mut();
    let text = doc.sql.text();
    let byte_end = text
        .char_indices()
        .nth(end)
        .map_or(text.len(), |(at, _)| at);
    let start = text[..byte_end]
        .char_indices()
        .rev()
        .find(|(_, ch)| !(ch.is_ascii_alphanumeric() || *ch == '_'))
        .map_or(0, |(at, ch)| at + ch.len_utf8());
    let word = &text[start..byte_end];
    if word.is_empty()
        || !dexo_sql::is_reserved(word)
        || word.bytes().all(|byte| !byte.is_ascii_lowercase())
    {
        return;
    }
    let before = text[..start].chars().next_back();
    if before.is_some_and(|ch| ch.is_alphanumeric() || "._$:@\"`'".contains(ch))
        || dexo_sql::suppressed_at(&text, byte_end, dialect)
    {
        return;
    }
    let upper = word.to_ascii_uppercase();
    let from = end - word.len();
    let cursor = doc.sql.cursor();
    if doc.sql.replace_chars(from..end, &upper).is_ok() {
        let _ = doc.sql.set_cursor(cursor);
    }
}

fn insert_newline(model: &mut Model) {
    let indent = current_line_indent(model);
    end_typing(model);
    let doc = model.active_document_mut();
    let _ = doc.sql.insert(doc.sql.cursor(), &format!("\n{indent}"));
}

fn backspace(model: &mut Model, word: bool) {
    let mark = edit_mark(model);
    end_typing(model);
    let doc = model.active_document_mut();
    if let Some(range) = doc.selection() {
        doc.anchor = None;
        let _ = doc.sql.delete(range);
    } else {
        let cursor = doc.sql.cursor();
        let start = if word {
            word_delete_start(&doc.sql.text(), cursor)
        } else {
            cursor.saturating_sub(1)
        };
        if start < cursor {
            let _ = doc.sql.delete(start..cursor);
        }
    }
    shift_snippet_stops(model, mark);
}

fn delete(model: &mut Model, word: bool) {
    let mark = edit_mark(model);
    end_typing(model);
    let doc = model.active_document_mut();
    if let Some(range) = doc.selection() {
        doc.anchor = None;
        let _ = doc.sql.delete(range);
    } else {
        let cursor = doc.sql.cursor();
        let text = doc.sql.text();
        let end = if word {
            word_delete_end(&text, cursor)
        } else {
            (cursor + 1).min(text.chars().count())
        };
        if cursor < end {
            let _ = doc.sql.delete(cursor..end);
        }
    }
    shift_snippet_stops(model, mark);
}

/// What Ctrl+C and Ctrl+X take: the selection, or the whole line under the cursor with
/// its newline when nothing is selected -- the VS Code rule, so a bare Ctrl+C on a line
/// still copies it.
fn clipboard_range(doc: &EditorDocument) -> Option<std::ops::Range<usize>> {
    if let Some(range) = doc.selection() {
        return (!range.is_empty()).then_some(range);
    }
    let text = doc.sql.text();
    let (start, end) = line_bounds(&text, doc.sql.cursor());
    let end = (end + 1).min(text.chars().count());
    (start < end).then_some(start..end)
}

pub fn copy(model: &Model) -> Option<String> {
    let doc = model.active_document();
    let range = clipboard_range(doc)?;
    Some(
        doc.sql
            .text()
            .chars()
            .skip(range.start)
            .take(range.len())
            .collect(),
    )
}

pub fn cut(model: &mut Model) -> Option<String> {
    let mark = edit_mark(model);
    end_typing(model);
    let doc = model.active_document_mut();
    let range = clipboard_range(doc)?;
    let text = doc
        .sql
        .text()
        .chars()
        .skip(range.start)
        .take(range.len())
        .collect();
    doc.anchor = None;
    let _ = doc.sql.delete(range);
    shift_snippet_stops(model, mark);
    Some(text)
}

pub fn undo(model: &mut Model) {
    end_typing(model);
    let doc = model.active_document_mut();
    let _ = doc.sql.undo();
}

pub fn redo(model: &mut Model) {
    end_typing(model);
    let doc = model.active_document_mut();
    let _ = doc.sql.redo();
}

/// The lines a line edit acts on, first and last: the cursor's, or every line the
/// selection touches -- one ending at the very start of a line leaves that line out.
fn touched_lines(doc: &EditorDocument, text: &str) -> (usize, usize) {
    let line_of = |at: usize| text.chars().take(at).filter(|ch| *ch == '\n').count();
    match doc.selection() {
        Some(range) => {
            let first = line_of(range.start);
            let mut last = line_of(range.end);
            let at_line_start = range.end > 0 && text.chars().nth(range.end - 1) == Some('\n');
            if last > first && at_line_start {
                last -= 1;
            }
            (first, last)
        }
        None => {
            let line = line_of(doc.cursor());
            (line, line)
        }
    }
}

/// Where each line starts, in chars.
fn line_starts(lines: &[&str]) -> Vec<usize> {
    let mut starts = Vec::with_capacity(lines.len());
    let mut at = 0;
    for line in lines {
        starts.push(at);
        at += line.chars().count() + 1;
    }
    starts
}

/// Replaces lines `first..=last` with `block` as one undo step, then puts the cursor and
/// the selection back where they were, moved by `shift` chars.
fn replace_lines(model: &mut Model, first: usize, last: usize, block: &str, shift: isize) {
    end_typing(model);
    let doc = model.active_document_mut();
    let text = doc.sql.text();
    let lines: Vec<&str> = text.split('\n').collect();
    let starts = line_starts(&lines);
    let range = starts[first]..starts[last] + lines[last].chars().count();
    // Never back past the edited lines: taking `-- ` off under the cursor would
    // otherwise drop it onto the line above.
    let floor = range.start as isize;
    let moved = |at: usize| (at as isize + shift).max(floor) as usize;
    let (anchor, cursor) = (doc.anchor.map(moved), moved(doc.sql.cursor()));
    doc.sql.begin_group();
    let _ = doc.sql.replace_chars(range, block);
    doc.sql.end_group();
    let len = doc.sql.text().chars().count();
    doc.anchor = anchor.map(|at| at.min(len));
    let _ = doc.sql.set_cursor(cursor.min(len));
    refresh_intelligence(model, false);
    follow_cursor(model);
}

/// Ctrl+/: comments the touched lines out with `--`, or, when every one of them already
/// is, takes the comment off. Blank lines are left alone.
pub fn toggle_comment(model: &mut Model) {
    let doc = model.active_document();
    let text = doc.text();
    let (first, last) = touched_lines(doc, &text);
    let lines: Vec<&str> = text.split('\n').collect();
    let block = &lines[first..=last];
    let filled: Vec<&&str> = block
        .iter()
        .filter(|line| !line.trim().is_empty())
        .collect();
    if filled.is_empty() {
        return;
    }
    let commented = filled
        .iter()
        .all(|line| line.trim_start().starts_with("--"));
    // One column for the whole block, the shallowest indent, so the dashes line up.
    let indent = filled
        .iter()
        .map(|line| line.chars().take_while(|ch| ch.is_whitespace()).count())
        .min()
        .unwrap_or(0);
    // The cursor moves with the text it sits in, and stays put in the indentation the
    // dashes go after.
    let cursor_column = {
        let doc = model.active_document();
        let line_start: usize = lines[..first]
            .iter()
            .map(|line| line.chars().count() + 1)
            .sum();
        doc.cursor().saturating_sub(line_start)
    };
    let mut shift = 0isize;
    let edited: Vec<String> = block
        .iter()
        .map(|line| {
            if line.trim().is_empty() {
                return line.to_string();
            }
            let lead = line.chars().take_while(|ch| ch.is_whitespace()).count();
            if commented {
                let rest: String = line.chars().skip(lead).collect();
                let bare = rest
                    .strip_prefix("-- ")
                    .or_else(|| rest.strip_prefix("--"))
                    .unwrap_or(&rest);
                let removed = rest.chars().count() - bare.chars().count();
                shift -= removed.min(cursor_column.saturating_sub(lead)) as isize;
                format!("{}{bare}", line.chars().take(lead).collect::<String>())
            } else {
                if cursor_column >= indent {
                    shift += 3;
                }
                let head: String = line.chars().take(indent).collect();
                let tail: String = line.chars().skip(indent).collect();
                format!("{head}-- {tail}")
            }
        })
        .collect();
    // The cursor moves with its own line's edit; one line is the common case, and for
    // a block the selection is put around all of it below.
    let single = first == last;
    replace_lines(
        model,
        first,
        last,
        &edited.join("\n"),
        if single { shift } else { 0 },
    );
    if !single {
        select_lines(model, first, last);
    }
}

/// Selects lines `first..=last` whole, so a repeated line edit acts on the same block.
fn select_lines(model: &mut Model, first: usize, last: usize) {
    let doc = model.active_document_mut();
    let text = doc.sql.text();
    let lines: Vec<&str> = text.split('\n').collect();
    let starts = line_starts(&lines);
    doc.anchor = Some(starts[first]);
    let _ = doc
        .sql
        .set_cursor(starts[last] + lines[last].chars().count());
}

/// Ctrl+Shift+D: the touched lines again, below themselves; the cursor goes with the copy.
pub fn duplicate_lines(model: &mut Model) {
    let doc = model.active_document();
    let text = doc.text();
    let (first, last) = touched_lines(doc, &text);
    let lines: Vec<&str> = text.split('\n').collect();
    let block = lines[first..=last].join("\n");
    let shift = block.chars().count() as isize + 1;
    replace_lines(model, first, last, &format!("{block}\n{block}"), shift);
}

/// Ctrl+Shift+Up and Down: the touched lines trade places with the line above or below.
pub fn move_lines(model: &mut Model, up: bool) {
    let doc = model.active_document();
    let text = doc.text();
    let (first, last) = touched_lines(doc, &text);
    let lines: Vec<&str> = text.split('\n').collect();
    if (up && first == 0) || (!up && last + 1 >= lines.len()) {
        return;
    }
    let block = lines[first..=last].join("\n");
    if up {
        let above = lines[first - 1];
        let shift = -(above.chars().count() as isize + 1);
        replace_lines(model, first - 1, last, &format!("{block}\n{above}"), shift);
    } else {
        let below = lines[last + 1];
        let shift = below.chars().count() as isize + 1;
        replace_lines(model, first, last + 1, &format!("{below}\n{block}"), shift);
    }
}

/// Puts `text` in place of the document's, as one undo step, keeping the cursor's place
/// as far as the new text allows. Nothing happens to a document that is gone.
pub fn replace_document(model: &mut Model, document: &str, text: &str) {
    let Some(index) = model.documents.iter().position(|doc| doc.id == document) else {
        return;
    };
    let doc = &mut model.documents[index];
    let current = doc.sql.text();
    // Editors end a file with a line break; one the document never had is theirs.
    let text = if current.ends_with('\n') {
        text
    } else {
        text.strip_suffix("\r\n")
            .or_else(|| text.strip_suffix('\n'))
            .unwrap_or(text)
    };
    if current == text {
        return;
    }
    if doc.typing {
        doc.sql.end_group();
        doc.typing = false;
    }
    let cursor = doc.sql.cursor();
    let len = doc.sql.text().chars().count();
    doc.anchor = None;
    doc.sql.begin_group();
    let _ = doc.sql.replace_chars(0..len, text);
    doc.sql.end_group();
    let _ = doc.sql.set_cursor(cursor.min(text.chars().count()));
    if index == model.active_document {
        refresh_intelligence(model, false);
        follow_cursor(model);
    }
}

pub fn select_all(model: &mut Model) {
    end_typing(model);
    let doc = model.active_document_mut();
    let len = doc.sql.text().chars().count();
    // Nothing to select in an empty document. An anchor left on its cursor was a
    // selection that began with the first letter typed, and the second replaced it.
    doc.anchor = (len > 0).then_some(0);
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
}

/// Home and End: the line's edge, or with `document` (Ctrl) the document's.
fn move_line_edge(model: &mut Model, home: bool, shift: bool, document: bool) {
    end_typing(model);
    let doc = model.active_document_mut();
    let text = doc.sql.text();
    let (line_start, line_end) = if document {
        (0, text.chars().count())
    } else {
        line_bounds(&text, doc.sql.cursor())
    };
    apply_move(doc, if home { line_start } else { line_end }, shift);
}

/// Ctrl+K in the Emacs profile: from the cursor to the end of the line, and at the end
/// of a line the newline after it, so the next line comes up.
fn kill_line(model: &mut Model) {
    let mark = edit_mark(model);
    end_typing(model);
    let doc = model.active_document_mut();
    let text = doc.sql.text();
    let cursor = doc.sql.cursor();
    let (_, line_end) = line_bounds(&text, cursor);
    let end = if cursor == line_end {
        (line_end + 1).min(text.chars().count())
    } else {
        line_end
    };
    doc.anchor = None;
    if cursor < end {
        let _ = doc.sql.delete(cursor..end);
    }
    shift_snippet_stops(model, mark);
}

/// The Emacs profile's motion and kill keys. The keymap has no commands for moving the
/// cursor -- the arrows are the editor's own keys -- so these are read here, once the
/// keymap has passed them by. They were only the overlay on the default profile before:
/// Ctrl+A selected everything, Ctrl+F opened Find, and Alt+F typed an f.
fn emacs_key(model: &mut Model, key: KeyEvent) -> bool {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let KeyCode::Char(letter) = key.code else {
        return false;
    };
    let popup = model.editor.completion_open;
    match (ctrl, alt, letter.to_ascii_lowercase()) {
        (true, false, 'a') => move_line_edge(model, true, false, false),
        (true, false, 'e') => move_line_edge(model, false, false, false),
        (true, false, 'b') => move_chars(model, -1, false, false),
        (true, false, 'f') => move_chars(model, 1, false, false),
        (true, false, 'n') if popup => move_completion(model, 1),
        (true, false, 'p') if popup => move_completion(model, -1),
        (true, false, 'n') => move_vertical(model, 1, false),
        (true, false, 'p') => move_vertical(model, -1, false),
        (true, false, 'd') => delete(model, false),
        (true, false, 'k') => kill_line(model),
        (true, false, 'v') => page(model, 1, false),
        (false, true, 'f') => move_chars(model, 1, false, true),
        (false, true, 'b') => move_chars(model, -1, false, true),
        (false, true, 'd') => delete(model, true),
        (false, true, 'v') => page(model, -1, false),
        (false, true, '<') => move_line_edge(model, true, false, true),
        (false, true, '>') => move_line_edge(model, false, false, true),
        _ => return false,
    }
    true
}

fn move_vertical(model: &mut Model, delta: i32, shift: bool) {
    end_typing(model);
    let doc = model.active_document_mut();
    let text = doc.sql.text();
    let (line, col) = line_col(&text, doc.sql.cursor());
    let next_line = line.saturating_add_signed(delta as isize);
    let cursor = cursor_at(&text, next_line, col);
    apply_move(doc, cursor, shift);
}

/// PageUp and PageDown: the view and the cursor move a screenful together, so the
/// cursor keeps its row on screen. They did nothing outside the completion popup.
fn page(model: &mut Model, direction: i32, shift: bool) {
    let rows = text_area(model).map_or(1, |(rows, _)| rows);
    let doc = model.active_document_mut();
    let lines = doc.sql.text().matches('\n').count() + 1;
    doc.viewport_line = if direction < 0 {
        doc.viewport_line.saturating_sub(rows)
    } else {
        (doc.viewport_line + rows).min(lines.saturating_sub(rows))
    };
    move_vertical(model, direction * rows as i32, shift);
}

/// Moves the view a line without the cursor, for the wheel and Ctrl+Up/Down. It stops with
/// the last line at the top, so turning back moves at once instead of first unwinding
/// every line scrolled past the end.
pub(crate) fn scroll_view(model: &mut Model, delta: i32) {
    let doc = model.active_document_mut();
    let last = doc.sql.text().matches('\n').count();
    doc.viewport_line = doc
        .viewport_line
        .saturating_add_signed(delta as isize)
        .min(last);
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

pub(crate) fn extend_selection_to(model: &mut Model, cursor: usize) {
    end_typing(model);
    let doc = model.active_document_mut();
    apply_move(doc, cursor, true);
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

/// Ctrl+Left and Ctrl+Right, the way VS Code moves: Left to the start of the run before
/// the cursor, Right to the end of the run after it. Neither leaves the line except from
/// its edge, and then across one line break, so the end of a line and an empty line are
/// stops. A lone separator glued to a word goes with it, so `p.name` is one press. It used
/// to skip every non-word character, blank lines included, and took `ç` for punctuation.
fn word_jump(text: &str, cursor: usize, delta: i32) -> usize {
    // ponytail: O(n) char scan per keystroke; switch to rope line/char APIs if files get huge.
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    let mut at = cursor.min(len);
    let lone_separator = |run: &std::ops::Range<usize>| {
        run.len() == 1
            && !is_word_char(chars[run.start])
            && chars.get(run.end).is_some_and(|ch| is_word_char(*ch))
    };
    if delta < 0 {
        if at > 0 && chars[at - 1] == '\n' {
            at -= 1;
        }
        let line_start = chars[..at]
            .iter()
            .rposition(|ch| *ch == '\n')
            .map_or(0, |index| index + 1);
        let mut run = run_before(&chars, line_start, at);
        if let Some(lone) = run.clone().filter(|run| lone_separator(run)) {
            run = run_before(&chars, line_start, lone.start);
        }
        run.map_or(line_start, |run| run.start)
    } else {
        if at < len && chars[at] == '\n' {
            at += 1;
        }
        let line_end = chars[at..]
            .iter()
            .position(|ch| *ch == '\n')
            .map_or(len, |index| at + index);
        let mut run = run_after(&chars, at, line_end);
        if let Some(lone) = run.clone().filter(|run| lone_separator(run)) {
            run = run_after(&chars, lone.end, line_end);
        }
        run.map_or(line_end, |run| run.end)
    }
}

/// The nearest run of word characters, or of punctuation, before `at` and after
/// `line_start`, blanks between skipped.
fn run_before(chars: &[char], line_start: usize, at: usize) -> Option<std::ops::Range<usize>> {
    let mut end = at;
    while end > line_start && is_blank(chars[end - 1]) {
        end -= 1;
    }
    if end == line_start {
        return None;
    }
    let word = is_word_char(chars[end - 1]);
    let mut start = end - 1;
    while start > line_start
        && !is_blank(chars[start - 1])
        && is_word_char(chars[start - 1]) == word
    {
        start -= 1;
    }
    Some(start..end)
}

/// [`run_before`] the other way: the nearest run after `at` and before `line_end`.
fn run_after(chars: &[char], at: usize, line_end: usize) -> Option<std::ops::Range<usize>> {
    let mut start = at;
    while start < line_end && is_blank(chars[start]) {
        start += 1;
    }
    if start == line_end {
        return None;
    }
    let word = is_word_char(chars[start]);
    let mut end = start + 1;
    while end < line_end && !is_blank(chars[end]) && is_word_char(chars[end]) == word {
        end += 1;
    }
    Some(start..end)
}

/// Letters (accented ones too), digits and `_` are one kind of run; anything else that is
/// not a space is the other, so `products.name` takes three presses, not one.
fn is_word_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

fn is_blank(ch: char) -> bool {
    ch == ' ' || ch == '\t'
}

/// Where Ctrl+Backspace deletes back to, the way VS Code does it. At the start of a line
/// it takes only the line break. Two or more blanks before the cursor go on their own, so
/// clearing indentation or a gap never eats the word before it. Otherwise it takes the
/// blank, then one run of word characters or one run of punctuation, and never crosses
/// into the line above. It used to skip every non-word character, newlines included, so
/// one press in an empty line could delete the end of the statement above it.
fn word_delete_start(text: &str, cursor: usize) -> usize {
    let chars: Vec<char> = text.chars().collect();
    let cursor = cursor.min(chars.len());
    if cursor == 0 {
        return 0;
    }
    if chars[cursor - 1] == '\n' {
        return cursor - 1;
    }
    let line_start = chars[..cursor]
        .iter()
        .rposition(|ch| *ch == '\n')
        .map_or(0, |index| index + 1);
    let mut start = cursor;
    while start > line_start && is_blank(chars[start - 1]) {
        start -= 1;
    }
    if cursor - start >= 2 || start == line_start {
        return start;
    }
    let word = is_word_char(chars[start - 1]);
    while start > line_start
        && !is_blank(chars[start - 1])
        && is_word_char(chars[start - 1]) == word
    {
        start -= 1;
    }
    start
}

/// Ctrl+Delete, the counterpart of [`word_delete_start`]: at the end of a line it takes
/// only the line break, and two or more blanks go on their own. Otherwise it takes one run
/// and the blanks on one side of it, never both, so the words either side are not glued
/// together: `select |name from` loses `name `, `foo| bar baz` loses ` bar`.
fn word_delete_end(text: &str, cursor: usize) -> usize {
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    let cursor = cursor.min(len);
    let blank_end = |from: usize| {
        let mut end = from;
        while end < len && is_blank(chars[end]) {
            end += 1;
        }
        end
    };
    if cursor == len {
        return len;
    }
    if chars[cursor] == '\n' {
        return cursor + 1;
    }
    let start = blank_end(cursor);
    if start - cursor >= 2 || start == len || chars[start] == '\n' {
        return start;
    }
    let word = is_word_char(chars[start]);
    let mut end = start;
    while end < len
        && !is_blank(chars[end])
        && chars[end] != '\n'
        && is_word_char(chars[end]) == word
    {
        end += 1;
    }
    if start == cursor { blank_end(end) } else { end }
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

impl EditorState {
    /// `document`'s text was replaced whole, its revision starting over: what was
    /// painted for it is not for this text.
    pub fn repaint(&mut self, document: &str) {
        if self
            .painted
            .as_ref()
            .is_some_and(|(painted, _)| painted == document)
        {
            self.painted = None;
        }
    }

    /// The history the filters leave, newest first, a statement run again on the same
    /// connection once: its last run, and how many there were.
    pub fn history_lines(&self) -> Vec<HistoryLine<'_>> {
        let mut lines: Vec<HistoryLine<'_>> = Vec::new();
        for row in &self.history {
            if self
                .history_connection
                .as_ref()
                .is_some_and(|connection| row.connection_id.as_ref() != Some(connection))
                || !self.history_status.lets(row.outcome)
                || !self.history_search.matches([row.sql.as_str()])
            {
                continue;
            }
            match lines
                .iter_mut()
                .find(|line| line.row.sql == row.sql && line.row.connection_id == row.connection_id)
            {
                Some(line) => line.ids.push(row.id.clone()),
                None => lines.push(HistoryLine {
                    row,
                    ids: vec![row.id.clone()],
                }),
            }
        }
        lines
    }

    /// Whether a filter is on: the search, a connection or a status.
    pub fn history_filtered(&self) -> bool {
        !self.history_search.input.is_empty()
            || self.history_connection.is_some()
            || self.history_status != StatusFilter::All
    }

    pub fn clear_history_filters(&mut self) {
        self.history_search = Default::default();
        self.history_connection = None;
        self.history_status = StatusFilter::All;
        self.history_selected = 0;
    }

    /// The connections the history has runs of, the one in use first.
    pub fn history_connections(&self, in_use: &str) -> Vec<String> {
        let mut connections: Vec<String> = Vec::new();
        for connection in self
            .history
            .iter()
            .filter_map(|row| row.connection_id.as_ref())
        {
            if !connections.contains(connection) {
                connections.push(connection.clone());
            }
        }
        if let Some(at) = connections
            .iter()
            .position(|connection| connection == in_use)
        {
            let current = connections.remove(at);
            connections.insert(0, current);
        }
        connections
    }

    /// Keeps the highlight on a row the filters still list.
    pub fn clamp_history(&mut self) {
        let count = self.history_lines().len();
        self.history_selected = self.history_selected.min(count.saturating_sub(1));
    }
}

/// A statement in the history list: its last run, and the ids of every run of it the
/// filters leave.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HistoryLine<'a> {
    pub row: &'a dexo_storage::HistoryRow,
    pub ids: Vec<String>,
}

/// Which runs the history shows by how they went.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum StatusFilter {
    #[default]
    All,
    Ok,
    /// Failed or cancelled: what did not run through.
    Failed,
}

impl StatusFilter {
    pub fn next(self) -> Self {
        match self {
            Self::All => Self::Ok,
            Self::Ok => Self::Failed,
            Self::Failed => Self::All,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Ok => "ok",
            Self::Failed => "failed",
        }
    }

    fn lets(self, outcome: dexo_storage::HistoryOutcome) -> bool {
        match self {
            Self::All => true,
            Self::Ok => outcome == dexo_storage::HistoryOutcome::Ok,
            Self::Failed => outcome != dexo_storage::HistoryOutcome::Ok,
        }
    }
}

/// Rows the history list takes a PageUp or PageDown.
pub const HISTORY_PAGE: usize = 8;

/// The run the highlight is on.
pub fn picked_history(model: &Model) -> Option<dexo_storage::HistoryRow> {
    model
        .editor
        .history_lines()
        .get(model.editor.history_selected)
        .map(|line| line.row.clone())
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

/// Leaves the prompt without running anything. One place, so the key and the mouse
/// cannot come to mean different things by it.
pub fn cancel_parameters(model: &mut Model) {
    model.editor.parameter_prompt = false;
    model.editor.parameter_index = 0;
    model.editor.parameter_draft.clear();
    model.editor.parameter_footer = crate::widgets::form::FooterFocus::Input;
}

/// Returns what the key did, because the caller has to tell a submit from a cancel:
/// both close the prompt, and only one of them should run the statement.
pub fn handle_parameter_key(model: &mut Model, key: KeyEvent) -> crate::widgets::form::FooterKey {
    use crate::widgets::form::{FooterFocus, FooterKey, footer_key};
    let outcome = footer_key(&mut model.editor.parameter_footer, &key);
    match outcome {
        FooterKey::Cancel => cancel_parameters(model),
        FooterKey::Submit => {
            submit_parameters(model);
            model.editor.parameter_footer = FooterFocus::Input;
        }
        FooterKey::Moved => {}
        FooterKey::Pass if model.editor.parameter_footer == FooterFocus::Input => {
            model.editor.parameter_draft.handle_key(key);
        }
        FooterKey::Pass => {}
    }
    outcome
}
