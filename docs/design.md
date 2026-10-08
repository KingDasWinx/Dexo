# Forms in Dexo's TUI

Every form in the TUI is built the same way: creating or editing a connection, a profile, an agent's write access, a setting. This page is that pattern, the inputs it is made of, and the code for each one.

It came from a New Grant form drawn four ways and compared in a real terminal. The patterns it borrows are from projects people use daily: lazydb's profile form (the label column, side-by-side choices, the button row) and sqlit's connection form (boxed inputs, dropdowns).

The inputs are built in `crates/dexo-tui/src/widgets/panel.rs`: a form is a `panel::Lines`, filled row by row, then drawn with `panel::draw`. Agents → Profiles' four forms -- New profile, Edit, Writes and Connect agent -- are made with it, in `crates/dexo-tui/src/screens/mcp_profiles.rs`. They run on their own, with the Profiles page they open from:

```sh
cargo run -p dexo-tui --example agents_styles    # n, Enter, w and a open the forms
```

## The whole form

```
┌▸ Writes of assistant───────────────────────────────────────────────┐
│                                                                    │
│ WHERE                                                              │
│                   ┌──────────────────────────────────────────┐     │
│   Connection      │ pg-dev        development ✎ has writes ▼ │     │
│                   └──────────────────────────────────────────┘     │
│                    pg-prod is not offered: production never takes  │
│                    agents' writes                                  │
│                    covers everything assistant sees there          │
│ WHAT                                                               │
│ › May              [x] change rows         [x] change schema       │
│                    [ ] end sessions                                │
│                    change rows: insert, update, delete, and SQL    │
│                    that writes                                     │
│ HOW LONG                                                           │
│   For              15m   1h   2h   8h   24h   Until revoked        │
│                                                                    │
│ RESULT                                                             │
│   assistant may change rows and the schema in pg-dev, asking you   │
│   before each write, for 1 hour.                                   │
│   Each write waits up to 2 min on Approvals, then is refused.      │
│   Replaces what it has there now: rows, schema · 28 min left.      │
│                                                                    │
│                  [   Cancel ]  [   Change writes ]                 │
│ ↑/↓/←/→ move   Space check   Tab next   Esc cancel                 │
└────────────────────────────────────────────────────────────────────┘
```

## Layout

- **Where.** A form takes the pane it is about -- Writes takes the profile's page, next to the list of profiles -- and is drawn in that pane's own frame, focused: `┌▸ Writes of assistant`. It is never a window over the screen.
- **Height.** The frame fills the pane; the buttons follow the last field, not the pane's foot. On a short pane the fields scroll to keep the focused row, and all of its box, in view; with the focus on the buttons, the end of the form is what shows. The buttons and the hint row never scroll.
- **Sections.** The fields are grouped by the question they answer -- WHO, WHERE, WHAT, HOW LONG, and CONFIRM when the action asks for it -- then RESULT. Section titles are uppercase, bold, in `Border`, one column in from the frame.
- **Label column.** Every row starts with ` › ` (the `›` only on the focused row, in bold `Focus`), then the label padded to 16 columns. The focused label is bold `Focus`; the others are `Muted`. Values start at column 19.
- **One field per row.** No two fields side by side: the eye goes down one column.
- **A blank line, then the buttons, then the hint row**, at the bottom of the frame.

## Choosing an input

| What the person gives | Input |
|---|---|
| One value from a list that grows: profiles, connections, databases | [Dropdown](#dropdown) |
| One value from five or fewer short, fixed options | [Segmented choice](#segmented-choice) |
| Several values from a small fixed set | [Checkbox grid](#checkbox-grid) |
| A yes or no that changes what happens | [Toggle row](#toggle-row) |
| Free text: a name, a selector, a number | [Text box](#text-box) |
| Proof of intent before a risky write | [Typed confirmation](#typed-confirmation) |

Never use `< value >` cycled with the arrows: the person cannot see the other options, and cannot tell it is a choice.

## Inputs

### Dropdown

```
                  ┌──────────────────────────────────────────┐
 › Connection     │ pg-commerce  production                ▼ │
                  ┌──────────────────────────────────────────┐
                  │ pg-commerce   production                 │   ← highlighted: OnFocus on Focus
                  │ pg-staging    staging                    │
                  └──────────────────────────────────────────┘
```

- **Width.** The box is as wide as a name needs: the form's value column, at most 44.
- **Value.** It is followed by `▼` in `Muted` at the right edge. Extra facts about the value come after it, coloured by their role, like `production` in `Production`.
- **The open list.** It hangs from the box's bottom border, as wide as the box, and is drawn last, over what is below it (`Clear` first). The highlighted row is `OnFocus` on `Focus`.
- **Keys.** Enter or Space opens the list. ←/→ change the value without opening it. While it is open, ↑/↓ move, Enter or Space picks, and Esc or Tab closes.

### Segmented choice

```
 › For              15m   1h   2h   8h   24h   Until revoked
```

- **Options.** Each one is padded by a space on each side, with one space between them. The options take as much room as they need: no fixed width per option.
- **The chosen option.** It is bold `OnFocus` on `Focus` while the row has the focus, `OnSelection` on `Selection` otherwise. The other options are `Muted`.
- **Description.** After the options, the chosen option's description, in `Muted`.
- **Keys.** ←/→ choose. Space moves to the next option.

### Checkbox grid

```
 › May              [x] change rows            [x] change schema
                    [ ] end sessions
```

- **Grid.** Two columns, each cell 22 columns wide. The `x` is bold `Focus`.
- **The cell under the cursor.** It is `OnSelection` on `Selection` while the row has the focus.
- **Keys.**
  - ←/→ move between the columns.
  - ↑/↓ move between the rows. Past the first or the last row they leave the grid for the field above or below.
  - Coming in from below lands on the last row, in the same column.
  - Space or Enter toggles. Tab leaves the grid.

### Toggle row

```
 › Skill file       [x] On   tells the agent how Dexo's tools behave
                    [ ] Off  no skill file
```

- **State.** `[x] On` or `[ ] Off`, then what that state means, in `Muted`. The person reads the consequence, not only the switch.
- **Fields it opens.** A field that only matters when the toggle is on appears right under it, and is left out of the focus order while the toggle is off.
- **Keys.** Space or Enter toggles.

### Text box

```
                  ┌──────────────────────────────────────────────────────┐
 › Name           │ assistant-2                                          │
                  └──────────────────────────── letters, digits, - and _ ─┘
```

- **Box.** It fills the value column.
- **Hint.** What the field expects goes in the bottom border, at the right, in `Muted`. It costs no extra row. A unit goes after the text, also `Muted`: `120 s`.
- **Border.** `Focus` while focused, `Error` when invalid, `Border` otherwise.
- **Cursor.** The terminal cursor sits after the text.
- **Keys.** Typing edits the text. Enter or Tab moves to the next field.

### Typed confirmation

```
 CONFIRM
 › Type 'pg-commerce' to confirm
   ┌──────────────────────────────────────────┐
   │ pg-com█                                  │
   └──────────────────────────────────────────┘
```

- **When.** Only for an action that is hard to undo or writes somewhere that matters, such as a `DELETE` without `WHERE` on a production connection.
- **Instruction.** It says exactly what to type, quoted, with the quoted part in bold. It follows the form: change the connection and the name in it changes.
- **Box.** It sits under the instruction, at the section's indent and as wide as a dropdown. It has no placeholder and no hint.

## The rest of the form

### Result

```
 RESULT
   assistant may change rows in pg-dev, asking you before each write, until you revoke it.
   ! no time limit: r on the connection takes it back
```

The form says in plain words what it will do, and the sentence updates as the person changes the fields. When the action is risky, a bold `Warning` line says why.

### Errors

```
 × check what it may change
```

- **Where.** On the line before RESULT, in bold `Error`.
- **The field.** Its box gets the `Error` border, and the focus moves to it, so the fix is where the cursor is.
- **Clearing.** The message goes the moment the person edits that field.

### Buttons

```
                       [   Cancel ]  [ > Allow writes ]
```

- **Row.** Centred, with two spaces between the buttons. Cancel goes first and the primary action last.
- **The `>` marker.** A focused button shows `>`, and every button keeps room for it, so nothing shifts when the focus moves.
- **Styles.**
  - Focused: bold `OnFocus` on the button's tone.
  - The primary button, unfocused: bold, in its tone.
  - Cancel, unfocused: `Muted`.
- **Tone.** `Focus` by default; `Warning` when the action writes to a production connection; `Error` when it deletes.

### Hint row

The last row of the frame names the keys of the control that has the focus, and only those:

| Focus on | Hints |
|---|---|
| Dropdown | `Enter open   ←/→ change   Tab next   Esc cancel` |
| Open dropdown | `↑/↓ choose   Enter pick   Esc close` |
| Segmented choice | `←/→ choose   Tab next   Esc cancel` |
| Checkbox grid | `↑/↓/←/→ move   Space/Enter toggle   Tab next   Esc cancel` |
| Toggle row | `Space/Enter toggle   Tab next   Esc cancel` |
| Text box | `Enter/Tab next   Shift+Tab back   Esc cancel` |
| Primary button | `Enter allow   ←/→ switch   Esc cancel` |
| Cancel | `Enter cancel   ←/→ switch` |

Each key is bold `Focus` and its description is `Muted`, with three spaces between pairs.

## Keys

| Key | Does |
|---|---|
| Tab / Shift+Tab | The next or previous control, buttons included |
| ↓ / ↑ | The next or previous row. Inside a checkbox grid they move between the grid's rows first. |
| ← / → | Change a choice, move in a grid, switch between the buttons |
| Space | Toggle, open a dropdown, or move to the next option |
| Enter | Open a dropdown or pick in it, toggle, or go to the next field. On the primary button it submits; on Cancel it cancels. |
| Esc | Close the open dropdown; otherwise cancel the form |

**Enter submits only from the primary button.** A person tabbing through the form never creates anything by accident.

## Colours

Only theme roles, never literal colours, so every theme and `NO_COLOR` work:

| Role | Used for |
|---|---|
| `Focus` | The frame, the focused label and box, the `›` marker, keys in hints, checked marks |
| `Border` | Section titles, unfocused boxes |
| `Muted` | Unfocused labels, hints, descriptions, `▼`, unchosen options |
| `Foreground` | Values and typed text |
| `OnFocus` on `Focus` | The chosen option while focused, the focused button, the highlighted dropdown row |
| `OnSelection` on `Selection` | The chosen option while unfocused, the grid's cursor cell, the focused toggle |
| `Error`, `Warning`, `Success` | Errors, risks, a done result |
| `Production`, `Staging`, `Development` | A connection's environment |

Colour is never the only signal: the focused row also has `›`, the focused button `>`, the checked box `x`, and an error `×`.

## Code

These are the building blocks, drawn on their own; `widgets/panel.rs` has them against the model, and `screens/mcp_profiles.rs` has Profiles' forms built on them. `Ui` holds the theme and the terminal's capabilities; in the TUI, `model.theme.style(role, model.capabilities)` is the same thing as `ui.fg(role)`.

### Colours

```rust
struct Ui {
    theme: Theme,
    caps: TerminalCapabilities,
}

impl Ui {
    fn fg(&self, role: Role) -> Style {
        self.theme.style(role, self.caps)
    }

    fn bold(&self, role: Role) -> Style {
        self.fg(role).add_modifier(Modifier::BOLD)
    }

    /// `fg` on `bg`; reversed when the terminal has no colour.
    fn on(&self, fg: Role, bg: Role) -> Style {
        match self.theme.color(bg, self.caps) {
            Some(color) => self.fg(fg).bg(color),
            None => Style::default().add_modifier(Modifier::REVERSED),
        }
    }
}
```

### The focus order

The order is computed from the form's state, so a field that does not apply is not in it -- New profile's, in `screens/mcp_profiles.rs`:

```rust
pub fn rows(&self) -> Vec<ProfileRow> {
    let mut rows = Vec::new();
    if !self.editing {
        rows.push(ProfileRow::Name);
    }
    rows.extend([ProfileRow::Connections, ProfileRow::Reads, ProfileRow::Sees]);
    if self.sees != Sees::All {
        rows.push(ProfileRow::Tables); // only while it sees some tables, not all
    }
    if !self.editing {
        rows.push(ProfileRow::Agent);
        if self.agent_choice().is_some_and(|agent| agent.skill) {
            rows.push(ProfileRow::Skill);
        }
    }
    rows.extend([ProfileRow::Cancel, ProfileRow::Save]);
    rows
}
```

### Section and label

```rust
const LABEL: usize = 16;

fn section(ui: &Ui, title: &str) -> Line<'static> {
    Line::styled(format!(" {title}"), ui.bold(Role::Border))
}

/// ` › Label` padded to the value column; the marker only on the focused row.
fn label(ui: &Ui, focused: bool, label: &str) -> Vec<Span<'static>> {
    vec![
        Span::styled(if focused { " › " } else { "   " }, ui.bold(Role::Focus)),
        Span::styled(
            format!("{label:<LABEL$}"),
            if focused { ui.bold(Role::Focus) } else { ui.fg(Role::Muted) },
        ),
    ]
}
```

### Segmented choice

```rust
fn segmented(ui: &Ui, options: &[&str], chosen: usize, focused: bool) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    for (index, option) in options.iter().enumerate() {
        let style = match (index == chosen, focused) {
            (true, true) => ui.on(Role::OnFocus, Role::Focus).add_modifier(Modifier::BOLD),
            (true, false) => ui.on(Role::OnSelection, Role::Selection),
            _ => ui.fg(Role::Muted),
        };
        spans.push(Span::styled(format!(" {option} "), style));
        spans.push(Span::raw(" "));
    }
    spans
}
```

### Boxes: text boxes and dropdowns

A box is three lines of the form's paragraph, so it scrolls with the rest of the form:

```rust
/// A box as three lines: its top, `label` beside its middle, and its foot with `foot` at
/// the right. The label's width is where the box starts.
fn boxed(
    ui: &Ui,
    label: Vec<Span<'static>>,
    width: usize,
    content: Vec<Span<'static>>,
    foot: &str,
    border: Role,
) -> [Line<'static>; 3] {
    let indent: usize = label.iter().map(|span| span.content.chars().count()).sum();
    let edge = ui.fg(border);
    let pad = " ".repeat(indent);
    let inside = width.saturating_sub(2);
    let top = Line::from(vec![
        Span::raw(pad.clone()),
        Span::styled(format!("┌{}┐", "─".repeat(inside)), edge),
    ]);
    let used: usize = content.iter().map(|span| span.content.chars().count()).sum();
    let mut middle = label;
    middle.push(Span::styled("│ ", edge));
    middle.extend(content);
    middle.push(Span::raw(" ".repeat(inside.saturating_sub(used + 1))));
    middle.push(Span::styled("│", edge));
    let foot = if foot.is_empty() { String::new() } else { format!(" {foot} ") };
    let rule = inside.saturating_sub(foot.chars().count() + 1);
    let bottom = Line::from(vec![
        Span::raw(pad),
        Span::styled(format!("└{}", "─".repeat(rule)), edge),
        Span::styled(foot, ui.fg(Role::Muted)),
        Span::styled("─┘", edge),
    ]);
    [top, Line::from(middle), bottom]
}
```

A text box, with its hint in the foot:

```rust
let border = if invalid { Role::Error } else if focused { Role::Focus } else { Role::Border };
lines.extend(boxed(
    ui,
    label(ui, focused, "Name"),
    width,
    vec![Span::styled(name.clone(), ui.fg(Role::Foreground))],
    "letters, digits, - and _",
    border,
));
```

A dropdown is the same box, narrower, with `▼` at the right of its content:

```rust
let narrow = width.min(44);
let mut value = vec![Span::styled(name.to_string(), ui.fg(Role::Foreground))];
let used: usize = value.iter().map(|span| span.content.chars().count()).sum();
value.push(Span::raw(" ".repeat(narrow.saturating_sub(used + 5))));
value.push(Span::styled("▼ ", ui.fg(Role::Muted)));
lines.extend(boxed(ui, label(ui, focused, "Profile"), narrow, value, "", border));
```

Its open list is drawn after everything else, hanging from the box's bottom line:

```rust
let list = Rect {
    x: body.x + 19,                                   // the value column
    y: body.y + (bottom_line - offset) as u16,        // over the box's foot
    width: narrow as u16,
    height: options.len() as u16 + 2,
};
let rows: Vec<Line> = options
    .iter()
    .enumerate()
    .map(|(index, option)| {
        let text = format!(" {option:<w$}", w = narrow.saturating_sub(3));
        if index == highlight {
            Line::styled(text, ui.on(Role::OnFocus, Role::Focus))
        } else {
            Line::styled(text, ui.fg(Role::Foreground))
        }
    })
    .collect();
frame.render_widget(Clear, list);
frame.render_widget(
    Paragraph::new(rows).block(Block::bordered().border_style(ui.fg(Role::Focus))),
    list,
);
```

### Typed confirmation

```rust
lines.push(section(ui, "CONFIRM"));
let style = if focused { ui.bold(Role::Focus) } else { ui.fg(Role::Foreground) };
lines.push(Line::from(vec![
    Span::styled(if focused { " › " } else { "   " }, ui.bold(Role::Focus)),
    Span::styled("Type '", style),
    Span::styled(target.to_string(), ui.bold(Role::Foreground)),
    Span::styled("' to confirm", style),
]));
lines.extend(boxed(
    ui,
    vec![Span::raw("   ")],
    narrow,
    vec![Span::styled(typed.clone(), ui.fg(Role::Foreground))],
    "",
    border,
));
```

### Buttons and hints

```rust
/// `[   Label ]`, or `[ > Label ]` focused: the marker keeps its room either way.
fn button(ui: &Ui, label: &str, focused: bool, tone: Role, primary: bool) -> Span<'static> {
    if focused {
        Span::styled(format!("[ > {label} ]"), ui.on(Role::OnFocus, tone).add_modifier(Modifier::BOLD))
    } else if primary {
        Span::styled(format!("[   {label} ]"), ui.bold(tone))
    } else {
        Span::styled(format!("[   {label} ]"), ui.fg(Role::Muted))
    }
}

fn hints(ui: &Ui, pairs: &[(&str, &str)]) -> Line<'static> {
    let mut spans = Vec::new();
    for (index, (key, what)) in pairs.iter().enumerate() {
        if index > 0 {
            spans.push(Span::raw("   "));
        }
        spans.push(Span::styled(key.to_string(), ui.bold(Role::Focus)));
        spans.push(Span::styled(format!(" {what}"), ui.fg(Role::Muted)));
    }
    Line::from(spans)
}

let tone = if production { Role::Warning } else { Role::Focus };
let row = Line::from(vec![
    button(ui, "Cancel", focus == Row::Cancel, Role::Focus, false),
    Span::raw("  "),
    button(ui, "Allow writes", focus == Row::Create, tone, true),
])
.alignment(Alignment::Center);
```

### Keys

The rule that matters most: Enter submits only from the primary button. A form's `key` gives the focused row its keys first -- an open dropdown, a grid's arrows, a segment's Left and Right, typing -- and the rest to the walk every form shares, in `screens/mcp_profiles.rs`:

```rust
fn walk_key<R: Copy + PartialEq>(
    rows: &[R],
    focus: &mut R,
    cancel: R,
    submit: R,
    key: KeyEvent,
) -> Option<FormKey> {
    match key.code {
        KeyCode::Esc => return Some(FormKey::Cancel),
        KeyCode::Tab | KeyCode::Down => *focus = walk(rows, *focus, 1),
        KeyCode::BackTab | KeyCode::Up => *focus = walk(rows, *focus, -1),
        KeyCode::Left | KeyCode::Right if *focus == cancel || *focus == submit => {
            *focus = if *focus == cancel { submit } else { cancel };
        }
        // The only Enter that submits.
        KeyCode::Enter if *focus == submit => return Some(FormKey::Submit),
        KeyCode::Enter if *focus == cancel => return Some(FormKey::Cancel),
        _ => return None, // the focused row's: Enter on it moves on
    }
    Some(FormKey::Stay)
}
```

### Drawing: in the pane, the buttons after the last row, scroll to the focus

```rust
// Build every line first, noting the focused row's line and the cursor.
let block = crate::render::pane_block(model, "Writes of assistant", true); // the pane's own frame
let inner = block.inner(area);
frame.render_widget(block, area);
let rows = (lines.len() as u16).min(inner.height.saturating_sub(3));
let [body, _, buttons, hints_row, _] = Layout::vertical([
    Constraint::Length(rows),
    Constraint::Length(1),
    Constraint::Length(1),
    Constraint::Length(1),
    Constraint::Min(0),
])
.areas(inner);
// Keep the focused row, and the box under it, in view.
let offset = (focus_line + 2).saturating_sub(usize::from(body.height));
frame.render_widget(Paragraph::new(lines).scroll((offset as u16, 0)), body);
if let Some((column, line)) = cursor && line >= offset {
    frame.set_cursor_position((body.x + column as u16, body.y + (line - offset) as u16));
}
```

## Checklist for a new form

- [ ] Fields grouped into sections by the question they answer.
- [ ] Each input chosen from [Choosing an input](#choosing-an-input). No `< value >`.
- [ ] Fields that do not apply are hidden and left out of the focus order.
- [ ] A RESULT that says what will happen, and a `Warning` line when it is risky.
- [ ] A typed confirmation when the action is hard to undo.
- [ ] Enter submits only from the primary button.
- [ ] The hint row follows the focused control.
- [ ] An error names the fix, marks the field, and moves the focus to it.
- [ ] Theme roles only, and no state carried by colour alone.
- [ ] Usable at 80×24: the form scrolls, and the buttons stay.
