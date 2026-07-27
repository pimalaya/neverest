//! Microsoft Graph backend: the protocol-direct client adapter.
//!
//! [`client`] wraps the io-msgraph session behind the shared
//! cross-protocol client surface (delta enumeration, cached `Meta`
//! rows, raw MIME bodies, flag/delete pushes, sendMail). It is opened
//! with a ready-made OAuth 2.0 bearer token resolved from the side's
//! [`crate::config::MsgraphAuthConfig`] secret command; neverest runs
//! no OAuth flow itself (device sign-in, client credentials and token
//! refresh live in an external tool, typically ortie).

pub mod client;
