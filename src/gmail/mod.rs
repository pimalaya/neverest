//! # Gmail
//!
//! The Gmail backend: a mail source over io-gmail's REST client, its reads
//! sent in paced batches ([`batch`]) and shared across its connections for
//! the run ([`run`]).

mod batch;
pub mod client;
pub mod run;
