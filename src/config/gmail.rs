//! # Gmail configuration
//!
//! The `gmail` table of a source, over the Gmail REST API.

use pimalaya_config::secret::Secret;
use serde::{Deserialize, Serialize};

use crate::config::{default_http_alpn, is_default_http_alpn, tls::TlsConfig};

source_config! {
    /// A Gmail REST API source, its labels exposed as mailboxes.
    ///
    /// The API host is fixed, so only the mailbox owner, TLS and the
    /// OAuth 2.0 credential are configurable.
    #[derive(Clone, Debug, Deserialize, Serialize)]
    #[serde(rename_all = "kebab-case", deny_unknown_fields)]
    pub struct GmailConfig {
        /// Gmail user id, `me` by default: the authenticated user.
        #[serde(default = "default_gmail_user_id")]
        pub user_id: String,
        /// How the connection is secured.
        #[serde(default)]
        pub tls: TlsConfig,
        /// ALPN identifiers offered during the TLS handshake, `["http/1.1"]`
        /// by default; `[]` skips ALPN.
        #[serde(
            default = "default_http_alpn",
            skip_serializing_if = "is_default_http_alpn"
        )]
        /// ALPN identifiers offered during the TLS handshake.
        pub alpn: Vec<String>,
        /// How the Gmail session authenticates.
        pub auth: GmailAuthConfig,
        /// The most message reads one HTTP batch carries, 10 by default,
        /// from 1 to 100: a batch with a read Gmail refuses for concurrency
        /// or rate, fails transiently or leaves unanswered halves the next,
        /// down to one read, and each full batch answered whole doubles it
        /// back up to this ceiling.
        ///
        /// Gmail runs the reads of a batch side by side and refuses past
        /// its per-user concurrency limit: measured on 2026-10-10, batches
        /// of 10 sent one after another drew no refusal in 2,250 reads, 25
        /// drew 1.2 % and 50 drew 19 %.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub batch_size: Option<usize>,
        /// How many batches the source's connections may have in flight at
        /// once, 1 by default.
        ///
        /// Two batches of 50 side by side drew 17 to 33 refusals of 50
        /// each: more than one in flight trades latency for refusals, each
        /// waited out, and needs as many connections.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub batches_in_flight: Option<usize>,
        /// The quota units the source spends over any sixty seconds, 6,000
        /// by default, a read counting the 5 units Gmail bills it.
        ///
        /// 6,000 units a minute a user is the quota Google gives a project
        /// today. A project granted more may raise it; a refusal past the
        /// quota is waited out all the same.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub units_per_minute: Option<u32>,
    }
}

/// Gmail authentication: OAuth 2.0 bearer tokens only.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct GmailAuthConfig {
    /// OAuth 2.0 bearer token, the `Bearer ` prefix added by the client.
    ///
    /// Refreshing it is the caller's responsibility.
    pub token: Secret,
}

fn default_gmail_user_id() -> String {
    String::from("me")
}
