//! # Address
//!
//! An email address shared by every mail protocol. Mail-specific: phase 2
//! of the kind seam moves it under the `message/rfc822` kind.

use serde::{Deserialize, Serialize};

/// A single email address with an optional display name.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Address {
    /// Display name (e.g. `Alice`), if any.
    pub name: Option<String>,

    /// Email address (e.g. `alice@example.org`).
    pub email: String,
}
