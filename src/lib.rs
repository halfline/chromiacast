//! Codec-agnostic sender-side implementation of Cast Streaming.
//!
//! The public boundary accepts complete encoded media access units and does
//! not depend on a capture API, media framework, desktop environment, or
//! encoder. Packet layouts and cryptographic machinery remain private so the
//! integration API can evolve independently of protocol internals.

mod constants;
mod error;
mod frame;
mod ntp;

pub use error::{EnqueueError, Error, SenderError};
pub use frame::{EncodedFrame, FrameDependency, FrameId};
