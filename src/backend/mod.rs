use std::fmt;

pub mod config;
#[cfg(feature = "wizard")]
pub mod wizard;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BackendSource {
    Left,
    Right,
}

impl BackendSource {
    pub fn is_left(&self) -> bool {
        matches!(self, Self::Left)
    }

    pub fn is_right(&self) -> bool {
        matches!(self, Self::Right)
    }
}

impl fmt::Display for BackendSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Left => write!(f, "Left backend source"),
            Self::Right => write!(f, "Right backend source"),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BackendKind {
    #[cfg(feature = "imap")]
    Imap,
    #[cfg(feature = "maildir")]
    Maildir,
    #[cfg(feature = "notmuch")]
    Notmuch,
}

impl fmt::Display for BackendKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            #[cfg(feature = "imap")]
            Self::Imap => write!(f, "IMAP"),
            #[cfg(feature = "maildir")]
            Self::Maildir => write!(f, "Maildir"),
            #[cfg(feature = "notmuch")]
            Self::Notmuch => write!(f, "Notmuch"),
        }
    }
}
