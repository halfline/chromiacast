//! Minimal protobuf surface used by Android TV Remote Service v2.
//!
//! The framing tests use a deployed message shape so they exercise the same
//! encoding rules as the pairing and control protocols built on this layer.

use prost::Message;

#[derive(Clone, PartialEq, Message)]
pub(crate) struct RemoteMessage {
    #[prost(message, optional, tag = "10")]
    pub key_inject: Option<RemoteKeyInject>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct RemoteKeyInject {
    #[prost(int32, tag = "1")]
    pub key_code: i32,
    #[prost(int32, tag = "2")]
    pub direction: i32,
}
