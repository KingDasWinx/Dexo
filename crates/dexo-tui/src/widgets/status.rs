use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::Paragraph;

use crate::accessibility::environment_marker;
use crate::model::Model;
use crate::theme::Role;
use dexo_driver_api::TransactionState;
use ratatui::style::Style;
use ratatui::text::{Line, Span};

pub fn render(frame: &mut Frame, area: Rect, model: &Model) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let conn = if model.connection.ready {
        format!("connected:{}", model.connection.name)
    } else if model.connection.name.is_empty() {
        "disconnected".into()
    } else {
        format!("offline:{}", model.connection.name)
    };
    let tx = match model.transaction {
        TransactionState::Idle => "tx:idle",
        TransactionState::Active => "tx:active",
        TransactionState::Failed => "tx:failed",
        TransactionState::Unknown => "tx:unknown",
    };
    let env = environment_marker(&model.connection.environment, model.capabilities.unicode);
    let env_style = model.theme.style(
        match model.connection.environment.to_ascii_lowercase().as_str() {
            "production" => Role::Production,
            "staging" => Role::Staging,
            "development" => Role::Development,
            _ => Role::Muted,
        },
        model.capabilities,
    );
    let err_style = model.theme.style(Role::Error, model.capabilities);
    let tx_style = if matches!(
        model.transaction,
        TransactionState::Failed | TransactionState::Unknown
    ) {
        err_style
    } else {
        Style::default()
    };
    let mut spans = Vec::new();
    if !env.is_empty() {
        spans.push(Span::styled(format!("{env} "), env_style));
    }
    spans.push(Span::raw(format!("{conn}  ")));
    spans.push(Span::styled(format!("{tx}  "), tx_style));
    let focus_name = match model.focus {
        crate::model::Focus::Explorer => "Explorer",
        crate::model::Focus::Editor | crate::model::Focus::Palette => "Editor",
        crate::model::Focus::Results => "Results",
        crate::model::Focus::Inspector => "Inspector",
    };
    spans.push(Span::styled(
        format!("FOCUS: {focus_name}  "),
        model.theme.status_focus(model.capabilities),
    ));
    spans.push(Span::raw(format!(
        "layout:{}  rows:{}  ctrl+p palette  F1 help",
        model.layout_preset.label(),
        model.results.row_count()
    )));
    if model.focus == crate::model::Focus::Explorer {
        let hint = model
            .explorer
            .selected_node()
            .and_then(|node| {
                crate::screens::explorer::opens_table_data(&node.kind)
                    .then_some("Enter abre a table")
            })
            .unwrap_or("Enter expande");
        spans.push(Span::raw(format!("  {hint}")));
    }
    if let Some(message) = model.messages.last() {
        spans.push(Span::raw(format!("  {message}")));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}
