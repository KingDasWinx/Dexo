use dexo_tui::Model;
use dexo_tui::mouse::HitMap;
use dexo_tui::theme::{Mode, theme_for};
use ratatui::style::Color;

/// The cell under the first occurrence of `needle`, which must be on screen.
fn cell_colours(model: &Model, needle: &str) -> (Color, Color) {
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
    let mut hits = HitMap::default();
    terminal
        .draw(|frame| dexo_tui::render::render(frame, model, &mut hits))
        .unwrap();
    let buffer = terminal.backend().buffer();
    for y in 0..buffer.area.height {
        let row: String = (0..buffer.area.width)
            .map(|x| buffer[(x, y)].symbol())
            .collect();
        // Borders are multi-byte, so the byte offset is not the column.
        if let Some(offset) = row.find(needle) {
            let x = row[..offset].chars().count() as u16;
            let cell = &buffer[(x, y)];
            return (cell.fg, cell.bg);
        }
    }
    panic!("{needle:?} is not on screen");
}

/// A popup left in the terminal's own colours draws the terminal's foreground on the
/// theme's background (Dexo sets the terminal's background to it), which vanishes when
/// a dark theme meets a terminal whose text is dark.
#[test]
fn popups_paint_in_the_theme_colours_in_dark_mode() {
    let model = Model {
        theme: theme_for(Mode::Dark, "blue"),
        ..Model::default()
    };
    let base = model.theme.base(model.capabilities);
    let expected = (base.fg.unwrap(), base.bg.unwrap());

    let mut prompt = model.clone();
    prompt.document_name_prompt.open = true;
    assert_eq!(cell_colours(&prompt, "name:"), expected, "new document");

    let mut transfer = model.clone();
    transfer.transfer.open = true;
    let first_line = transfer.transfer.lines()[0].clone();
    assert_eq!(cell_colours(&transfer, &first_line), expected, "transfer");
}
