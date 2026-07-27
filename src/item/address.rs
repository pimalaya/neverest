//! Email address shared across all mail protocols.
//!
//! Mail-specific; the kind seam (change `generic-pim-sync`, phase 2)
//! moves it under the `message/rfc822` kind.

use serde::{Deserialize, Serialize};

/// A single email address with an optional display name.
///
/// Common shape used by every protocol-specific envelope and message
/// representation.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Address {
    /// Display name (e.g. `Alice`), if any.
    pub name: Option<String>,

    /// Email address (e.g. `alice@example.org`).
    pub email: String,
}
