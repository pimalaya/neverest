//! Maildir client wrapper. Construction is cheap (no I/O until the
//! first op) but kept symmetric with the other side wrappers so the
//! dispatch code in [`crate::side`] stays uniform.

use io_maildir::client::MaildirClient as Inner;

use crate::{account::context::Account, config::MaildirConfig};

pub struct MaildirClient {
    inner: Inner,
    #[allow(dead_code)]
    pub account: Account,
}

impl MaildirClient {
    pub fn new(config: MaildirConfig, account: Account) -> Self {
        Self {
            inner: Inner::new(config.root),
            account,
        }
    }

    pub fn into_inner(self) -> Inner {
        self.inner
    }
}
