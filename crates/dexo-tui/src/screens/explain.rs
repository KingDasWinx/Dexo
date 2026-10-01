use dexo_app::explain_service::{
    NodeDelta, PlanRow, PlanView, compare_plans, plan_view, row_tags, summary_lines, table_cells,
    table_columns, table_order, tree_cells, tree_columns,
};
use dexo_driver_api::ExplainPlan;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExplainView {
    Tree,
    Table,
    Summary,
}

impl ExplainView {
    pub fn next(self) -> Self {
        match self {
            Self::Tree => Self::Table,
            Self::Table => Self::Summary,
            Self::Summary => Self::Tree,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExplainScreen {
    pub plan: Option<ExplainPlan>,
    pub compare: Vec<NodeDelta>,
    pub view: ExplainView,
    /// The statement `plan` is for. Each new plan used to be compared with whatever came
    /// before it, often a plan of some other query.
    pub sql: String,
}

impl Default for ExplainScreen {
    fn default() -> Self {
        Self {
            plan: None,
            compare: Vec::new(),
            view: ExplainView::Tree,
            sql: String::new(),
        }
    }
}

/// The styles a plan is drawn with, taken from the theme by the caller.
pub struct ExplainStyles {
    pub muted: Style,
    pub warning: Style,
    pub unicode: bool,
}

impl ExplainScreen {
    pub fn fixture() -> Self {
        let plan = ExplainPlan {
            planning_ms: Some(0.04),
            execution_ms: Some(0.21),
            raw: r#"[{"Plan":{"Node Type":"Seq Scan","Relation Name":"items"}}]"#.into(),
            root: dexo_driver_api::PlanNode {
                kind: "Seq Scan".into(),
                relation: Some("items".into()),
                detail: None,
                estimates: dexo_driver_api::PlanMetrics {
                    cost: Some(22.5),
                    rows: Some(10.0),
                    width: Some(36.0),
                    time_ms: None,
                },
                actual: dexo_driver_api::PlanMetrics {
                    cost: None,
                    rows: Some(1000.0),
                    width: None,
                    time_ms: Some(0.18),
                },
                loops: Some(1),
                children: Vec::new(),
                native: Default::default(),
            },
        };
        Self {
            plan: Some(plan),
            ..Self::default()
        }
    }

    /// The plan fitted to `width` cells. It used to print a line of internal flags, a
    /// table whose tab-separated cells ran together, and plan comparisons as Rust debug
    /// output.
    pub fn lines(&self, width: u16, styles: &ExplainStyles) -> Vec<Line<'static>> {
        let Some(plan) = &self.plan else {
            return vec![Line::styled(
                "No plan yet. Explain Plan or Explain Analyze shows the plan of the statement under the cursor.",
                styles.muted,
            )];
        };
        let width = width as usize;
        let view = plan_view(plan, styles.unicode);
        match self.view {
            ExplainView::Tree => {
                let mut headline = view.headline.clone();
                if !self.compare.is_empty() {
                    let count = self.compare.len();
                    headline.push_str(&format!(
                        " · {count} {} since the last plan (Summary)",
                        if count == 1 { "change" } else { "changes" }
                    ));
                }
                let rows: Vec<&PlanRow> = view.rows.iter().collect();
                columns(
                    &headline,
                    tree_columns(view.analyzed),
                    &rows,
                    |row| tree_cells(row, view.analyzed, styles.unicode),
                    true,
                    &view,
                    width,
                    styles,
                )
            }
            ExplainView::Table => {
                let order = table_order(&view);
                let rows: Vec<&PlanRow> = order.iter().map(|index| &view.rows[*index]).collect();
                columns(
                    &view.headline,
                    table_columns(view.analyzed),
                    &rows,
                    |row| table_cells(row, view.analyzed),
                    false,
                    &view,
                    width,
                    styles,
                )
            }
            ExplainView::Summary => summary_lines(plan, &self.compare, styles.unicode)
                .into_iter()
                .enumerate()
                .map(|(index, text)| {
                    if index == 0 {
                        Line::styled(text, Style::default().add_modifier(Modifier::BOLD))
                    } else if text.starts_with("Off estimate") || text.starts_with("Slowest") {
                        Line::styled(text, styles.warning)
                    } else {
                        Line::raw(text)
                    }
                })
                .collect(),
        }
    }

    /// A second plan of the same statement is compared with the first -- estimated
    /// against analyzed, or before and after an index -- and any other plan starts clean.
    pub fn set_plan(&mut self, plan: ExplainPlan, sql: String) {
        self.compare = match &self.plan {
            Some(previous) if self.sql == sql => compare_plans(previous, &plan),
            _ => Vec::new(),
        };
        self.plan = Some(plan);
        self.sql = sql;
    }

    pub fn clear(&mut self) {
        self.plan = None;
        self.compare.clear();
        self.sql.clear();
    }
}

/// The headline with the column headers beside it, then one line per node: the node on
/// the left, cut to what is left of the width, and its figures right-aligned in columns
/// as wide as their widest entry.
#[allow(clippy::too_many_arguments)]
fn columns(
    headline: &str,
    headers: &[&str],
    rows: &[&PlanRow],
    cells: impl Fn(&PlanRow) -> Vec<String>,
    tree: bool,
    view: &PlanView,
    width: usize,
    styles: &ExplainStyles,
) -> Vec<Line<'static>> {
    let cells: Vec<Vec<String>> = rows.iter().map(|row| cells(row)).collect();
    let widths: Vec<usize> = headers
        .iter()
        .enumerate()
        .map(|(index, header)| {
            cells
                .iter()
                .filter_map(|row| row.get(index))
                .map(|cell| cell.chars().count())
                .chain([header.chars().count()])
                .max()
                .unwrap_or(0)
        })
        .collect();
    let figures: usize = widths.iter().map(|width| width + 2).sum();
    let left_width = width.saturating_sub(figures).max(12);
    let right = |cells: &[String], style: Style| -> Vec<Span<'static>> {
        cells
            .iter()
            .zip(&widths)
            .map(|(cell, width)| Span::styled(format!("  {cell:>width$}"), style))
            .collect()
    };
    let header_cells: Vec<String> = headers.iter().map(|header| header.to_string()).collect();
    let mut lines = Vec::new();
    // The headline shares a line with the headers when there is room, which is the
    // usual case; a narrow pane gives it a line of its own.
    if headline.chars().count() + 1 < left_width {
        let mut spans = fit(
            vec![(
                headline.to_string(),
                Style::default().add_modifier(Modifier::BOLD),
            )],
            left_width,
        );
        spans.extend(right(&header_cells, styles.muted));
        lines.push(Line::from(spans));
    } else {
        lines.push(Line::styled(
            headline.to_string(),
            Style::default().add_modifier(Modifier::BOLD),
        ));
        let mut spans = fit(Vec::new(), left_width);
        spans.extend(right(&header_cells, styles.muted));
        lines.push(Line::from(spans));
    }
    for (row, cells) in rows.iter().zip(&cells) {
        let mut segments = Vec::new();
        if tree {
            segments.push((row.prefix.clone(), styles.muted));
        }
        let label_style = if row.hottest {
            styles.warning.add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        segments.push((row.label.clone(), label_style));
        for tag in row_tags(row, view.analyzed, styles.unicode) {
            segments.push((format!("  {tag}"), styles.warning));
        }
        if !row.detail.is_empty() {
            segments.push((format!("  {}", row.detail), styles.muted));
        }
        let mut spans = fit(segments, left_width);
        spans.extend(right(cells, Style::default()));
        lines.push(Line::from(spans));
    }
    lines
}

/// Styled pieces cut to exactly `width` cells: padded when short, ended with `…` when
/// long, so the figures to their right stay in their columns.
fn fit(segments: Vec<(String, Style)>, width: usize) -> Vec<Span<'static>> {
    let total: usize = segments.iter().map(|(text, _)| text.chars().count()).sum();
    let mut spans = Vec::new();
    if total <= width {
        spans.extend(
            segments
                .into_iter()
                .map(|(text, style)| Span::styled(text, style)),
        );
        spans.push(Span::raw(" ".repeat(width - total)));
        return spans;
    }
    let mut room = width.saturating_sub(1);
    for (text, style) in segments {
        if room == 0 {
            break;
        }
        let piece: String = text.chars().take(room).collect();
        room -= piece.chars().count();
        spans.push(Span::styled(piece, style));
    }
    spans.push(Span::raw("…"));
    spans
}
