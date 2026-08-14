use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::Paragraph;

use crate::model::Model;
use dexo_driver_api::TransactionState;

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
    let line = format!(
        "{conn}  {tx}  rows:{}  ctrl+p palette",
        model.results.row_count()
    );
    frame.render_widget(Paragraph::new(line), area);
}
