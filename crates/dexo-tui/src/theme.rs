use std::collections::HashMap;
use std::path::Path;

use ratatui::style::{Color, Style};
use serde::Deserialize;

use crate::capabilities::{ColorDepth, TerminalCapabilities};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Role {
    Background,
    Foreground,
    Border,
    Muted,
    Production,
    Staging,
    Development,
    Error,
    Warning,
    Success,
    Selection,
    Focus,
}

impl Role {
    fn as_key(self) -> &'static str {
        match self {
            Self::Background => "background",
            Self::Foreground => "foreground",
            Self::Border => "border",
            Self::Muted => "muted",
            Self::Production => "production",
            Self::Staging => "staging",
            Self::Development => "development",
            Self::Error => "error",
            Self::Warning => "warning",
            Self::Success => "success",
            Self::Selection => "selection",
            Self::Focus => "focus",
        }
    }

    fn all() -> &'static [Role] {
        &[
            Self::Background,
            Self::Foreground,
            Self::Border,
            Self::Muted,
            Self::Production,
            Self::Staging,
            Self::Development,
            Self::Error,
            Self::Warning,
            Self::Success,
            Self::Selection,
            Self::Focus,
        ]
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ThemeKind {
    Light,
    Dark,
    LowColor,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RolePalette {
    ansi16: Color,
    ansi256: Color,
    truecolor: Color,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    pub name: String,
    pub kind: ThemeKind,
    slots: HashMap<Role, RolePalette>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ThemeError {
    pub field: String,
    pub reason: String,
}

impl std::fmt::Display for ThemeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.field, self.reason)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct LoadedTheme {
    pub theme: Theme,
    pub error: Option<ThemeError>,
}

#[derive(Deserialize)]
struct ThemeToml {
    name: Option<String>,
    kind: Option<String>,
    #[serde(default)]
    roles: HashMap<String, String>,
}

impl Theme {
    pub fn style(&self, role: Role, caps: TerminalCapabilities) -> Style {
        let Some(slot) = self.slots.get(&role) else {
            return Style::default();
        };
        match caps.color_depth {
            ColorDepth::None => Style::default(),
            ColorDepth::Ansi16 => Style::default().fg(slot.ansi16),
            ColorDepth::Ansi256 => Style::default().fg(slot.ansi256),
            ColorDepth::TrueColor => Style::default().fg(slot.truecolor),
        }
    }

    pub fn color(&self, role: Role, caps: TerminalCapabilities) -> Option<Color> {
        let slot = self.slots.get(&role)?;
        match caps.color_depth {
            ColorDepth::None => None,
            ColorDepth::Ansi16 => Some(slot.ansi16),
            ColorDepth::Ansi256 => Some(slot.ansi256),
            ColorDepth::TrueColor => Some(slot.truecolor),
        }
    }
}

pub fn builtin_dark() -> Theme {
    theme(
        "dark",
        ThemeKind::Dark,
        &[
            (Role::Background, named(Color::Black, 235, 18, 18, 18)),
            (Role::Foreground, named(Color::White, 252, 230, 230, 230)),
            (Role::Border, named(Color::DarkGray, 240, 80, 80, 80)),
            (Role::Muted, named(Color::Gray, 245, 140, 140, 140)),
            (Role::Production, named(Color::Red, 160, 220, 50, 50)),
            (Role::Staging, named(Color::Yellow, 178, 200, 160, 40)),
            (Role::Development, named(Color::Cyan, 81, 40, 180, 180)),
            (Role::Error, named(Color::LightRed, 196, 240, 70, 70)),
            (Role::Warning, named(Color::Yellow, 214, 230, 180, 40)),
            (Role::Success, named(Color::Green, 40, 60, 180, 80)),
            (Role::Selection, named(Color::Blue, 33, 50, 110, 210)),
            (Role::Focus, named(Color::Magenta, 135, 180, 80, 200)),
        ],
    )
}

pub fn builtin_light() -> Theme {
    theme(
        "light",
        ThemeKind::Light,
        &[
            (Role::Background, named(Color::White, 255, 250, 250, 250)),
            (Role::Foreground, named(Color::Black, 232, 20, 20, 20)),
            (Role::Border, named(Color::Gray, 249, 180, 180, 180)),
            (Role::Muted, named(Color::DarkGray, 243, 100, 100, 100)),
            (Role::Production, named(Color::Red, 124, 180, 20, 20)),
            (Role::Staging, named(Color::Yellow, 172, 160, 110, 0)),
            (Role::Development, named(Color::Blue, 27, 0, 90, 160)),
            (Role::Error, named(Color::LightRed, 160, 190, 0, 0)),
            (Role::Warning, named(Color::Yellow, 178, 170, 110, 0)),
            (Role::Success, named(Color::Green, 28, 0, 130, 50)),
            (Role::Selection, named(Color::Blue, 27, 20, 80, 180)),
            (Role::Focus, named(Color::Magenta, 127, 120, 20, 140)),
        ],
    )
}

pub fn builtin_low_color() -> Theme {
    theme(
        "low-color",
        ThemeKind::LowColor,
        &[
            (Role::Background, named(Color::Reset, 0, 0, 0, 0)),
            (Role::Foreground, named(Color::White, 7, 200, 200, 200)),
            (Role::Border, named(Color::DarkGray, 8, 120, 120, 120)),
            (Role::Muted, named(Color::Gray, 8, 140, 140, 140)),
            (Role::Production, named(Color::Red, 1, 180, 40, 40)),
            (Role::Staging, named(Color::Yellow, 3, 180, 180, 40)),
            (Role::Development, named(Color::Cyan, 6, 40, 160, 160)),
            (Role::Error, named(Color::Red, 1, 200, 40, 40)),
            (Role::Warning, named(Color::Yellow, 3, 200, 180, 40)),
            (Role::Success, named(Color::Green, 2, 40, 160, 40)),
            (Role::Selection, named(Color::Blue, 4, 40, 80, 180)),
            (Role::Focus, named(Color::Magenta, 5, 160, 40, 160)),
        ],
    )
}

pub fn builtin_for_depth(depth: ColorDepth) -> Theme {
    match depth {
        ColorDepth::None | ColorDepth::Ansi16 => builtin_low_color(),
        ColorDepth::Ansi256 | ColorDepth::TrueColor => builtin_dark(),
    }
}

pub fn parse_theme(src: &str) -> Result<Theme, ThemeError> {
    let parsed: ThemeToml = toml::from_str(src).map_err(|err| ThemeError {
        field: field_from_toml_error(&err),
        reason: err.message().to_string(),
    })?;
    let kind = match parsed.kind.as_deref().unwrap_or("dark") {
        "dark" => ThemeKind::Dark,
        "light" => ThemeKind::Light,
        "low-color" | "lowcolor" => ThemeKind::LowColor,
        other => {
            return Err(ThemeError {
                field: "kind".into(),
                reason: format!("unknown theme kind `{other}`"),
            });
        }
    };
    let mut base = match kind {
        ThemeKind::Dark => builtin_dark(),
        ThemeKind::Light => builtin_light(),
        ThemeKind::LowColor => builtin_low_color(),
    };
    base.name = parsed.name.unwrap_or(base.name);
    base.kind = kind;
    for (key, value) in parsed.roles {
        let role = parse_role(&key)?;
        let color = parse_color(&value).map_err(|reason| ThemeError {
            field: format!("roles.{key}"),
            reason,
        })?;
        base.slots.insert(
            role,
            RolePalette {
                ansi16: color.ansi16,
                ansi256: color.ansi256,
                truecolor: color.truecolor,
            },
        );
    }
    Ok(base)
}

pub fn load_theme_file(path: &Path, fallback: Theme) -> LoadedTheme {
    let Ok(src) = std::fs::read_to_string(path) else {
        return LoadedTheme {
            theme: fallback,
            error: None,
        };
    };
    match parse_theme(&src) {
        Ok(theme) => LoadedTheme { theme, error: None },
        Err(error) => LoadedTheme {
            theme: fallback,
            error: Some(error),
        },
    }
}

pub fn preview_lines(theme: &Theme, caps: TerminalCapabilities) -> String {
    let mut lines = vec![format!(
        "theme={} kind={:?} depth={:?} unicode={}",
        theme.name, theme.kind, caps.color_depth, caps.unicode
    )];
    for role in [Role::Production, Role::Error, Role::Selection] {
        let marker = crate::accessibility::marker(role, caps.unicode);
        let color = theme
            .color(role, caps)
            .map(format_color)
            .unwrap_or_else(|| "none".into());
        lines.push(format!("{} {} color={color}", role.as_key(), marker));
    }
    lines.join("\n")
}

fn named(ansi16: Color, indexed: u8, r: u8, g: u8, b: u8) -> RolePalette {
    RolePalette {
        ansi16,
        ansi256: Color::Indexed(indexed),
        truecolor: Color::Rgb(r, g, b),
    }
}

fn theme(name: &str, kind: ThemeKind, slots: &[(Role, RolePalette)]) -> Theme {
    Theme {
        name: name.into(),
        kind,
        slots: slots.iter().copied().collect(),
    }
}

struct ParsedColor {
    ansi16: Color,
    ansi256: Color,
    truecolor: Color,
}

fn parse_role(key: &str) -> Result<Role, ThemeError> {
    Role::all()
        .iter()
        .copied()
        .find(|role| role.as_key() == key)
        .ok_or_else(|| ThemeError {
            field: format!("roles.{key}"),
            reason: format!("unknown role `{key}`"),
        })
}

fn parse_color(value: &str) -> Result<ParsedColor, String> {
    let value = value.trim();
    if let Some(hex) = value.strip_prefix('#') {
        if hex.len() != 6 {
            return Err("hex color must be #RRGGBB".into());
        }
        let r = u8::from_str_radix(&hex[0..2], 16).map_err(|_| "invalid hex color")?;
        let g = u8::from_str_radix(&hex[2..4], 16).map_err(|_| "invalid hex color")?;
        let b = u8::from_str_radix(&hex[4..6], 16).map_err(|_| "invalid hex color")?;
        return Ok(ParsedColor {
            ansi16: nearest_ansi16(r, g, b),
            ansi256: Color::Indexed(nearest_ansi256(r, g, b)),
            truecolor: Color::Rgb(r, g, b),
        });
    }
    if let Some(index) = value.strip_prefix("ansi:") {
        let n: u8 = index.parse().map_err(|_| "invalid ansi index")?;
        return Ok(ParsedColor {
            ansi16: Color::Indexed(n.min(15)),
            ansi256: Color::Indexed(n),
            truecolor: Color::Indexed(n),
        });
    }
    let named = match value.to_ascii_lowercase().as_str() {
        "black" => Color::Black,
        "red" => Color::Red,
        "green" => Color::Green,
        "yellow" => Color::Yellow,
        "blue" => Color::Blue,
        "magenta" => Color::Magenta,
        "cyan" => Color::Cyan,
        "gray" | "grey" => Color::Gray,
        "darkgray" | "darkgrey" => Color::DarkGray,
        "lightred" => Color::LightRed,
        "lightgreen" => Color::LightGreen,
        "lightyellow" => Color::LightYellow,
        "lightblue" => Color::LightBlue,
        "lightmagenta" => Color::LightMagenta,
        "lightcyan" => Color::LightCyan,
        "white" => Color::White,
        "reset" => Color::Reset,
        _ => return Err(format!("unknown color `{value}`")),
    };
    Ok(ParsedColor {
        ansi16: named,
        ansi256: named,
        truecolor: named,
    })
}

fn nearest_ansi16(r: u8, g: u8, b: u8) -> Color {
    const TABLE: [(Color, u8, u8, u8); 8] = [
        (Color::Black, 0, 0, 0),
        (Color::Red, 205, 0, 0),
        (Color::Green, 0, 205, 0),
        (Color::Yellow, 205, 205, 0),
        (Color::Blue, 0, 0, 238),
        (Color::Magenta, 205, 0, 205),
        (Color::Cyan, 0, 205, 205),
        (Color::White, 229, 229, 229),
    ];
    TABLE
        .iter()
        .min_by_key(|(_, cr, cg, cb)| dist(r, g, b, *cr, *cg, *cb))
        .map(|(c, ..)| *c)
        .unwrap_or(Color::White)
}

fn nearest_ansi256(r: u8, g: u8, b: u8) -> u8 {
    // ponytail: cube snapshot is enough; upgrade to a real 256-color table if themes need gray ramp fidelity.
    let ri = (u16::from(r) * 5 / 255) as u8;
    let gi = (u16::from(g) * 5 / 255) as u8;
    let bi = (u16::from(b) * 5 / 255) as u8;
    16 + 36 * ri + 6 * gi + bi
}

fn dist(r: u8, g: u8, b: u8, cr: u8, cg: u8, cb: u8) -> u32 {
    let dr = i32::from(r) - i32::from(cr);
    let dg = i32::from(g) - i32::from(cg);
    let db = i32::from(b) - i32::from(cb);
    (dr * dr + dg * dg + db * db) as u32
}

fn format_color(color: Color) -> String {
    match color {
        Color::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
        Color::Indexed(n) => format!("ansi:{n}"),
        other => format!("{other:?}"),
    }
}

fn field_from_toml_error(err: &toml::de::Error) -> String {
    match err.span() {
        Some(span) => format!("toml[{}..{}]", span.start, span.end),
        None => "theme".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Role, ThemeKind, builtin_dark, builtin_light, builtin_low_color, load_theme_file,
        parse_theme, preview_lines,
    };
    use crate::capabilities::{ColorDepth, TerminalCapabilities};

    #[test]
    fn invalid_role_color_names_field_and_keeps_file() {
        let path = std::env::temp_dir().join(format!(
            "dexo-theme-invalid-{}.toml",
            std::process::id()
        ));
        let original = "name = \"broken\"\nkind = \"dark\"\n[roles]\nproduction = \"not-a-color\"\n";
        std::fs::write(&path, original).unwrap();
        let loaded = load_theme_file(&path, builtin_dark());
        let err = loaded.error.expect("invalid theme");
        assert_eq!(err.field, "roles.production");
        assert!(err.reason.contains("unknown color"));
        assert_eq!(loaded.theme.kind, ThemeKind::Dark);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn builtins_cover_light_dark_low_color() {
        assert_eq!(builtin_light().kind, ThemeKind::Light);
        assert_eq!(builtin_dark().kind, ThemeKind::Dark);
        assert_eq!(builtin_low_color().kind, ThemeKind::LowColor);
    }

    #[test]
    fn preview_keeps_text_markers_without_color() {
        let caps = TerminalCapabilities {
            color_depth: ColorDepth::None,
            unicode: false,
            mouse: false,
        };
        let text = preview_lines(&builtin_dark(), caps);
        assert!(text.contains("[PROD]"));
        assert!(text.contains("[ERR]"));
        assert!(text.contains(">"));
        assert!(text.contains("color=none"));
        let prod = text.lines().find(|l| l.starts_with("production")).unwrap();
        let err = text.lines().find(|l| l.starts_with("error")).unwrap();
        let sel = text.lines().find(|l| l.starts_with("selection")).unwrap();
        assert_ne!(prod.split_whitespace().nth(1), err.split_whitespace().nth(1));
        assert_ne!(prod.split_whitespace().nth(1), sel.split_whitespace().nth(1));
    }

    #[test]
    fn parse_hex_role() {
        let theme = parse_theme("name=\"x\"\nkind=\"light\"\n[roles]\nerror=\"#cc0000\"\n").unwrap();
        assert_eq!(theme.name, "x");
        assert_eq!(
            theme.color(
                Role::Error,
                TerminalCapabilities {
                    color_depth: ColorDepth::TrueColor,
                    unicode: true,
                    mouse: true
                }
            ),
            Some(ratatui::style::Color::Rgb(204, 0, 0))
        );
    }
}
