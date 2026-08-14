#[derive(Clone, Debug, PartialEq)]
pub struct SettingsScreen {
    pub open: bool,
    pub theme: String,
    pub keymap: String,
    pub mouse: bool,
    pub animation: bool,
    pub unicode: bool,
    pub confirm_reset: bool,
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

    pub fn diff(&self, previous: &Self) -> String {
        let mut lines = Vec::new();
        if self.theme != previous.theme {
            lines.push(format!("theme {} -> {}", previous.theme, self.theme));
        }
        if self.keymap != previous.keymap {
            lines.push(format!("keymap {} -> {}", previous.keymap, self.keymap));
        }
        if self.mouse != previous.mouse {
            lines.push(format!("mouse {} -> {}", previous.mouse, self.mouse));
        }
        if self.animation != previous.animation {
            lines.push(format!("animation {} -> {}", previous.animation, self.animation));
        }
        if self.unicode != previous.unicode {
            lines.push(format!("unicode {} -> {}", previous.unicode, self.unicode));
        }
        if lines.is_empty() {
            "no changes".into()
        } else {
            lines.join("\n")
        }
    }

    pub fn reset(&mut self) {
        *self = Self {
            open: true,
            confirm_reset: false,
            ..Self::default()
        };
    }

    pub fn lines(&self) -> Vec<String> {
        vec![
            format!("theme={}", self.theme),
            format!("keymap={}", self.keymap),
            format!("mouse={}", self.mouse),
            format!("animation={}", self.animation),
            format!("unicode={}", self.unicode),
            format!("confirm_reset={}", self.confirm_reset),
            format!("diff {}", self.diff(&Self::default())),
        ]
    }
}
