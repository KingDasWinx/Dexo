#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KnownHost {
    pub host: String,
    pub port: u16,
    pub fingerprint: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HostKeyDecision {
    Trusted,
    New { fingerprint: String },
    Changed,
}

pub fn verify_host_key(known: Option<&KnownHost>, presented: &str) -> HostKeyDecision {
    match known {
        Some(known) if known.fingerprint == presented => HostKeyDecision::Trusted,
        Some(_) => HostKeyDecision::Changed,
        None => HostKeyDecision::New {
            fingerprint: presented.to_string(),
        },
    }
}

pub fn ssh_fingerprint(key: &russh::keys::PublicKey) -> String {
    key.fingerprint(russh::keys::HashAlg::Sha256).to_string()
}
