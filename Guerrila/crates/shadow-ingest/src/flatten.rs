//! Canonical flattened-account emission.
//!
//! Merges static account keys and ALT-expanded account keys into a
//! single deterministic-order list that fits within `FLAT_ACCOUNTS_CAP`.
//! The output feeds directly into `IngressRecord::set_flat_accounts`.

use shadow_ipc::ingress_record::FLAT_ACCOUNTS_CAP;

/// Errors during account flattening.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlattenError {
    /// Combined static + ALT-expanded account count exceeds
    /// [`FLAT_ACCOUNTS_CAP`] (64). The transaction must be dropped.
    TooManyAccounts {
        static_count: usize,
        alt_count: usize,
    },
}

/// Flatten static keys and ALT-expanded keys into canonical order.
///
/// Order: static keys first (in message order), then ALT-expanded keys.
///
/// Returns the merged list or `FlattenError::TooManyAccounts` if the
/// total exceeds [`FLAT_ACCOUNTS_CAP`].
pub fn flatten_accounts(
    static_keys: &[[u8; 32]],
    alt_keys: &[[u8; 32]],
) -> Result<Vec<[u8; 32]>, FlattenError> {
    let total = static_keys.len() + alt_keys.len();
    if total > FLAT_ACCOUNTS_CAP {
        return Err(FlattenError::TooManyAccounts {
            static_count: static_keys.len(),
            alt_count: alt_keys.len(),
        });
    }
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(static_keys);
    out.extend_from_slice(alt_keys);
    Ok(out)
}
