//! Codec-agnostic sender-side implementation of Cast Streaming.
//!
//! The public boundary accepts complete encoded media access units and does
//! not depend on a capture API, media framework, desktop environment, or
//! encoder. Packet layouts and cryptographic machinery remain private so the
//! integration API can evolve independently of protocol internals.

mod codec;
mod constants;
mod crypto;
mod error;
mod frame;
mod ntp;
mod offer;
mod rtcp;
mod rtp;
mod sync_source;
mod transport;

pub use codec::{AudioCodec, CastMode, Framerate, Resolution, StreamType, VideoCodec};
pub use error::{EnqueueError, Error, SenderError};
pub use frame::{EncodedFrame, FrameDependency, FrameId};
pub use offer::{AudioStreamConfig, Offer, OfferBuilder, VideoStreamConfig};
pub use transport::{Transport, UdpTransport};
