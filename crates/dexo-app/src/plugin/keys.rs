//! The minisign public keys Dexo trusts to sign the plugin index: the current one and
//! the next, so a planned rotation breaks no installed Dexo. The private keys never
//! enter this repository; `docs/src/plugins.md`, "For maintainers", says how they are
//! made and kept.
pub const TRUSTED: &[&str] = &[
    "RWSHK3wYZTmeZ8XwV4jphLmHiLURRMyVYKtTU5dEZLIfWPaVPEC4IxaK",
    "RWQQPfYbrrbV9qFlqvDsOTQPHx74nTrCDPQC4DBErL8XAHo4d4lJMJha",
];
