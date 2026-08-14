use dexo_tui::capabilities::{ColorDepth, TerminalCapabilities};
use dexo_tui::theme::{builtin_dark, builtin_light, builtin_low_color, preview_lines};

fn caps(depth: ColorDepth, unicode: bool) -> TerminalCapabilities {
    TerminalCapabilities {
        color_depth: depth,
        unicode,
        mouse: false,
    }
}

fn assert_distinguishable(text: &str) {
    let prod = text
        .lines()
        .find(|line| line.starts_with("production"))
        .unwrap();
    let err = text.lines().find(|line| line.starts_with("error")).unwrap();
    let sel = text
        .lines()
        .find(|line| line.starts_with("selection"))
        .unwrap();
    let prod_mark = prod.split_whitespace().nth(1).unwrap();
    let err_mark = err.split_whitespace().nth(1).unwrap();
    let sel_mark = sel.split_whitespace().nth(1).unwrap();
    assert_ne!(prod_mark, err_mark);
    assert_ne!(prod_mark, sel_mark);
    assert_ne!(err_mark, sel_mark);
}

#[test]
fn snapshot_truecolor_dark() {
    let text = preview_lines(&builtin_dark(), caps(ColorDepth::TrueColor, true));
    assert_distinguishable(&text);
    insta::assert_snapshot!(text);
}

#[test]
fn snapshot_256_light() {
    let text = preview_lines(&builtin_light(), caps(ColorDepth::Ansi256, true));
    assert_distinguishable(&text);
    insta::assert_snapshot!(text);
}

#[test]
fn snapshot_16_low_color() {
    let text = preview_lines(&builtin_low_color(), caps(ColorDepth::Ansi16, false));
    assert_distinguishable(&text);
    insta::assert_snapshot!(text);
}

#[test]
fn snapshot_ascii_fallback() {
    let text = preview_lines(&builtin_dark(), caps(ColorDepth::Ansi16, false));
    assert!(text.contains("[PROD]"));
    assert!(text.contains("[ERR]"));
    assert!(text.contains(">"));
    assert_distinguishable(&text);
    insta::assert_snapshot!(text);
}

#[test]
fn snapshot_no_color() {
    let text = preview_lines(&builtin_dark(), caps(ColorDepth::None, false));
    assert!(text.contains("color=none"));
    assert_distinguishable(&text);
    insta::assert_snapshot!(text);
}
