//! Per-side dispatch tag carried by hunks and cache entries.

use std::fmt;

use anyhow::{Result, bail};
use io_email::client::EmailClientStd;
use serde::{Deserialize, Serialize};

/// Which half of the sync a value belongs to. Pure tag; carried by hunks
/// and cache entries.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Side {
    Left,
    Right,
}

impl Side {
    /// Selects the matching client from a `(left, right)` mutable pair.
    pub fn client_mut<'a>(
        self,
        left: &'a mut EmailClientStd,
        right: &'a mut EmailClientStd,
    ) -> &'a mut EmailClientStd {
        match self {
            Side::Left => left,
            Side::Right => right,
        }
    }

    /// Returns the `(source, target)` client pair for a `Copy` hunk;
    /// errors when `source == target`.
    pub fn pair_mut<'a>(
        source: Side,
        target: Side,
        left: &'a mut EmailClientStd,
        right: &'a mut EmailClientStd,
    ) -> Result<(&'a mut EmailClientStd, &'a mut EmailClientStd)> {
        match (source, target) {
            (Side::Left, Side::Right) => Ok((left, right)),
            (Side::Right, Side::Left) => Ok((right, left)),
            _ => bail!("Copy hunk has identical source and target side"),
        }
    }
}

impl fmt::Display for Side {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Left => write!(f, "left"),
            Self::Right => write!(f, "right"),
        }
    }
}
