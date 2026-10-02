use dexo_app::ConnectionProfile;
use secrecy::SecretString;

use crate::widgets::text_input::TextInput;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SecretPurpose {
    DatabasePassword,
    SshPassword,
    SshPassphrase,
    ProxyPassword,
    TlsPassphrase,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SecretChoiceKind {
    SessionOnly,
    SaveToKeychain,
    Cancel,
}

pub enum SecretChoice {
    SessionOnly(SecretString),
    SaveToKeychain(SecretString),
    Cancel,
}

/// A secret as it is typed: it edits like any input -- Ctrl+A, the cursor, the word
/// keys -- and is wiped when dropped.
pub struct SecretBuffer(TextInput);

impl SecretBuffer {
    pub fn new(value: impl Into<String>) -> Self {
        Self(TextInput::new(value))
    }

    pub fn expose(&self) -> &str {
        self.0.as_str()
    }

    pub fn into_secret(self) -> SecretString {
        SecretString::from(self.expose())
    }

    /// The input, for drawing its cursor and selection over the marks.
    pub fn input(&self) -> &TextInput {
        &self.0
    }

    pub fn handle_key(&mut self, key: crossterm::event::KeyEvent) -> bool {
        self.0.handle_key(key)
    }

    pub fn chars(&self) -> usize {
        self.0.len()
    }
}

impl Drop for SecretBuffer {
    fn drop(&mut self) {
        self.0.wipe();
    }
}

impl Clone for SecretBuffer {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl std::fmt::Debug for SecretBuffer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SecretBuffer([REDACTED])")
    }
}

impl PartialEq for SecretBuffer {
    fn eq(&self, other: &Self) -> bool {
        self.expose() == other.expose()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeleteSecretDecision {
    KeepSecrets,
    DeleteSecrets,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SecretPrompt {
    pub open: bool,
    pub purpose: SecretPurpose,
    pub profile_name: String,
    pub secret_ref: String,
    pub buffer: SecretBuffer,
    pub profile: Option<ConnectionProfile>,
    pub delete: Option<DeleteSecretDecision>,
    /// For a temporary connection: the secret stays in memory, no keychain offer.
    pub temporary: bool,
    /// Keep it in the keychain rather than for this session only (Alt+K).
    pub keychain: bool,
    /// The keychain checkbox has the focus: it is a stop between the secret and the buttons.
    pub keychain_focus: bool,
    /// What the server said about the secret that was just tried.
    pub error: Option<String>,
    pub footer: crate::widgets::form::FooterFocus,
}

impl Default for SecretPrompt {
    fn default() -> Self {
        Self {
            open: false,
            purpose: SecretPurpose::DatabasePassword,
            profile_name: String::new(),
            secret_ref: String::new(),
            buffer: SecretBuffer::new(String::new()),
            profile: None,
            delete: None,
            temporary: false,
            keychain: false,
            keychain_focus: false,
            error: None,
            footer: crate::widgets::form::FooterFocus::Input,
        }
    }
}

impl SecretPrompt {
    pub fn open_for(
        purpose: SecretPurpose,
        profile: ConnectionProfile,
        buffer: SecretBuffer,
    ) -> Self {
        Self {
            open: true,
            purpose,
            profile_name: profile.name.clone(),
            secret_ref: profile.secret_ref.as_str().to_string(),
            buffer,
            profile: Some(profile),
            delete: None,
            temporary: false,
            keychain: false,
            keychain_focus: false,
            error: None,
            footer: crate::widgets::form::FooterFocus::Input,
        }
    }

    pub fn close(&mut self) {
        *self = Self::default();
    }

    pub fn lines(&self) -> Vec<String> {
        use crate::widgets::form::{FooterFocus, footer_line};
        let what = match self.purpose {
            SecretPurpose::DatabasePassword => "Password",
            SecretPurpose::SshPassword => "SSH password",
            SecretPurpose::SshPassphrase => "SSH key passphrase",
            SecretPurpose::ProxyPassword => "Proxy password",
            SecretPurpose::TlsPassphrase => "TLS key passphrase",
        };
        let marker = if self.footer == FooterFocus::Input && !self.keychain_focus {
            ">"
        } else {
            " "
        };
        let mut lines = vec![
            format!("{what} for {}", self.profile_name),
            format!(
                "{marker} {}: {}",
                what.to_lowercase(),
                "*".repeat(self.buffer.chars())
            ),
        ];
        // A temporary connection has no saved profile for a keychain entry to belong to.
        if !self.temporary {
            lines.push(format!(
                "{} [{}] save to the keychain  Alt+K",
                if self.keychain_focus { ">" } else { " " },
                if self.keychain { "x" } else { " " }
            ));
        }
        if let Some(error) = &self.error {
            for line in crate::model::wrap_words(error, 66) {
                lines.push(format!("  {line}"));
            }
        }
        lines.push(footer_line("Submit", self.footer));
        lines
    }
}
