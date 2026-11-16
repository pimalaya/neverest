//! # Send drain
//!
//! The queue's `submit` intents, sent through the channel each one names and
//! parked, unconfirmed or left pending by how the send ends.

use crate::sync::driver::*;

/// One endpoint a run may send through: its name, and its open session when
/// the run opened one.
///
/// A run performing intents alone opens an endpoint only for an intent that
/// needs its session, so an SMTP channel sends with its source unopened.
pub(super) struct SendSide<'a> {
    name: &'a str,
    #[cfg_attr(not(feature = "msgraph"), allow(dead_code))]
    client: Option<&'a mut Client>,
}

impl<'a> SendSide<'a> {
    /// A side whose session the run opened.
    pub(super) fn open(ctx: &'a mut SourceCtx) -> Self {
        Self {
            name: &ctx.name,
            client: Some(ctx.pool.primary()),
        }
    }

    /// A side the run did not open.
    pub(super) fn unopened(name: &'a str) -> Self {
        Self { name, client: None }
    }
}

/// Performs the queue's `submit` intents, which the store's drain leaves alone.
///
/// A sent intent is acknowledged with a receipt, releasing its body's pin, so a
/// producer reads it applied rather than withdrawn; a permanent failure parks
/// the row with its error, one whose outcome is unknown parks it unconfirmed
/// (pimdir STORAGE §15.2, io-pimdir `unconfirm_action`), never to be sent
/// again, and a transient one leaves it pending. With no channel they stay
/// pending, never parked, another build being able to send.
///
/// Each attempt is counted, and the intent marked in flight
/// ([`submit::InFlight`]), before its first byte leaves: a producer reading
/// the row mid-send sees the attempt, and a run killed mid-send leaves the
/// mark, so the next one parks that intent as unconfirmed instead of sending
/// it again. The mark is cleared once the outcome is in the store.
pub(super) fn drain_submits(
    account_config: &AccountConfig,
    account: &Account,
    sides: &mut [SendSide<'_>],
    store: &mut PimdirSourceStore,
    blobs: &PimdirBlobStage,
    dir: &Path,
    report: &mut SyncOutput,
) {
    let intents = match submit::pending(store) {
        Ok(intents) => intents,
        Err(err) => {
            warn!("cannot read the queued submit intents: {err:#}");
            return;
        }
    };
    let in_flight = submit::InFlight::new(dir);
    let Some(intents) = park_interrupted(store, &in_flight, intents, report) else {
        return;
    };
    // NOTE: an intent naming another source is that source's to send, on its
    // own run (pimdir STORAGE §15.6); one whose payload does not decode names
    // none, and is sent here to be parked as malformed.
    let sender = sending_source(account_config, sides);
    let intents: Vec<_> = intents
        .into_iter()
        .filter(|intent| {
            intent
                .source()
                .is_none_or(|source| Some(&source) == sender.as_ref())
        })
        .collect();
    if intents.is_empty() {
        return;
    }
    info!("performing {} queued submit intent(s)", intents.len());
    // NOTE: a provider filing what it sends (Gmail, Graph) answers a copy
    // by itself; any other gets it as an `add`, queued once sent (pimdir
    // STORAGE Annex B.2).
    let files_copy = sender.as_ref().is_some_and(|sender| {
        account_config
            .sources()
            .ok()
            .and_then(|sources| sources.get(sender).map(capability::files_sent_copy))
            .unwrap_or(false)
    });

    #[cfg(any(feature = "smtp", feature = "msgraph"))]
    {
        let entry =
            |intent: &submit::SubmitIntent, failure: Option<Failure>, parked, unconfirmed| {
                let (code, error, sources) = Failure::row(failure);
                SubmitEntry {
                    id: intent.id,
                    collection: intent.collection.clone(),
                    subject: intent.subject(),
                    code,
                    error,
                    sources,
                    parked,
                    unconfirmed,
                }
            };

        let mut channel = match open_send_channel(account_config, account, sides, intents.len()) {
            Ok(channel) => channel,
            Err(Unopened::Pending) => return,
            Err(Unopened::Unreached(unreached)) => {
                report.unreached.push(unreached);
                return;
            }
            Err(Unopened::Refused(failure)) => {
                // NOTE: nothing was attempted, so no attempt is counted:
                // the producer reads the rows as never tried.
                for intent in &intents {
                    report
                        .submitted
                        .push(entry(intent, Some(failure.clone()), false, false));
                }
                return;
            }
        };

        let mut sent = 0;
        let mut copies = 0;
        // NOTE: why the run stops attempting, once it must: an outcome it
        // could not record keeps its mark, which the next intent's would
        // overwrite.
        let mut halted: Option<String> = None;
        for intent in &intents {
            if let Some(why) = halted.as_deref().or(channel.stopped()) {
                let failure = Failure::new(format!("Not attempted, {why}"));
                report
                    .submitted
                    .push(entry(intent, Some(failure), false, false));
                continue;
            }

            let outgoing = match submit::prepare(blobs.blobs(), intent) {
                Ok(outgoing) => outgoing,
                Err(failure) => {
                    let reason = failure.reason();
                    warn!("submit intent #{} failed: {reason}", intent.id);
                    let parked = failure.parks();
                    if let Err(err) = store.fail_action(intent.id, parked.then_some(&*reason)) {
                        warn!("cannot record submit intent #{} failure: {err}", intent.id);
                    }
                    report
                        .submitted
                        .push(entry(intent, Some(failure.failure()), parked, false));
                    continue;
                }
            };

            // NOTE: counted and marked before the first byte leaves; a row
            // whose attempt cannot be recorded is not sent.
            let marked = store
                .fail_action(intent.id, None)
                .map_err(|err| format!("cannot record the attempt: {err}"))
                .and_then(|()| {
                    in_flight
                        .begin(intent.id)
                        .map_err(|err| format!("cannot mark it in flight: {err}"))
                });
            if let Err(why) = marked {
                warn!("submit intent #{} not sent, {why}", intent.id);
                report.submitted.push(entry(
                    intent,
                    Some(Failure::new(format!("Not attempted, {why}"))),
                    false,
                    false,
                ));
                continue;
            }

            let (recorded, outcome) = match submit::deliver(&mut channel, outgoing) {
                Ok(()) => {
                    let copy = intent.copy().filter(|_| !files_copy);
                    // NOTE: both leave the intent's receipt with no seq: the
                    // copy is not in the store yet, a queued `add` of its
                    // own or one the provider files for the next sync
                    // (pimdir STORAGE §15.5).
                    let acknowledged = match &copy {
                        Some(collection) => {
                            let action = PimdirAction::Add {
                                link_id: None,
                                flags: PimdirFlags::from_iter(["\\Seen"]),
                                object: intent.object.clone(),
                                supersedes: None,
                            };
                            store.replace_action(intent.id, "neverest", collection, &action)
                        }
                        None => store.acknowledge_action(intent.id, None),
                    };
                    copies += usize::from(copy.is_some());
                    sent += 1;
                    let recorded = match acknowledged {
                        Ok(true) => true,
                        Ok(false) => {
                            warn!(
                                "submit intent #{} vanished from the queue before its acknowledgement",
                                intent.id
                            );
                            true
                        }
                        Err(err) => {
                            warn!(
                                "submit intent #{} was sent but could not be acknowledged, the next run parks it as unconfirmed: {err}",
                                intent.id
                            );
                            false
                        }
                    };
                    (recorded, entry(intent, None, false, false))
                }
                Err(failure) => {
                    let reason = failure.reason();
                    let (parked, unconfirmed) = (failure.parks(), failure.unconfirmed());
                    warn!("submit intent #{} failed: {reason}", intent.id);
                    // NOTE: the attempt is counted already, so a transient
                    // failure records nothing more.
                    let recorded = !parked
                        || match park_submit(store, intent.id, &failure) {
                            Ok(()) => true,
                            Err(err) => {
                                warn!("cannot park submit intent #{}: {err}", intent.id);
                                // NOTE: a refused message may be sent again
                                // and refused again; an unconfirmed one must
                                // not be, which the mark sees to.
                                !unconfirmed
                            }
                        };
                    let failure = failure.failure();
                    (recorded, entry(intent, Some(failure), parked, unconfirmed))
                }
            };
            report.submitted.push(outcome);

            if recorded {
                if let Err(err) = in_flight.end() {
                    warn!(
                        "cannot clear the in-flight mark of submit intent #{}: {err}",
                        intent.id
                    );
                    halted = Some(format!("the in-flight mark could not be cleared: {err}"));
                }
            } else {
                halted = Some(String::from(
                    "an earlier message's outcome could not be recorded",
                ));
            }
        }

        channel.close();
        if sent > 0 {
            info!("submitted {sent} of {} queued intent(s)", intents.len());
        }
        if copies > 0 {
            drain_queues(store, report);
        }
    }

    #[cfg(not(any(feature = "smtp", feature = "msgraph")))]
    {
        let _ = (
            account_config,
            account,
            sides,
            blobs,
            store,
            report,
            files_copy,
        );
        warn!(
            "this build has no send channel (needs the `smtp` or the `msgraph` cargo feature), {} submit intent(s) stay pending",
            intents.len()
        );
    }
}

/// Parks the `submit` intent `id` for `failure`: unconfirmed when its
/// outcome is unknown, under the prefix the store reserves and writes
/// itself (pimdir STORAGE §13, §15.2), with its error otherwise.
#[cfg(any(feature = "smtp", feature = "msgraph"))]
fn park_submit(
    store: &mut PimdirSourceStore,
    id: i64,
    failure: &submit::SubmitFailure,
) -> Result<(), PimdirError> {
    if !failure.unconfirmed() {
        return store.fail_action(id, Some(&failure.detail()));
    }

    unconfirm_submit(store, id, &failure.detail())
}

/// Parks the `submit` intent `id` as unconfirmed, `detail` saying why. A
/// row gone since has nothing left to send again.
fn unconfirm_submit(
    store: &mut PimdirSourceStore,
    id: i64,
    detail: &str,
) -> Result<(), PimdirError> {
    if !store.unconfirm_action(id, detail)? {
        warn!("submit intent #{id} vanished from the queue before it was parked");
    }
    Ok(())
}

/// Parks the intent an earlier run left in flight as unconfirmed, and returns
/// the others, still to send.
///
/// That run was stopped between counting the attempt and recording its
/// outcome: the message may have left, so it is never sent again on its
/// own. `None` when the intent could not be parked, the run then sending
/// nothing, so the mark stays for the next.
pub(super) fn park_interrupted(
    store: &mut PimdirSourceStore,
    in_flight: &submit::InFlight,
    mut intents: Vec<submit::SubmitIntent>,
    report: &mut SyncOutput,
) -> Option<Vec<submit::SubmitIntent>> {
    let Some(left) = in_flight.left() else {
        // NOTE: a mark is renamed into place whole, so one that does not
        // read names nothing.
        if let Err(err) = in_flight.end() {
            warn!("cannot clear an unreadable in-flight mark: {err}");
        }
        return Some(intents);
    };

    // NOTE: an intent no longer pending was acknowledged or parked before
    // the run stopped, its outcome recorded.
    if let Some(index) = intents.iter().position(|intent| intent.id == left) {
        let intent = intents.remove(index);
        let detail = submit::unconfirmed_detail("a run stopped while handing it over");
        let error = submit::unconfirmed_reason(&detail);
        warn!("submit intent #{} parked: {error}", intent.id);
        if let Err(err) = unconfirm_submit(store, intent.id, &detail) {
            warn!(
                "cannot park submit intent #{}, nothing is sent this run: {err}",
                intent.id
            );
            return None;
        }
        report.submitted.push(SubmitEntry {
            id: intent.id,
            collection: intent.collection.clone(),
            subject: intent.subject(),
            code: None,
            error: Some(error),
            sources: Vec::new(),
            parked: true,
            unconfirmed: true,
        });
    }

    if let Err(err) = in_flight.end() {
        warn!("cannot clear the in-flight mark, nothing is sent this run: {err}");
        return None;
    }
    Some(intents)
}

/// The source whose channel a run over `sides` sends through, found as
/// [`open_send_channel`] picks it.
pub(super) fn sending_source(
    account_config: &AccountConfig,
    sides: &[SendSide<'_>],
) -> Option<String> {
    let configured = account_config.sources().ok()?;
    sides
        .iter()
        .find(|side| {
            configured
                .get(side.name)
                .is_some_and(|source| source.smtp.is_some() || source.sends_natively())
        })
        .map(|side| side.name.to_owned())
}

/// Why a run has no send channel for its intents.
#[cfg(any(feature = "smtp", feature = "msgraph"))]
enum Unopened {
    /// None this build or this account offers: the intents stay pending,
    /// unreported, another build or run being able to send them.
    Pending,
    /// The SMTP server could not be reached: named under `unreached`, the
    /// intents pending and unattempted.
    #[cfg_attr(not(feature = "smtp"), allow(dead_code))]
    Unreached(UnreachedEndpoint),
    /// The SMTP server refused the credentials: each intent fails with this
    /// error, pending and unattempted.
    #[cfg_attr(not(feature = "smtp"), allow(dead_code))]
    Refused(Failure),
}

/// Resolves the account's send channel: an `smtp` table, else a native send.
///
/// At most one source may declare an `smtp` table, the account refusing more at
/// load, so there is no tiebreak here. A channel the run cannot have warns and
/// leaves the intents pending.
#[cfg(any(feature = "smtp", feature = "msgraph"))]
#[cfg_attr(not(feature = "smtp"), allow(unused_variables))]
fn open_send_channel<'a>(
    account_config: &AccountConfig,
    account: &Account,
    sides: &'a mut [SendSide<'_>],
    queued: usize,
) -> Result<submit::SendChannel<'a>, Unopened> {
    let configured = account_config.sources().map_err(|err| {
        warn!("cannot read the account's sources, {queued} intent(s) stay pending: {err:#}");
        Unopened::Pending
    })?;
    let pick = sides.iter().enumerate().find_map(|(index, side)| {
        let source = configured.get(side.name)?;
        match &source.smtp {
            Some(_) => Some(SendChannelPick::Smtp(side.name.to_owned())),
            None if source.sends_natively() => Some(SendChannelPick::Native(index)),
            None => None,
        }
    });

    match pick {
        #[cfg(feature = "smtp")]
        Some(SendChannelPick::Smtp(name)) => {
            let source = account.get(&name).map_err(|err| {
                warn!("cannot resolve the smtp channel, {queued} intent(s) stay pending: {err:#}");
                Unopened::Pending
            })?;
            let smtp = source.smtp.expect("the pick found a configured channel");
            match submit::connect_smtp(&smtp) {
                Ok(client) => Ok(submit::SendChannel::smtp(client)),
                Err(err) => {
                    warn!("cannot open the smtp channel, {queued} intent(s) stay pending: {err:#}");
                    let failure = Failure::of(&err);
                    match submit::open_failure(&err) {
                        submit::OpenFailure::Unreached => Err(Unopened::Unreached(
                            UnreachedEndpoint::new(format!("{name}.smtp"), failure),
                        )),
                        submit::OpenFailure::Refused => Err(Unopened::Refused(failure)),
                    }
                }
            }
        }
        #[cfg(not(feature = "smtp"))]
        Some(SendChannelPick::Smtp(_)) => {
            warn!(
                "the source's `smtp` table needs the `smtp` cargo feature, {queued} intent(s) stay pending"
            );
            Err(Unopened::Pending)
        }
        #[cfg(feature = "msgraph")]
        Some(SendChannelPick::Native(index)) => match sides[index].client.as_deref_mut() {
            Some(Client::Msgraph(client)) if client.kind() == GraphKind::Mail => {
                Ok(submit::SendChannel::graph(client.as_mut()))
            }
            #[allow(unreachable_patterns)]
            _ => {
                warn!("no send channel available, {queued} intent(s) stay pending");
                Err(Unopened::Pending)
            }
        },
        #[cfg(not(feature = "msgraph"))]
        Some(SendChannelPick::Native(_)) => {
            warn!(
                "the side sends through Microsoft Graph, which needs the `msgraph` cargo feature, {queued} intent(s) stay pending"
            );
            Err(Unopened::Pending)
        }
        None => {
            warn!("no send channel available, {queued} intent(s) stay pending");
            Err(Unopened::Pending)
        }
    }
}

/// Which source performs the submit intents, and how.
///
/// Each variant is consumed by the arm its cargo feature gates, so a build with
/// only one of them carries the other unread rather than duplicating the walk
/// per feature combination.
#[cfg(any(feature = "smtp", feature = "msgraph"))]
#[allow(dead_code)]
enum SendChannelPick {
    /// The name of the source whose `smtp` table the intents leave through.
    Smtp(String),
    /// The index of the source that sends by itself.
    Native(usize),
}
