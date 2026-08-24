//! Android TV Remote Service support.
//!
//! The protocol uses bounded protobuf frames independently from Google Cast.
//! Later modules build pairing and control connections on this transport.

mod connection;
mod error;
mod framing;
mod identity;
mod pairing;
mod proto;
mod remote_session;
mod remote_types;

pub use error::AndroidTvError;
pub use identity::AndroidTvRemoteIdentity;
pub use pairing::AndroidTvPairingSession;
pub use remote_types::{
    AndroidTvDeviceInfo, AndroidTvKeyAction, AndroidTvKeyCode, AndroidTvRemoteClientInfo,
    AndroidTvRemoteEvent, AndroidTvRemoteFeatures, AndroidTvVolume,
};

/// Default TCP port for the Android TV pairing service.
pub const ANDROID_TV_PAIRING_PORT: u16 = 6467;
