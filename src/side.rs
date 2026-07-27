//! Per-side dispatch tag carried by report hunks and the replica index.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Which half of the sync a value belongs to. Pure tag.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Side {
    Left,
    Right,
}

impl Side {
    /// The opposite side.
    pub fn other(self) -> Side {
        match self {
            Side::Left => Side::Right,
            Side::Right => Side::Left,
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
