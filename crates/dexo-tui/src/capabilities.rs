use std::collections::HashMap;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ColorDepth {
    None,
    Ansi16,
    Ansi256,
    TrueColor,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TerminalCapabilities {
    pub color_depth: ColorDepth,
    pub unicode: bool,
    pub mouse: bool,
}

impl TerminalCapabilities {
    pub fn detect() -> Self {
        Self::from_env(std::env::vars().collect())
    }

    pub fn from_env(vars: HashMap<String, String>) -> Self {
        let get = |key: &str| vars.get(key).map(String::as_str);
        let no_color = get("NO_COLOR").is_some_and(|v| !v.is_empty());
        let colorterm = get("COLORTERM").unwrap_or("").to_ascii_lowercase();
        let term = get("TERM").unwrap_or("").to_ascii_lowercase();
        let color_depth = if no_color {
            ColorDepth::None
        } else if colorterm.contains("truecolor") || colorterm.contains("24bit") {
            ColorDepth::TrueColor
        } else if term.contains("256color") || colorterm.contains("256") {
            ColorDepth::Ansi256
        } else if term == "dumb" || term.is_empty() && cfg!(not(windows)) {
            ColorDepth::None
        } else {
            ColorDepth::Ansi16
        };
        let unicode = get("DEXO_ASCII")
            .map(|v| v.is_empty() || v == "0")
            .unwrap_or(true)
            && !term.contains("linux");
        Self {
            color_depth,
            unicode,
            mouse: true,
        }
    }

    pub fn no_color() -> Self {
        Self {
            color_depth: ColorDepth::None,
            unicode: false,
            mouse: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ColorDepth, TerminalCapabilities};
    use std::collections::HashMap;

    fn env(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).into(), (*v).into()))
            .collect()
    }

    #[test]
    fn no_color_wins() {
        let caps =
            TerminalCapabilities::from_env(env(&[("NO_COLOR", "1"), ("COLORTERM", "truecolor")]));
        assert_eq!(caps.color_depth, ColorDepth::None);
    }

    #[test]
    fn truecolor_from_colorterm() {
        let caps = TerminalCapabilities::from_env(env(&[("COLORTERM", "truecolor")]));
        assert_eq!(caps.color_depth, ColorDepth::TrueColor);
    }

    #[test]
    fn ansi256_from_term() {
        let caps = TerminalCapabilities::from_env(env(&[("TERM", "xterm-256color")]));
        assert_eq!(caps.color_depth, ColorDepth::Ansi256);
    }
}
