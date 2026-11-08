//! # Settings
//!
//! The last step of the wizard: for each source, the collections its
//! sign-in test listed, picked in a multi-select. Mail starts with every
//! folder but junk and trash, the other domains with everything.
//!
//! Keeping everything writes nothing; otherwise the folders left out are
//! the source's `collection.filter.exclude`, so a collection created later
//! still syncs.

use std::fmt;

use anyhow::Result;
use pimalaya_cli::prompt;

use crate::{cli::check::CheckedCollection, config::CollectionFilter, wizard::plan::Domain};

/// The roles a mail source leaves out unless picked.
const UNPICKED_ROLES: [&str; 2] = ["junk", "trash"];

/// Asks which of the `collections` a `domain` source syncs, answering the
/// filter to write.
pub fn pick(domain: Domain, collections: &[CheckedCollection]) -> Result<CollectionFilter> {
    if collections.is_empty() {
        return Ok(CollectionFilter::All);
    }

    let label = match domain {
        Domain::Mail | Domain::Sending => "Folders to sync:",
        Domain::Contacts => "Address books to sync:",
        Domain::Calendar => "Calendars to sync:",
    };

    let items: Vec<Item> = collections
        .iter()
        .enumerate()
        .map(|(index, collection)| Item {
            index,
            label: label_of(collection, collections),
        })
        .collect();

    let picked = prompt::items(label, items, preselected(domain, collections))?;
    let picked: Vec<usize> = picked.into_iter().map(|item| item.index).collect();

    Ok(filter(collections, &picked))
}

/// One collection in the pick list.
#[derive(Eq, PartialEq)]
struct Item {
    index: usize,
    label: String,
}

impl fmt::Display for Item {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.label)
    }
}

/// The collection's name, its id appended when another collection reads
/// the same.
fn label_of(collection: &CheckedCollection, collections: &[CheckedCollection]) -> String {
    let twins = collections
        .iter()
        .filter(|other| other.name == collection.name)
        .count();

    match twins {
        1 => collection.name.clone(),
        _ => format!("{} ({})", collection.name, collection.id),
    }
}

/// The indices picked to begin with: every collection, but the junk and
/// trash folders of mail.
fn preselected(domain: Domain, collections: &[CheckedCollection]) -> Vec<usize> {
    collections
        .iter()
        .enumerate()
        .filter(|(_, collection)| {
            domain != Domain::Mail
                || !collection
                    .role
                    .as_deref()
                    .is_some_and(|role| UNPICKED_ROLES.contains(&role))
        })
        .map(|(index, _)| index)
        .collect()
}

/// The filter keeping the `picked` collections: none when every one is,
/// else the others excluded by id.
fn filter(collections: &[CheckedCollection], picked: &[usize]) -> CollectionFilter {
    let excluded: Vec<String> = collections
        .iter()
        .enumerate()
        .filter(|(index, _)| !picked.contains(index))
        .map(|(_, collection)| collection.id.clone())
        .collect();

    match excluded.is_empty() {
        true => CollectionFilter::All,
        false => CollectionFilter::Exclude(excluded),
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        cli::check::CheckedCollection,
        config::CollectionFilter,
        wizard::{plan::Domain, settings::*},
    };

    fn collection(id: &str, name: &str, role: Option<&str>) -> CheckedCollection {
        CheckedCollection {
            id: id.to_string(),
            name: name.to_string(),
            role: role.map(ToString::to_string),
            default: false,
            takes_tasks: None,
        }
    }

    fn mailboxes() -> Vec<CheckedCollection> {
        vec![
            collection("INBOX", "INBOX", Some("inbox")),
            collection("Sent", "Sent", Some("sent")),
            collection("Junk", "Junk", Some("junk")),
            collection("Trash", "Trash", Some("trash")),
            collection("Archive/2025", "2025", None),
        ]
    }

    #[test]
    fn mail_starts_without_junk_and_trash() {
        assert_eq!(preselected(Domain::Mail, &mailboxes()), [0, 1, 4]);

        // A role means nothing to the other domains, nor does a name.
        let books = vec![
            collection("trash", "Trash", Some("trash")),
            collection("work", "Work", None),
        ];
        assert_eq!(preselected(Domain::Contacts, &books), [0, 1]);
    }

    #[test]
    fn the_collections_left_out_are_excluded_by_id() {
        let mailboxes = mailboxes();

        assert_eq!(
            filter(&mailboxes, &preselected(Domain::Mail, &mailboxes)),
            CollectionFilter::Exclude(vec![String::from("Junk"), String::from("Trash")])
        );
        assert_eq!(
            filter(&mailboxes, &[0, 1, 2, 3]),
            CollectionFilter::Exclude(vec![String::from("Archive/2025")])
        );
    }

    #[test]
    fn everything_picked_writes_nothing() {
        let mailboxes = mailboxes();

        assert_eq!(filter(&mailboxes, &[0, 1, 2, 3, 4]), CollectionFilter::All);
        assert_eq!(filter(&[], &[]), CollectionFilter::All);
    }

    #[test]
    fn a_name_two_collections_share_shows_their_ids() {
        let calendars = vec![
            collection("a1b2", "Work", None),
            collection("c3d4", "Work", None),
            collection("home", "Home", None),
        ];

        assert_eq!(label_of(&calendars[0], &calendars), "Work (a1b2)");
        assert_eq!(label_of(&calendars[2], &calendars), "Home");
    }
}
