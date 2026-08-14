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

impl Environment {
    pub fn parse(value: &str) -> Self {
        match value.to_ascii_lowercase().as_str() {
            "production" => Self::Production,
            "staging" => Self::Staging,
            "development" => Self::Development,
            _ => Self::Local,
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

    pub fn shows_insecure_indicator(&self) -> bool {
        !self.require_verified_tls
    }
}

#[cfg(test)]
mod tests {
    use super::{ConnectionPolicy, Environment};

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
}
