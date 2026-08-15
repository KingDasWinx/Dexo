use dexo_app::ConnectionProfile;
use dexo_secrets::{SecretError, SecretStore};

use crate::action::Action;
use crate::runtime::SessionSecrets;
use crate::screens::secret_prompt::SecretPurpose;

pub fn connect_with_store(
    store: &dyn SecretStore,
    profile: &ConnectionProfile,
) -> Result<secrecy::SecretString, Action> {
    match store.get(profile.secret_ref.as_str()) {
        Ok(Some(secret)) => Ok(secret),
        Ok(None) | Err(SecretError::Unavailable) => Err(Action::SecretRequired {
            purpose: SecretPurpose::DatabasePassword,
            profile: profile.clone(),
            buffer: crate::screens::secret_prompt::SecretBuffer::new(String::new()),
        }),
        Err(error) => Err(Action::ConnectionFormError {
            message: error.to_string(),
        }),
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
    ) -> Result<secrecy::SecretString, Action> {
        connect_with_store(self.secrets, profile)
    }
}
