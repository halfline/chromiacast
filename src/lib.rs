//! Codec-agnostic sender-side implementation of Cast Streaming.
//!
//! The public boundary accepts complete encoded media access units and does
//! not depend on a capture API, media framework, desktop environment, or
//! encoder. Packet layouts and cryptographic machinery remain private so the
//! integration API can evolve independently of protocol internals.

mod answer;
mod codec;
mod constants;
mod control;
mod crypto;
#[cfg(feature = "discovery")]
mod discovery;
mod error;
mod frame;
mod ntp;
mod offer;
mod rtcp;
mod rtp;
mod sender;
mod sync_source;
mod tls;
mod transport;

pub use answer::{Answer, AudioConstraints, Constraints, DisplayDescription, VideoConstraints};
pub use codec::{AudioCodec, CastMode, Framerate, Resolution, StreamType, VideoCodec};
pub use control::{
    AppAvailability, AuthenticatedDeviceInfo, AuthenticatedEurekaInfo, CastApp, CastConnection,
    ControlCloseReason, ControlEvent, DeviceIdentity, EurekaInfoOutcome, ReceiverStatus,
    APP_MIRRORING, CAST_PORT,
};
#[cfg(feature = "discovery")]
pub use discovery::{discover, CastCapabilities, CastDevice, CastEndpoint};
pub use error::{EnqueueError, Error, SenderError};
pub use frame::{EncodedFrame, FrameDependency, FrameId};
pub use offer::{AudioStreamConfig, Offer, OfferBuilder, VideoStreamConfig};
pub use sender::{SenderEvent, SenderSession, SessionStatistics, StreamHandle, StreamStatistics};
pub use transport::{Transport, UdpTransport};
