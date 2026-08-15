use serde::{Deserialize, Serialize};

use crate::error::{AppError, ErrorCategory};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Environment {
    Local,
    Development,
    Staging,
    Production,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConnectionPolicy {
    pub read_only: bool,
    pub confirm_destructive: bool,
    pub require_verified_tls: bool,
    pub max_rows: u64,
    pub timeout_secs: u64,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct ConnectionPolicyOverrides {
    pub read_only: Option<bool>,
    pub confirm_destructive: Option<bool>,
    pub require_verified_tls: Option<bool>,
    pub max_rows: Option<u64>,
    pub timeout_secs: Option<u64>,
}

impl Environment {
    pub fn parse(value: &str) -> Self {
        Self::known(value).unwrap_or(Self::Local)
    }

    pub fn known(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "production" => Some(Self::Production),
            "staging" => Some(Self::Staging),
            "development" => Some(Self::Development),
            "local" => Some(Self::Local),
            _ => None,
        }
    }
}

impl ConnectionPolicy {
    pub fn for_environment(environment: Environment) -> Self {
        match environment {
            Environment::Production | Environment::Staging => Self {
                read_only: false,
                confirm_destructive: true,
                require_verified_tls: true,
                max_rows: 10_000,
                timeout_secs: 30,
            },
            Environment::Local | Environment::Development => Self {
                read_only: false,
                confirm_destructive: true,
                require_verified_tls: false,
                max_rows: 100_000,
                timeout_secs: 120,
            },
        }
    }

    pub fn resolve(
        environment: &str,
        overrides: &ConnectionPolicyOverrides,
    ) -> Result<Self, AppError> {
        let mut policy = match Environment::known(environment) {
            Some(env) => Self::for_environment(env),
            None => Self {
                read_only: required_override(overrides.read_only, "read_only")?,
                confirm_destructive: required_override(
                    overrides.confirm_destructive,
                    "confirm_destructive",
                )?,
                require_verified_tls: required_override(
                    overrides.require_verified_tls,
                    "require_verified_tls",
                )?,
                max_rows: required_override(overrides.max_rows, "max_rows")?,
                timeout_secs: required_override(overrides.timeout_secs, "timeout_secs")?,
            },
        };
        if let Some(value) = overrides.read_only {
            policy.read_only = value;
        }
        if let Some(value) = overrides.confirm_destructive {
            policy.confirm_destructive = value;
        }
        if let Some(value) = overrides.require_verified_tls {
            policy.require_verified_tls = value;
        }
        if let Some(value) = overrides.max_rows {
            policy.max_rows = value;
        }
        if let Some(value) = overrides.timeout_secs {
            policy.timeout_secs = value;
        }
        Ok(policy)
    }

    pub fn shows_insecure_indicator(&self) -> bool {
        !self.require_verified_tls
    }
}

fn required_override<T>(value: Option<T>, field: &str) -> Result<T, AppError> {
    value.ok_or_else(|| {
        AppError::new(
            ErrorCategory::Configuration,
            format!("custom environment requires persisted {field}"),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::{ConnectionPolicy, ConnectionPolicyOverrides, Environment};
    use crate::error::ErrorCategory;

    #[test]
    fn production_defaults_to_strict_controls() {
        let policy = ConnectionPolicy::for_environment(Environment::Production);
        assert!(policy.confirm_destructive);
        assert!(policy.require_verified_tls);
        assert_eq!(policy.max_rows, 10_000);
    }

    #[test]
    fn local_defaults_keep_destructive_confirmation() {
        let policy = ConnectionPolicy::for_environment(Environment::Local);
        assert!(policy.confirm_destructive);
        assert_eq!(policy.max_rows, 100_000);
        assert_eq!(policy.timeout_secs, 120);
        assert!(policy.shows_insecure_indicator());
    }

    #[test]
    fn custom_environment_does_not_use_parse_fallback() {
        let error = ConnectionPolicy::resolve("pci-lab", &ConnectionPolicyOverrides::default())
            .unwrap_err();
        assert_eq!(error.category(), ErrorCategory::Configuration);
        assert!(error.to_string().contains("persisted"));
    }

    #[test]
    fn custom_environment_uses_persisted_policy() {
        let policy = ConnectionPolicy::resolve(
            "pci-lab",
            &ConnectionPolicyOverrides {
                read_only: Some(true),
                confirm_destructive: Some(true),
                require_verified_tls: Some(true),
                max_rows: Some(50),
                timeout_secs: Some(5),
            },
        )
        .unwrap();
        assert!(policy.read_only);
        assert_eq!(policy.max_rows, 50);
        assert_eq!(policy.timeout_secs, 5);
    }
}
