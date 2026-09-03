/// Rows the user can move through: the five settings, then the reset action.
pub const FIELD_COUNT: usize = 5;
pub const RESET_FOCUS: usize = FIELD_COUNT;

#[derive(Clone, Debug, PartialEq)]
pub struct SettingsScreen {
    pub open: bool,
    pub theme: String,
    pub keymap: String,
    pub mouse: bool,
    pub animation: bool,
    pub unicode: bool,
    pub confirm_reset: bool,
    pub focus: usize,
}

impl Default for SettingsScreen {
    fn default() -> Self {
        Self {
            open: false,
            theme: "dark".into(),
            keymap: "default".into(),
            mouse: true,
            animation: true,
            unicode: true,
            confirm_reset: false,
            focus: 0,
        }
    }
}

impl SettingsScreen {
    pub fn fixture() -> Self {
        Self {
            open: true,
            ..Self::default()
        }
    }

    pub fn reset(&mut self) {
        *self = Self {
            open: true,
            confirm_reset: false,
            focus: self.focus,
            ..Self::default()
        };
    }

    pub fn focus_next(&mut self) {
        self.confirm_reset = false;
        self.focus = (self.focus + 1) % (FIELD_COUNT + 1);
    }

    pub fn focus_prev(&mut self) {
        self.confirm_reset = false;
        self.focus = (self.focus + FIELD_COUNT) % (FIELD_COUNT + 1);
    }

    pub fn field_rows(&self) -> Vec<String> {
        [
            ("Theme", theme_label(&self.theme)),
            ("Keymap", keymap_label(&self.keymap)),
            ("Mouse", on_off(self.mouse)),
            ("Animation", on_off(self.animation)),
            ("Unicode", on_off(self.unicode)),
        ]
        .into_iter()
        .enumerate()
        .map(|(index, (label, value))| {
            let marker = if index == self.focus { ">" } else { " " };
            format!("{marker} {label:<11}{value}")
        })
        .collect()
    }

    pub fn footer_line(&self) -> String {
        let marker = if self.focus == RESET_FOCUS { ">" } else { " " };
        let label = if self.confirm_reset {
            "Confirm reset"
        } else {
            "Reset to defaults"
        };
        format!("{marker} [{label}]")
    }

    pub fn lines(&self) -> Vec<String> {
        let mut lines = self.field_rows();
        lines.push(String::new());
        lines.push(self.footer_line());
        lines.push("  up/down move  enter change  esc close".into());
        lines
    }
}

fn on_off(value: bool) -> &'static str {
    if value { "On" } else { "Off" }
}

fn theme_label(id: &str) -> &'static str {
    match id {
        "light" => "Light",
        "low-color" => "Low color",
        "high-contrast" => "High contrast",
        _ => "Dark",
    }
}

fn keymap_label(name: &str) -> &'static str {
    match name {
        "vim" => "Vim",
        "emacs" => "Emacs",
        _ => "Default",
    }
}
