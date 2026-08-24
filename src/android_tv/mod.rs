//! Android TV Remote Service support.
//!
//! The protocol uses bounded protobuf frames independently from Google Cast.
//! Later modules build pairing and control connections on this transport.

mod error;
mod framing;
mod proto;

pub use error::AndroidTvError;
