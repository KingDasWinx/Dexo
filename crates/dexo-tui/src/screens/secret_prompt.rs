use dexo_app::ConnectionProfile;
use secrecy::{ExposeSecret, SecretString};

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

pub struct SecretBuffer(SecretString);

impl SecretBuffer {
    pub fn new(value: impl Into<String>) -> Self {
        Self(SecretString::from(value.into()))
    }

    pub fn expose(&self) -> &str {
        self.0.expose_secret()
    }

    pub fn into_secret(self) -> SecretString {
        self.0
    }

    pub fn push(&mut self, ch: char) {
        let mut text = self.0.expose_secret().to_string();
        text.push(ch);
        self.0 = SecretString::from(text);
    }

    pub fn pop(&mut self) {
        let mut text = self.0.expose_secret().to_string();
        text.pop();
        self.0 = SecretString::from(text);
    }

    pub fn chars(&self) -> usize {
        self.0.expose_secret().chars().count()
    }
}

impl Clone for SecretBuffer {
    fn clone(&self) -> Self {
        Self(SecretString::from(self.0.expose_secret().to_string()))
    }
}

impl std::fmt::Debug for SecretBuffer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SecretBuffer([REDACTED])")
    }
}

impl PartialEq for SecretBuffer {
    fn eq(&self, other: &Self) -> bool {
        self.0.expose_secret() == other.0.expose_secret()
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
        let marker = if self.footer == FooterFocus::Input {
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
                "  [{}] save to the keychain  Alt+K",
                if self.keychain { "x" } else { " " }
            ));
        }
        lines.push(footer_line("Submit", self.footer));
        lines
    }
}
