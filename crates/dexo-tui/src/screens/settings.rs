/// One settings row: every choice stays on screen, so nothing has to be guessed.
pub struct FieldOptions {
    pub label: &'static str,
    pub values: Vec<String>,
    pub active: usize,
    /// Paint each value in the accent it names; the index maps into `theme::ACCENTS`.
    pub tint: bool,
}

/// Widest option row (`Accent`) plus its label column. Below this the rows collapse
/// to the active value alone.
pub const WIDE_MIN_WIDTH: u16 = 58;

/// Rows the user can move through: the eight settings, then the reset action.
pub const FIELD_COUNT: usize = 8;
pub const RESET_FOCUS: usize = FIELD_COUNT;

#[derive(Clone, Debug, PartialEq)]
pub struct SettingsScreen {
    pub open: bool,
    /// `dexo`, a preset or `file:<name>`; see [`crate::theme::resolve`].
    pub theme: String,
    /// What the Theme row steps through, as `(key, label)`.
    pub themes: Vec<(String, String)>,
    /// Light/dark surface and the system's primary color are separate settings.
    pub mode: String,
    pub accent: String,
    pub keymap: String,
    pub mouse: bool,
    pub animation: bool,
    pub unicode: bool,
    pub updates: bool,
    pub confirm_reset: bool,
    pub focus: usize,
    pub completion_trigger: dexo_sql::TriggerMode,
}

impl Default for SettingsScreen {
    fn default() -> Self {
        Self {
            open: false,
            theme: crate::theme::DEXO_THEME.into(),
            themes: crate::theme::choices(&[]),
            mode: crate::theme::Mode::Dark.as_key().into(),
            accent: crate::theme::DEFAULT_ACCENT.into(),
            keymap: "default".into(),
            completion_trigger: dexo_sql::TriggerMode::default(),
            mouse: true,
            animation: true,
            unicode: true,
            updates: true,
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
            themes: std::mem::take(&mut self.themes),
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

    pub fn options(&self) -> Vec<FieldOptions> {
        // Themes are too many to show side by side: the row shows the one in use and
        // where it is in the list; left and right step through them, applied at once.
        let theme = self
            .themes
            .iter()
            .position(|(key, _)| *key == self.theme)
            .unwrap_or(0);
        let theme_label = self
            .themes
            .get(theme)
            .map_or("Dexo", |(_, label)| label.as_str());
        vec![
            FieldOptions {
                label: "Theme",
                // The place first: a long file name is what gets cut, not where it is.
                values: vec![format!(
                    "{}/{}  {theme_label}",
                    theme + 1,
                    self.themes.len().max(1)
                )],
                active: 0,
                tint: false,
            },
            FieldOptions {
                label: "Mode",
                values: crate::theme::MODES
                    .iter()
                    .map(|m| m.label().to_string())
                    .collect(),
                active: crate::theme::Mode::from_key(&self.mode).index(),
                tint: false,
            },
            FieldOptions {
                label: "Accent",
                values: crate::theme::ACCENTS
                    .iter()
                    .map(|(.., l)| l.to_string())
                    .collect(),
                active: crate::theme::accent_index(&self.accent),
                tint: true,
            },
            FieldOptions {
                label: "Keymap",
                values: crate::keymap::PROFILES
                    .iter()
                    .map(|(_, l)| l.to_string())
                    .collect(),
                active: crate::keymap::profile_index(&self.keymap),
                tint: false,
            },
            on_off_field("Mouse", self.mouse),
            on_off_field("Animation", self.animation),
            on_off_field("Unicode", self.unicode),
            on_off_field("Updates", self.updates),
        ]
    }

    /// Text form of [`Self::options`], for hit testing and the narrow layout.
    pub fn field_rows(&self) -> Vec<String> {
        self.options()
            .into_iter()
            .enumerate()
            .map(|(index, field)| {
                let marker = if index == self.focus { ">" } else { " " };
                let label = field.label;
                let value = &field.values[field.active];
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

    /// Spelled out while there is room; the short form still names every key.
    pub fn hint(wide: bool) -> &'static str {
        if wide {
            "  up/down row  left/right change  e theme  r reset  esc close"
        } else {
            "  arrows change  e theme  r reset  esc close"
        }
    }

    pub fn lines(&self, wide: bool) -> Vec<String> {
        let mut lines = self.field_rows();
        lines.push(String::new());
        lines.push(self.footer_line());
        lines.push(Self::hint(wide).into());
        lines
    }
}

fn on_off_field(label: &'static str, value: bool) -> FieldOptions {
    FieldOptions {
        label,
        values: vec!["On".into(), "Off".into()],
        active: usize::from(!value),
        tint: false,
    }
}
