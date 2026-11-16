//! # Permissions configuration
//!
//! The `collection`, `flag` and `item` tables of a source: what a run may
//! create, update or delete on it.

use serde::{Deserialize, Serialize};

use crate::config::{filter::CollectionFilter, is_default};

/// Per-source permission triple gating which sync hunks may materialize.
#[derive(Clone, Copy, Debug)]
pub struct SourcePermissions {
    /// What the source lets a run do to its collection set.
    pub collection: CollectionPermissions,
    /// Whether the source takes flag updates.
    pub flag: FlagSourcePermissions,
    /// What the source lets a run do to its items.
    pub item: ItemSourcePermissions,
}

/// A source's collection-level configuration: which collections it syncs,
/// and what it may do to the collection set.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct CollectionSourceConfig {
    /// Whether the sync may create a collection on this source.
    ///
    /// It grants by default, unlike the `item` block, which must be declared
    /// in full: this table also carries `filter`, and demanding a permission
    /// pair from someone writing a filter would be a trap.
    #[serde(default = "default_true", skip_serializing_if = "is_true")]
    pub create: bool,
    /// Whether the sync may delete one on this source, off unless asked for.
    ///
    /// A collection delete is inferred from an absence, the only signal IMAP
    /// and DAV leave, and a listing may drop a collection nobody deleted.
    #[serde(default, skip_serializing_if = "is_default")]
    pub delete: bool,
    /// Whether a frontend may rename a collection on this source, through
    /// the queue.
    ///
    /// Unset, false on a `one-way` account, whose sources are copied rather
    /// than written to, true otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rename: Option<bool>,
    /// Collection-name filter for this source.
    ///
    /// Per source, because an account may hold several kinds and a mailbox
    /// include-list means nothing to a contacts source. Filters are
    /// therefore asymmetric: a collection may sync on one source only.
    #[serde(default, skip_serializing_if = "is_default")]
    pub filter: CollectionFilter,
    /// Whether this source lists at most a number of the items a collection
    /// holds, whatever it holds: a folder size limit set on the server.
    ///
    /// Every round of its collections then says so (pimdir SYNC §4, §5):
    /// an item it leaves unlisted is kept, and only a removal the source
    /// states deletes.
    #[serde(default, skip_serializing_if = "is_default")]
    pub capped: bool,
}

impl CollectionSourceConfig {
    /// The copyable permission pair, the only part the sync seam gates on.
    pub fn permissions(&self) -> CollectionPermissions {
        CollectionPermissions {
            create: self.create,
            delete: self.delete,
            rename: self.rename,
        }
    }
}

impl Default for CollectionSourceConfig {
    fn default() -> Self {
        Self {
            create: true,
            delete: false,
            rename: None,
            filter: CollectionFilter::default(),
            capped: false,
        }
    }
}

/// Per-source collection permissions gating collection-set mutations.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CollectionPermissions {
    /// Whether a run may create a collection here.
    pub create: bool,
    /// Whether a run may delete one.
    pub delete: bool,
    /// Whether a frontend may rename one, `None` taking the account's
    /// answer: allowed unless it is one-way.
    pub rename: Option<bool>,
}

impl CollectionPermissions {
    /// Whether a rename is allowed on an account `one_way` or not.
    pub fn renames(&self, one_way: bool) -> bool {
        self.rename.unwrap_or(!one_way)
    }
}

impl Default for CollectionPermissions {
    fn default() -> Self {
        Self {
            create: true,
            delete: false,
            rename: None,
        }
    }
}

/// Per-source flag permissions, gating flag propagation.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct FlagSourcePermissions {
    /// Whether a run may push a flag change here.
    pub update: bool,
}

impl Default for FlagSourcePermissions {
    fn default() -> Self {
        Self { update: true }
    }
}

/// Per-source item permissions gating item mutations.
///
/// `create` and `delete` are required once the block is declared at all;
/// `update` defaults to true, added later so an older configuration keeps
/// parsing. It bites on mutable content alone, mail bodies being immutable.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct ItemSourcePermissions {
    /// Whether a run may create an item here.
    pub create: bool,
    /// Whether a run may delete one.
    pub delete: bool,
    /// Whether a run may replace an item's body here.
    #[serde(default = "default_true")]
    pub update: bool,
}

impl Default for ItemSourcePermissions {
    fn default() -> Self {
        Self {
            create: true,
            delete: true,
            update: true,
        }
    }
}

/// [`is_default`] for a flag defaulting to true.
fn is_true(value: &bool) -> bool {
    *value
}

/// `serde` default for a permission that grants by default.
fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use crate::config::{AccountConfig, filter::CollectionFilter};

    /// The kind-neutral spellings load; the old mail ones are refused as
    /// unknown ([`a_key_this_version_does_not_read_is_refused`]).
    #[test]
    fn the_generic_pim_sync_spellings_load() {
        let account: AccountConfig = toml::from_str(
            r#"
            imap.server = "imaps://imap.example.org:993"
            imap.collection.create = false
            imap.collection.delete = false
            imap.collection.filter.include = ["INBOX"]
            imap.item.create = true
            imap.item.delete = false
            "#,
        )
        .unwrap();
        let sources = account.sources().unwrap();
        assert_eq!(
            sources["imap"].collection().filter,
            CollectionFilter::Include(vec![String::from("INBOX")])
        );
        let perms = sources["imap"].permissions();
        assert!(!perms.collection.create);
        assert!(!perms.collection.delete);
        assert!(perms.item.create);
        assert!(!perms.item.delete);
        assert!(perms.flag.update);
        assert!(perms.item.update);
    }

    /// A collection is deleted on a server only where the user asked for it.
    ///
    /// Absence is the only signal IMAP and DAV leave, and a listing may drop
    /// a collection nobody deleted (pimdir SYNC §5).
    #[test]
    fn a_collection_delete_is_granted_only_when_asked_for() {
        for document in [
            "imap.server = \"imaps://imap.example.org:993\"",
            "imap.server = \"imaps://imap.example.org:993\"\n\
             imap.collection.filter.include = [\"INBOX\"]",
        ] {
            let account: AccountConfig = toml::from_str(document).unwrap();
            let perms = account.sources().unwrap()["imap"].permissions();
            assert!(perms.collection.create, "{document}");
            assert!(!perms.collection.delete, "{document}");
        }

        let account: AccountConfig = toml::from_str(
            r#"
            imap.server = "imaps://imap.example.org:993"
            imap.collection.delete = true
            "#,
        )
        .unwrap();
        assert!(
            account.sources().unwrap()["imap"]
                .permissions()
                .collection
                .delete
        );
    }

    #[test]
    fn item_update_is_denied_only_when_asked_for() {
        let account: AccountConfig = toml::from_str(
            r#"
            imap.server = "imaps://imap.example.org:993"
            imap.item.create = true
            imap.item.delete = true
            imap.item.update = false
            "#,
        )
        .unwrap();
        let sources = account.sources().unwrap();
        let perms = sources["imap"].permissions();
        assert!(perms.item.create);
        assert!(perms.item.delete);
        assert!(!perms.item.update);

        let account: AccountConfig = toml::from_str(
            r#"
            imap.server = "imaps://imap.example.org:993"
            imap.item.create = true
            imap.item.delete = true
            "#,
        )
        .unwrap();
        let sources = account.sources().unwrap();
        assert!(sources["imap"].permissions().item.update);
    }
}
