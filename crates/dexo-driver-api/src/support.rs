#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MatrixStatus {
    Supported,
    Unverified,
}

pub fn postgres_matrix_status(version: &str) -> MatrixStatus {
    match major(version) {
        Some(14..=17) => MatrixStatus::Supported,
        _ => MatrixStatus::Unverified,
    }
}

pub fn mysql_matrix_status(version: &str) -> MatrixStatus {
    match major(version) {
        Some(8..=9) => MatrixStatus::Supported,
        _ => MatrixStatus::Unverified,
    }
}

fn major(version: &str) -> Option<u32> {
    version
        .split(|c: char| !c.is_ascii_digit())
        .find(|part| !part.is_empty())
        .and_then(|part| part.parse().ok())
}

#[cfg(test)]
mod tests {
    use super::{MatrixStatus, mysql_matrix_status, postgres_matrix_status};

    #[test]
    fn matrix_uses_major_not_full_number() {
        assert_eq!(postgres_matrix_status("16.9"), MatrixStatus::Supported);
        assert_eq!(postgres_matrix_status("18.0"), MatrixStatus::Unverified);
        assert_eq!(mysql_matrix_status("8.4.5"), MatrixStatus::Supported);
        assert_eq!(mysql_matrix_status("5.7.44"), MatrixStatus::Unverified);
    }

    #[test]
    fn capabilities_are_independent_of_matrix_number() {
        use crate::capability::{Capability, CapabilityState};
        assert_eq!(postgres_matrix_status("18"), MatrixStatus::Unverified);
        let explain = CapabilityState::available(Capability::Explain);
        assert!(explain.available);
        let missing = CapabilityState::unavailable(Capability::Admin, "requires pg_stat_activity");
        assert!(!missing.available);
        assert_eq!(missing.reason(), Some("requires pg_stat_activity"));
    }
}
