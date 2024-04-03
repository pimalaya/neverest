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

impl From<BackendSource> for String {
    fn from(source: BackendSource) -> Self {
        match source {
            BackendSource::Left => String::from("Left backend source"),
            BackendSource::Right => String::from("Right backend source"),
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

impl ToString for BackendKind {
    fn to_string(&self) -> String {
        match self {
            #[cfg(feature = "imap")]
            Self::Imap => String::from("IMAP"),
            #[cfg(feature = "maildir")]
            Self::Maildir => String::from("Maildir"),
            #[cfg(feature = "notmuch")]
            Self::Notmuch => String::from("Notmuch"),
        }
    }
}
