---
cairn: delta
change: one-account-for-every-domain
---

# Delta

## MODIFIED Requirements

### Requirement: The wizard configures one channel from the account's credentials
The send channel SHALL be the sending domain of the review: through the mail source when it sends by itself (Microsoft Graph mail, through `sendMail`), with no other choice offered since an account-level `smtp` table cannot complete such a source; otherwise over the best discovered SMTP server, implicit TLS before STARTTLS, signing in as mail does when that server offers it, written as the account-level `smtp` table completing the mail source; and nowhere when no SMTP server was discovered, the account being saved without a channel rather than with a guessed `smtp.<domain>`. A mail source that does not send by itself, the Gmail API included, SHALL take the SMTP channel.

The channel SHALL share mail's secret by the rules of "Sign-ins are grouped by identity": one OAuth sign-in when both sign in through one authorization server, one password or API token when their hosts are servers of one name, a question before reusing it for another host. A password SHALL sign SMTP in with PLAIN and a token with XOAUTH2, never through a mechanism menu; the mechanism stays editable in the file. A typed secret SHALL be stored under the keyring entry of the first source it signs in, so a channel sharing mail's password reads mail's entry and one with its own reads `neverest-<account>-smtp`.

Sending needs a mail source: with mail not set up or left out, sending SHALL be not set up and the account written without a channel.

#### Scenario: Mail and sending of one domain
- GIVEN mail IMAP at imap.example.org and sending SMTP at smtp.example.org, both with a password
- WHEN they sign in
- THEN one password prompt lists both servers, and `imap.sasl` and `smtp.sasl` read the one keyring entry `neverest-<account>-imap`

#### Scenario: Mail over the Gmail API
- GIVEN an address whose provider rule offers the Gmail API and SMTP, both through one authorization server
- WHEN the plan is made
- THEN mail is the Gmail API and sending SMTP through the same sign-in, never "through mail"

### Requirement: The wizard discovers in parallel and proposes what it found
The discovery fan-out SHALL ask for every service neverest has a backend for: IMAP and submission, CardDAV and CalDAV, and the provider APIs a provider rule carries (`gmail`, `gpeople`, `gcal`, Microsoft Graph mail, contacts and calendars), within its deadline. The OAuth metadata resolutions (RFC 8414, RFC 9728) SHALL stay off, their fetches running past the deadline. The wizard SHALL offer only what the running build compiles, and no JMAP option, JMAP having no backend yet.

The wizard SHALL write one account holding one source per domain the review set up (mail, contacts, calendar), each through the direct-backend sugar and so named after its protocol, plus the mail source's `smtp` channel when sending goes over SMTP. Each domain's options map to keys of their own (`imap`, `gmail`, `msgraph` for mail; `carddav`, `gpeople`, `msgraph-contacts` for contacts; `caldav`, `gcal`, `msgraph-calendar` for calendar), so two sources the wizard writes never need one name. An account with no target retains every body and reads offline with no further setting: the wizard SHALL NOT write `one-way`, `retain` or a `targets` table, their defaults being the offline replica it exists to produce. A mirror, a fan-in and a second source of one kind are configured by hand against config.sample.toml.

#### Scenario: A provider carrying every domain
- GIVEN an address a provider rule matches, offering its mail, contacts and calendar APIs through one authorization server
- WHEN the wizard saves the account
- THEN the account holds the `gmail`, `gpeople` and `gcal` sources and an `smtp` channel under one `[accounts.<name>]` header, and no `default`

### Requirement: The generated configuration is a dotted document
A configuration neverest writes or prints SHALL render as Himalaya's does: one `[accounts.<name>]` table header per account, the only header in the document, with every field below it written as a dotted key. An empty table SHALL write nothing. The saved file and the document printed on stdout SHALL be identical.

A rendered account SHALL be readable in that order rather than the serializer's: the groups SHALL be ordered with the backends the wizard wrote first, mail then its `smtp` channel, contacts, then calendar, before the sync options it never writes; each group SHALL be separated by a blank line, and the key naming what a group points at (`server`, `user-id`) SHALL be lifted to the top of its own, ahead of the credential authenticating against it.

An account naming several sources SHALL render under that same single header, its sources being dotted keys like every other field, so appending a table after it never opens a header a later account would fall into.

The document SHALL hold only what was actually decided: every field equal to its default SHALL be omitted (`default`, which the wizard never sets, the per-source collection / flag / item permissions, `collection.create` and `collection.delete` beside a written filter included, the per-source pool size, the collection filter, the HTTP-backend ALPN list, `starttls`). Omitting a field SHALL be lossless: every skipped field keeps a deserialization default equal to the value that was skipped.

#### Scenario: A source with an excluded folder
- GIVEN a generated IMAP source excluding `Junk`
- WHEN the account renders
- THEN it reads `imap.collection.filter.exclude = ["Junk"]` and no `imap.collection.create` nor `imap.collection.delete`

## ADDED Requirements

### Requirement: Each domain gets its best option without a question
For each domain of the account, mail, sending, contacts and calendar, the wizard SHALL list the options discovery found that the build compiles, each a protocol, a server and a sign-in method (a password, an API token or OAuth 2.0). A discovered IMAP or SMTP server offering neither implicit TLS nor STARTTLS SHALL be dropped, and so SHALL a discovered HTTP server that is not `https`. Two options reading the same, protocol, server and sign-in method, SHALL be one, so a server offering OAuth through several grants is one OAuth option. A provider API SHALL take OAuth alone.

Options SHALL be ranked the provider's own API first where a provider rule matched (`gmail`, `gpeople`, `gcal`, Microsoft Graph), then IMAP, SMTP, CardDAV and CalDAV; the servers of one protocol implicit TLS before STARTTLS; and within one server OAuth, an API token, then a password when `ortie` is on the `PATH`, and a password, an API token, then OAuth otherwise. The first ranked option of each domain, Auto, SHALL be picked without a question. Sending follows "The wizard configures one channel from the account's credentials".

#### Scenario: A provider rule matched
- GIVEN an address a provider rule matches, offering its mail, contacts and calendar APIs beside IMAP, SMTP, CardDAV and CalDAV
- WHEN the plan is made
- THEN mail, contacts and calendar are the provider's APIs, with or without `ortie` on the `PATH`

#### Scenario: OAuth ranks first only with Ortie
- GIVEN a CardDAV server offering OAuth and a password
- WHEN the plan is made with `ortie` on the `PATH`, then without
- THEN contacts sign in through OAuth, then with a password

### Requirement: The review lists each pick in plain words
Before any sign-in, the wizard SHALL show one row per domain the build can set up, mail, sending, contacts and calendar, through pimalaya-cli's `review::prompt`, worded without jargon beyond the protocol name: `<Protocol> at <host>, with your password`, `, with an API token`, `, through your browser` (OAuth through Ortie) or `, with a token from a broker` (OAuth without Ortie); a provider API by its name, as `Gmail API, through your browser`; a domain sharing mail's secret `, with mail's password`; sending over mail's session `Through mail`; sending without a channel and a domain left out `Not set up`; a domain discovery missed `Nothing found`.

Continue SHALL be the default and SHALL need at least one domain set up. Changing a domain SHALL list `Auto (<what Auto is>)` when discovery found an option for it, every option found as `<Protocol> · <Sign-in method>`, the server URL appended only where two entries would otherwise read the same, `Custom server URL`, then `No sending` for sending and `Leave out` for every other domain; a pick SHALL return to the review. Sending through a mail source that sends by itself offers no change, and sending with mail not set up asks for mail first.

#### Scenario: One provider over the open protocols
- GIVEN mail IMAP at imap.example.org, sending SMTP at smtp.example.org, contacts CardDAV at dav.example.org, all with a password, and no calendar found
- WHEN the review shows
- THEN it reads `Mail      IMAP at imap.example.org, with your password`, `Sending   SMTP at smtp.example.org, with mail's password`, `Contacts  CardDAV at dav.example.org, with mail's password` and `Calendar  Nothing found`

### Requirement: Nothing discovered asks for a server URL
When discovery yields no option for any domain (an empty result, the deadline elapsing with nothing completed, or a resolver that does not answer), the wizard SHALL say that nothing could be discovered for the domain and ask `Server URL:`, whose scheme decides the domain: `imaps://` and `imap://` mail, `carddavs://` and `carddav://` contacts, `caldavs://` and `caldav://` calendar, `https://` both contacts and calendar, a DAV entry point serving both, each signed in and tested on its own. A sending URL SHALL be refused there, sending needing mail. An empty answer SHALL stop the wizard with a message stating it could not automatically discover a configuration for the input and inviting the user to write the account by hand from the documented sample configuration (linked), nothing being written.

When discovery found something, a domain it missed SHALL read `Nothing found` in the review, its change offering `Custom server URL` and `Leave out`. `Custom server URL` SHALL ask `Server URL:` for that domain (`Sending server URL:` for sending), an empty answer returning to the review.

#### Scenario: An unknown domain
- GIVEN an address whose domain discovery knows nothing of
- WHEN the lookup ends
- THEN the wizard says nothing could be discovered for the domain and asks `Server URL:`
- AND an empty answer stops it naming the documented sample, with no file written

#### Scenario: A DAV entry point
- GIVEN the answer `https://dav.example.org` to that prompt
- WHEN it is read
- THEN contacts is CardDAV and calendar CalDAV at `https://dav.example.org/`, and the review shows both

### Requirement: A typed server URL names its protocol by its scheme
A server URL typed for a domain SHALL name its protocol by its scheme: for mail, `imaps://host[:port]` is IMAP over implicit TLS (993 by default) and `imap://` IMAP over STARTTLS (143); for sending, `smtps://` is SMTP over implicit TLS (465) and `smtp://` SMTP over STARTTLS (587); for contacts, `https://`, `carddavs://` and `carddav://` are CardDAV over https; for calendar, `https://`, `caldavs://` and `caldav://` are CalDAV over https. Encrypted servers only: an `http://` URL SHALL be refused naming it, and asked again, unless its host is this machine (`localhost` or a loopback address), where it is CardDAV or CalDAV over http. A URL without a scheme, a scheme the domain does not take and a URL carrying a password SHALL be refused and asked again too. A user name in the URL is the login. The sign-in method SHALL then be picked from Password (the default) and API token.

#### Scenario: A cleartext server
- GIVEN the answer `http://dav.example.org` for contacts
- WHEN it is read
- THEN the wizard refuses it naming `http://dav.example.org`, saying only encrypted servers are accepted, and asks again

#### Scenario: A cleartext server on this machine
- GIVEN the answer `http://localhost:5232/` for calendar
- WHEN it is read
- THEN calendar is CalDAV at `http://localhost:5232/`

### Requirement: The sign-in mechanism is picked, not asked
The wizard SHALL NOT prompt for a SASL mechanism, an HTTP authentication scheme, the user id of the Gmail API or Microsoft Graph (`me`), or a login it knows. A password SHALL sign IMAP in with the best mechanism the unauthenticated CAPABILITY probe advertises, SCRAM-SHA-256, then PLAIN, then the LOGIN command, and PLAIN when the probe fails or advertises none of them; a token with OAUTHBEARER when advertised, else XOAUTH2. SMTP SHALL take PLAIN for a password and XOAUTH2 for a token, and CardDAV and CalDAV Basic for a password and Bearer for a token. The login SHALL be the user name of a typed URL, else the advertised user name when it is an address, else the typed address, and SHALL be asked only when none of them is known. The mechanism stays editable in the configuration.

#### Scenario: A server advertising PLAIN and LOGIN
- GIVEN an IMAP server whose unauthenticated CAPABILITY advertises `AUTH=PLAIN` and no `LOGINDISABLED`
- WHEN it is signed in with a password
- THEN the account's IMAP SASL is PLAIN, and no mechanism was asked

### Requirement: Sign-ins are grouped by identity
After the review, the domains set up SHALL be signed in once per identity, in domain order: one OAuth sign-in per authorization server, covering every domain that picked it, and one password or API token per group of servers of one name, servers whose hosts are equal, or equal once their first label is dropped (`imap.example.org`, `smtp.example.org` and `dav.example.org`), an IP address or a two-label host never losing one. A later group signing in with the same kind of secret on another host SHALL be asked `Use the same password for <host>?` (`API token` for a token), no by default, before its own prompt. A domain already signed in during the run SHALL NOT be asked again after a return to the review unless its pick changed.

#### Scenario: Every domain of one name
- GIVEN mail at imap.example.org, sending at smtp.example.org, contacts and calendar at dav.example.org, all with a password
- WHEN they sign in
- THEN one password prompt lists the four servers

#### Scenario: Contacts elsewhere
- GIVEN mail at imap.example.org and contacts at dav.example.net, both with a password
- WHEN mail signed in
- THEN the wizard asks `Use the same password for dav.example.net?`, no by default, before a second prompt

### Requirement: A password is typed once, tested, then stored
A password or an API token SHALL be asked once, masked and without confirmation, through pimalaya-cli's `keyring::prompt_password`, which names every server it goes to and flags one off the address's domain. Each group SHALL be tested as soon as it is typed, each source by `check`'s per-endpoint test (open it, then list its collections with their roles, see "A check lists collections with their roles") and the send channel by `check`'s channel test, under their spinners. Once a test passed, the wizard SHALL offer to store the secret once through pimalaya-cli's `keyring::prompt_store`, under the entry `neverest-<account name>-<source>` (`imap`, `smtp`, `carddav`, `caldav`) of the first source it signs in, the account name being the one the account is saved under, and the sources sharing the secret sharing the entry. The configuration SHALL hold the stored secret's read command, never the typed value unless raw storage was picked.

#### Scenario: A password stored in a keyring
- GIVEN mail, sending and contacts signed in with one password the tests accepted
- WHEN the user stores it in an installed keyring
- THEN the entry `neverest-<account name>-imap` holds it, and `imap.sasl`, `smtp.sasl` and `carddav.auth` read it with the keyring's command

### Requirement: OAuth hands off to Ortie
Neverest runs no OAuth 2.0 grant itself. With `ortie` on the `PATH`, an OAuth sign-in SHALL run pimalaya-cli's `ortie::sign_in` with the typed address (the domain when none was typed), the Ortie account `neverest-<account name>` (suffixed with its first service when the account needs several OAuth sign-ins) and the services it covers, in domain order, by their discovery wire names (`imap`, `smtp`, `carddav`, `caldav`, `gmail`, `gpeople`, `gcal`, `msgraph`, `msgraphContacts`, `msgraphCalendar`), every source it covers reading the token with the command it answers. Without it, the wizard SHALL say in one line that a token broker issues the token and that Ortie can sign in for it, then offer the token brokers through pimalaya-cli's token picker.

#### Scenario: A provider account
- GIVEN mail over the Gmail API, sending over SMTP, contacts and calendar over the provider's APIs, all through one authorization server, and `ortie` on the `PATH`
- WHEN they sign in for the account `perso`
- THEN `ortie configure --name neverest-perso --service gmail --service smtp --service gpeople --service gcal -- <address>` runs once, then `ortie auth get -a neverest-perso`
- AND every source and the channel read the token with `ortie token show -a neverest-perso`

### Requirement: A refusal offers a way on
A failed sign-in or connection test SHALL be reported for its domain, the domains that passed being kept, and SHALL offer `Try again` (retype the secret, or sign in again), `Change the login` (ask the login, then retype; where the sign-in uses a login), `Leave <domain> out` (only when another domain is set up or still to come) and `Back to the review`. Leaving mail out SHALL leave sending out too.

#### Scenario: Calendar refuses the password
- GIVEN mail and calendar sharing one password, which IMAP accepts and CalDAV refuses
- WHEN the refusal shows
- THEN mail stays signed in, and `Leave calendar out` saves an account without a calendar source

### Requirement: The collections to sync are picked last
Once every domain is signed in, the wizard SHALL offer, for each source in domain order (mail, contacts, calendar), the collections its sign-in test listed, in a multi-select through pimalaya-cli's `prompt::items`, every one picked except, for mail, those whose stated role is `junk` or `trash`. All picked SHALL write nothing; otherwise the source SHALL write the unpicked ones as `collection.filter.exclude`, by the id the listing names them by, so a collection created later still syncs. A source that listed no collection SHALL be asked nothing.

#### Scenario: Junk and trash left out
- GIVEN an IMAP source listing `INBOX`, `Sent`, `Junk` (role `junk`) and `Trash` (role `trash`)
- WHEN the folders show
- THEN `INBOX` and `Sent` are picked, and confirming writes `imap.collection.filter.exclude = ["Junk", "Trash"]`

#### Scenario: Everything picked
- GIVEN a CardDAV source listing two address books, both kept
- WHEN the address books are confirmed
- THEN the source writes no `collection` key
