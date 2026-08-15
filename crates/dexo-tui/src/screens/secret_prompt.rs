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
        }
    }

    pub fn close(&mut self) {
        *self = Self::default();
    }

    pub fn lines(&self) -> Vec<String> {
        vec![
            format!("secret required for {}", self.profile_name),
            "s session only  k save to keychain  esc cancel".into(),
        ]
    }
}
