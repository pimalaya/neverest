//! What the live suites share: tokens minted from the environment, a
//! [`Replica`] (one neverest client with its own store) and the round trip
//! every mutable-content backend runs ([`round_trip`]).
//!
//! Two replicas of one account are two clients of one server. What one
//! pushes, the other pulls through its delta round, so a single server
//! proves the pull and the push plumbing with no vendor call but the setup
//! and the cleanup of a throwaway collection.
//!
//! Google tokens are minted from a service account key with domain-wide
//! delegation, inline in `GOOGLE_SERVICE_ACCOUNT_KEY` or at the path
//! `GOOGLE_SERVICE_ACCOUNT_KEY_FILE`, acting as
//! `GOOGLE_SERVICE_ACCOUNT_SUBJECT` (`google@pimalaya.org` by default).
//! Microsoft tokens are app-only, minted from `MSGRAPH_TENANT_ID`,
//! `MSGRAPH_CLIENT_ID` and `MSGRAPH_CLIENT_SECRET`, acting on
//! `MSGRAPH_USER_ID` (`microsoft@pimalaya.onmicrosoft.com` by default).

#![allow(dead_code)]

use std::{
    borrow::Cow,
    collections::BTreeSet,
    env, fs,
    io::{Read, Write},
    panic::{self, AssertUnwindSafe},
    path::PathBuf,
    process::Command,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use io_oauth::{
    client::Oauth20ClientStd,
    rfc6749::client_credentials::*,
    rfc7523::{
        assertion::{Oauth20JwtBearerClaims, Oauth20JwtBearerKey},
        auth_grant::Oauth20JwtBearerGrantRequestParams,
    },
};
use io_pimdir::{
    client::{
        blobs::PimdirBlobs,
        producer::PimdirProducer,
        reader::{PimdirItem, PimdirReader},
    },
    codec::PimdirAction,
    object::PimdirObject,
    placement::PimdirFlags,
};
use pimalaya_stream::tls::Tls;
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use tempfile::TempDir;
use url::Url;

/// The Pimalaya Workspace user the service account acts as by default.
const GOOGLE_SUBJECT: &str = "google@pimalaya.org";

/// The Pimalaya test mailbox an app-only Graph token acts on by default.
const MSGRAPH_USER: &str = "microsoft@pimalaya.onmicrosoft.com";

/// The scope of an app-only Graph token: every permission the app holds.
const MSGRAPH_SCOPE: &str = "https://graph.microsoft.com/.default";

/// The account name every replica's configuration declares.
const ACCOUNT: &str = "live";

/// The environment variable the configured token command prints.
const TOKEN_VAR: &str = "NEVEREST_LIVE_TOKEN";

/// How long a change may take to show on the other replica.
const SETTLE: Duration = Duration::from_secs(90);

/// A name no earlier run used, for every resource a run creates.
pub fn tag() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis();

    format!("neverest-live-{millis}")
}

/// One neverest client of the live account, with its own store.
pub struct Replica {
    dir: TempDir,
    token: String,
    backend: String,
}

impl Replica {
    /// Writes an account holding one `backend` source, plus `extra` TOML
    /// lines, its token read from the environment of each run.
    pub fn open(backend: &str, extra: &str, token: &str) -> Self {
        let dir = tempfile::tempdir().unwrap();

        let config = format!(
            "[accounts.{ACCOUNT}]\n\
             {backend}.auth.token.command = \"printenv {TOKEN_VAR}\"\n\
             {extra}\n",
        );
        fs::write(dir.path().join("config.toml"), config).unwrap();

        let replica = Self {
            dir,
            token: token.to_owned(),
            backend: backend.to_owned(),
        };
        replica.run(&["init", "-a", ACCOUNT], &[0]);
        replica
    }

    /// The store collection id of the backend's collection `key`.
    pub fn collection(&self, key: &str) -> String {
        format!("{}/{key}", self.backend)
    }

    /// Syncs the account narrowed to the collection `key`, which must
    /// succeed with nothing left to decide.
    pub fn sync(&self, key: &str) -> String {
        self.run(&["sync", "-a", ACCOUNT, "-m", key, "--json"], &[0])
            .1
    }

    /// Runs neverest on the account, which must end on one of `codes`.
    fn run(&self, args: &[&str], codes: &[i32]) -> (i32, String) {
        let output = Command::new(env!("CARGO_BIN_EXE_neverest"))
            .arg("-c")
            .arg(self.dir.path().join("config.toml"))
            .args(args)
            .env("XDG_STATE_HOME", self.dir.path())
            .env(TOKEN_VAR, &self.token)
            .output()
            .expect("spawn neverest");

        let code = output.status.code().unwrap_or(-1);
        assert!(
            codes.contains(&code),
            "`neverest {}` ended on {code}:\n--- stdout ---\n{}\n--- stderr ---\n{}",
            args.join(" "),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );

        (code, String::from_utf8_lossy(&output.stdout).into_owned())
    }

    /// Syncs until a run ends settled and `check` holds on the store.
    ///
    /// A provider acknowledges a write before every read reflects it, and
    /// may touch an item on its own (Exchange bumps an event's change key
    /// after creating it), which parks a push until the next run merges it.
    pub fn sync_until(&self, key: &str, what: &str, mut check: impl FnMut(&Self) -> bool) {
        let deadline = Instant::now() + SETTLE;

        loop {
            let (code, _) = self.run(&["sync", "-a", ACCOUNT, "-m", key, "--json"], &[0, 2]);
            if code == 0 && check(self) {
                return;
            }
            if Instant::now() > deadline {
                panic!("timed out waiting until {what}");
            }
            thread::sleep(Duration::from_secs(3));
        }
    }

    /// The store directory.
    pub fn store(&self) -> PathBuf {
        self.dir.path().join("neverest").join(ACCOUNT)
    }

    /// The live item of `key` whose link id contains `tag`.
    pub fn item(&self, key: &str, tag: &str) -> Option<PimdirItem> {
        PimdirReader::open(self.store())
            .expect("open the store")
            .list_items(self.collection(key), None, 10_000)
            .expect("list the collection")
            .into_iter()
            .find(|item| item.link_id.0.contains(tag))
    }

    /// The body of a stored item, empty when it holds none.
    pub fn body(&self, item: &PimdirItem) -> String {
        let Some(hash) = &item.object else {
            return String::new();
        };

        let reader = PimdirReader::open(self.store()).expect("open the store");
        let blobs = PimdirBlobs::open(self.store(), reader.hash_algo());
        let mut body = String::new();
        if let Some(mut file) = blobs.reader(hash).expect("open the blob") {
            file.read_to_string(&mut body).expect("read the blob");
        }
        body
    }

    /// Stages a creation, as a frontend does.
    pub fn add(&self, key: &str, body: &str, flags: &[&str]) {
        let (hash, object) = self.stage(body);
        let action = PimdirAction::Add {
            link_id: None,
            flags: known(flags),
            object: Some(hash),
        };
        self.enqueue(key, action, Some(object));
    }

    /// Stages a new body for an item.
    pub fn update(&self, key: &str, seq: i64, body: &str) {
        let (hash, object) = self.stage(body);
        self.enqueue(
            key,
            PimdirAction::Update { seq, object: hash },
            Some(object),
        );
    }

    /// Stages a whole new flag set for an item.
    pub fn set_flags(&self, key: &str, seq: i64, flags: &[&str]) {
        let action = PimdirAction::SetFlags {
            seq,
            flags: known(flags),
        };
        self.enqueue(key, action, None);
    }

    /// Stages the removal of an item.
    pub fn remove(&self, key: &str, seq: i64) {
        self.enqueue(key, PimdirAction::Remove { seq }, None);
    }

    /// Writes a body to the blob tree, durably, before any queue row names it.
    fn stage(&self, body: &str) -> (io_pimdir::object::PimdirHash, PimdirObject) {
        let producer = PimdirProducer::open(self.store(), "neverest-live").expect("open producer");
        let hash = producer.hash(body.as_bytes());
        let blobs = PimdirBlobs::open(self.store(), producer.hash_algo());
        let mut writer = blobs.writer().expect("blob writer");
        writer.write_all(body.as_bytes()).unwrap();
        let object = PimdirObject {
            hash: hash.clone(),
            size: writer.commit(&hash).expect("commit the body") as usize,
        };
        (hash, object)
    }

    fn enqueue(&self, key: &str, action: PimdirAction, object: Option<PimdirObject>) {
        PimdirProducer::open(self.store(), "neverest-live")
            .expect("open producer")
            .enqueue(&self.collection(key), &action, object.as_ref())
            .expect("enqueue the action");
    }
}

fn known(flags: &[&str]) -> PimdirFlags {
    PimdirFlags::Known(
        flags
            .iter()
            .map(|flag| flag.to_string())
            .collect::<BTreeSet<_>>(),
    )
}

/// The round trip of a mutable-content backend over its collection `key`.
///
/// `body(uid, marker)` renders the item. One replica adds it and the other
/// pulls it, that one edits the body it pulled, as a frontend does, and the
/// first pulls the edit. A fresh replica reads it in a full round, then the
/// first removes it and the other sees it go.
pub fn round_trip(
    backend: &str,
    extra: &str,
    token: &str,
    key: &str,
    body: impl Fn(&str, &str) -> String,
) {
    let a = Replica::open(backend, extra, token);
    let b = Replica::open(backend, extra, token);
    let uid = tag();

    a.sync(key);
    b.sync(key);

    a.add(key, &body(&uid, "created"), &[]);
    a.sync(key);
    assert!(a.item(key, &uid).is_some(), "the added item stays live");

    b.sync_until(key, "the creation reaches the other replica", |b| {
        b.item(key, &uid)
            .is_some_and(|item| b.body(&item).contains("created"))
    });

    let item = b.item(key, &uid).unwrap();
    b.update(key, item.seq, &b.body(&item).replace("created", "edited"));
    b.sync_until(key, "the edit settles", |_| true);

    a.sync_until(key, "the edit reaches the first replica", |a| {
        a.item(key, &uid)
            .is_some_and(|item| a.body(&item).contains("edited"))
    });

    let c = Replica::open(backend, extra, token);
    c.sync(key);
    let item = c.item(key, &uid).expect("a full round reads the item");
    assert!(c.body(&item).contains("edited"), "with its edited body");

    let seq = a.item(key, &uid).unwrap().seq;
    a.remove(key, seq);
    a.sync_until(key, "the removal settles", |_| true);

    b.sync_until(key, "the removal reaches the other replica", |b| {
        b.item(key, &uid).is_none()
    });
}

/// Runs `body`, then `cleanup` whichever way `body` went, and only then
/// re-raises a panic `body` may have raised, so a failed run leaves nothing
/// behind on the live account.
pub fn with_cleanup(body: impl FnOnce(), cleanup: impl FnOnce()) {
    let outcome = panic::catch_unwind(AssertUnwindSafe(body));

    if panic::catch_unwind(AssertUnwindSafe(cleanup)).is_err() {
        eprintln!("WARNING: cleanup failed, the live account may hold leftovers");
    }

    if let Err(payload) = outcome {
        panic::resume_unwind(payload);
    }
}

/// The subset of a service account key the JWT bearer grant needs.
#[derive(Deserialize)]
struct ServiceAccountKey {
    client_email: String,
    private_key: String,
    #[serde(default = "default_token_uri")]
    token_uri: String,
}

fn default_token_uri() -> String {
    String::from("https://oauth2.googleapis.com/token")
}

/// A Google access token for `scope`, traded for a JWT the service account
/// signs on behalf of the delegated subject (RFC 7523 section 2.1).
pub fn google_token(scope: &str) -> String {
    let key = match env::var("GOOGLE_SERVICE_ACCOUNT_KEY") {
        Ok(key) if !key.is_empty() => key,
        _ => {
            let path = env::var("GOOGLE_SERVICE_ACCOUNT_KEY_FILE").expect(
                "set GOOGLE_SERVICE_ACCOUNT_KEY or GOOGLE_SERVICE_ACCOUNT_KEY_FILE to mint a token",
            );
            fs::read_to_string(&path).expect("read the service account key")
        }
    };
    let key: ServiceAccountKey = serde_json::from_str(&key).expect("parse the service account key");
    let subject =
        env::var("GOOGLE_SERVICE_ACCOUNT_SUBJECT").unwrap_or_else(|_| String::from(GOOGLE_SUBJECT));

    let signer = Oauth20JwtBearerKey::from_pkcs8_pem(&key.private_key)
        .expect("the service account key holds a PKCS#8 private key");
    let token_uri: Url = key.token_uri.parse().expect("the token URI is a URL");
    let mut client =
        Oauth20ClientStd::connect(token_uri, &Tls::default(), key.client_email.as_str())
            .expect("connect to the token endpoint");

    let claims = Oauth20JwtBearerClaims {
        iss: key.client_email.as_str().into(),
        sub: Some(subject.into()),
        scope: [Cow::from(scope.to_owned())].into_iter().collect(),
        ..Default::default()
    };
    let assertion = client
        .sign_jwt_bearer_assertion(&signer, claims, None, Duration::from_secs(600))
        .expect("sign the assertion");
    let params = Oauth20JwtBearerGrantRequestParams {
        assertion,
        scope: Default::default(),
    };

    match client
        .request_jwt_bearer_grant(params)
        .expect("request the token")
    {
        Ok(granted) => granted.access_token.expose_secret().to_owned(),
        Err(err) => panic!("the token endpoint refused the assertion: {err:?}"),
    }
}

/// The mailbox an app-only Graph token acts on.
pub fn msgraph_user() -> String {
    env::var("MSGRAPH_USER_ID").unwrap_or_else(|_| String::from(MSGRAPH_USER))
}

/// An app-only Graph token, traded for the app's client secret (RFC 6749
/// section 4.4).
pub fn msgraph_token() -> String {
    let var = |name: &str| {
        env::var(name).unwrap_or_else(|_| {
            panic!(
                "set MSGRAPH_TENANT_ID, MSGRAPH_CLIENT_ID and MSGRAPH_CLIENT_SECRET \
                 to mint a token ({name} is missing)"
            )
        })
    };
    let tenant = var("MSGRAPH_TENANT_ID");
    let client_id = var("MSGRAPH_CLIENT_ID");

    let token_uri: Url = format!("https://login.microsoftonline.com/{tenant}/oauth2/v2.0/token")
        .parse()
        .expect("the token URI is a URL");
    let mut client = Oauth20ClientStd::connect(token_uri, &Tls::default(), client_id.as_str())
        .expect("connect to the token endpoint");
    client.client_secret = Some(SecretString::from(var("MSGRAPH_CLIENT_SECRET")));

    let params = Oauth20ClientCredentialsRequestParams {
        scope: [Cow::from(MSGRAPH_SCOPE)].into_iter().collect(),
    };

    match client
        .request_client_credentials(params)
        .expect("request the token")
    {
        Ok(granted) => granted.access_token.expose_secret().to_owned(),
        Err(err) => panic!("the token endpoint refused the client: {err:?}"),
    }
}

/// A vCard 4.0 named `<marker> <uid>`, as given and family names, so a
/// provider deriving the display name from them answers the same `FN`.
pub fn card(uid: &str, marker: &str) -> String {
    format!(
        "BEGIN:VCARD\r\n\
         VERSION:4.0\r\n\
         UID:{uid}\r\n\
         FN:{marker} {uid}\r\n\
         N:{uid};{marker};;;\r\n\
         END:VCARD\r\n",
    )
}

/// A one-hour event whose `SUMMARY` carries `marker`.
pub fn event(uid: &str, marker: &str) -> String {
    format!(
        "BEGIN:VCALENDAR\r\n\
         VERSION:2.0\r\n\
         PRODID:-//pimalaya//neverest live tests//EN\r\n\
         BEGIN:VEVENT\r\n\
         UID:{uid}\r\n\
         DTSTAMP:20260101T000000Z\r\n\
         DTSTART:20261110T090000Z\r\n\
         DTEND:20261110T100000Z\r\n\
         SUMMARY:Live {marker}\r\n\
         END:VEVENT\r\n\
         END:VCALENDAR\r\n",
    )
}

/// An RFC 5322 message whose `Message-ID` is `<tag@pimalaya.org>`.
pub fn message(tag: &str, to: &str) -> String {
    format!(
        "From: Neverest <{to}>\r\n\
         To: {to}\r\n\
         Subject: {tag}\r\n\
         Date: Thu, 01 Jan 2026 00:00:00 +0000\r\n\
         Message-ID: <{tag}@pimalaya.org>\r\n\
         MIME-Version: 1.0\r\n\
         Content-Type: text/plain; charset=utf-8\r\n\
         \r\n\
         Sent by the neverest live tests.\r\n",
    )
}

/// The flags of an item, empty when unknown.
pub fn flags(item: &PimdirItem) -> BTreeSet<String> {
    item.flags.known().cloned().unwrap_or_default()
}
