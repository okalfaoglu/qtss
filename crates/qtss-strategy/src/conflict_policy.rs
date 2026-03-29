//! Shared env: FAQ §10 — TA vs on-chain conflict → skip veya yarım boy.

#[derive(Clone, Copy, Eq, PartialEq)]
pub enum ConflictSizePolicy {
    Skip,
    Half,
}

/// `QTSS_SIGNAL_FILTER_ON_CONFLICT=half` | `half_size` → yarım miktar; aksi halde atla.
#[must_use]
pub fn conflict_size_policy_from_env() -> ConflictSizePolicy {
    match std::env::var("QTSS_SIGNAL_FILTER_ON_CONFLICT")
        .ok()
        .as_deref()
        .map(str::trim)
    {
        Some("half") | Some("half_size") => ConflictSizePolicy::Half,
        _ => ConflictSizePolicy::Skip,
    }
}
