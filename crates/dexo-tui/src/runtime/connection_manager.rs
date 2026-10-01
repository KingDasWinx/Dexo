use dexo_app::ConnectionProfile;
use dexo_secrets::{SecretError, SecretStore};

use crate::action::Action;
use crate::runtime::SessionSecrets;
use crate::screens::secret_prompt::SecretPurpose;

/// The password to connect with, or the prompt that asks for one. A file connection
/// has no password, so it never prompts.
pub fn connect_with_store(
    store: &dyn SecretStore,
    profile: &ConnectionProfile,
) -> Result<secrecy::SecretString, Box<Action>> {
    match profile.password(store) {
        Ok(Some(secret)) => Ok(secret),
        Ok(None) | Err(SecretError::Unavailable) => Err(Box::new(Action::SecretRequired {
            purpose: SecretPurpose::DatabasePassword,
            profile: profile.clone(),
            buffer: crate::screens::secret_prompt::SecretBuffer::new(String::new()),
        })),
        Err(error) => Err(Box::new(Action::ConnectionFormError {
            message: error.to_string(),
        })),
    }
}

pub(crate) struct ConnectionManager<'a> {
    secrets: &'a SessionSecrets,
}

impl<'a> ConnectionManager<'a> {
    pub(crate) fn new(secrets: &'a SessionSecrets) -> Self {
        Self { secrets }
    }

    pub(crate) fn connect(
        &self,
        profile: &ConnectionProfile,
    ) -> Result<secrecy::SecretString, Box<Action>> {
        connect_with_store(self.secrets, profile)
    }
}
