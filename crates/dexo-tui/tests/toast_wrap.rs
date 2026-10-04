//! A toast carries a whole message. The one that said what to do after a failed pre-connect
//! command was one line cut at the border of the screen, with the useful half beyond it.
use dexo_tui::{Model, render::render_to_string};

#[test]
fn a_long_error_wraps_inside_the_screen_and_is_readable_to_its_end() {
    let mut model = Model::default();
    model.apply_size(100, 30);
    model.messages.error(
        "x3: pre-connect command `touch /tmp/claude-1000/qa/some/long/path/marker` went to the \
         background; it has to keep running in the foreground (drop -f or &), so Dexo can stop \
         it with the session"
            .into(),
    );
    let screen = render_to_string(&model, 100, 30);
    for part in [
        "pre-connect",
        "background;",
        "foreground",
        "(drop",
        "&),",
        "session",
    ] {
        assert!(screen.contains(part), "{part:?} is cut off:\n{screen}");
    }
    // Inside the screen: no line is wider than it, and the sidebar's frame is not drawn over.
    assert!(screen.lines().all(|line| line.chars().count() <= 100));
    let toast_rows = screen
        .lines()
        .filter(|line| line.contains("pre-connect"))
        .count();
    assert_eq!(toast_rows, 1, "the toast is drawn once:\n{screen}");
}
