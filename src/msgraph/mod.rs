//! # Microsoft Graph backend
//!
//! [`client`] wraps the io-msgraph session behind the shared cross-protocol
//! client surface: delta enumeration, cached `Meta` rows, raw MIME bodies,
//! flag and delete pushes, sendMail.
//!
//! It is opened with a ready-made OAuth 2.0 bearer token resolved from the
//! side's [`crate::config::MsgraphAuthConfig`] secret command; neverest runs no
//! OAuth flow itself, sign-in and refresh living in an external tool.

pub mod client;
