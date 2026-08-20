pub(crate) mod event_loop;
pub(crate) mod stats;
pub(crate) mod stream;

use std::net::SocketAddr;
use tokio::sync::{mpsc, oneshot};

use crate::answer::Answer;
use crate::codec::StreamType;
use crate::constants::{MAX_RTP_PACKET_SIZE_IPV4, MAX_RTP_PACKET_SIZE_IPV6};
use crate::error::{EnqueueError, Error, SenderError};
use crate::frame::{EncodedFrame, FrameId};
use crate::offer::Offer;
use crate::transport::Transport;

use event_loop::StreamCommand;
use stream::{StreamParameters, StreamState};

pub use event_loop::SenderEvent;
pub use stats::{SessionStatistics, StreamStatistics};

/// A streaming session sending encoded frames to a Cast receiver.
///
/// Created by negotiating an `Offer` with an `Answer`, then calling `start()`.
/// Spawns a single background task that handles burst-paced transmission,
/// RTCP feedback parsing, and Sender Reports.
pub struct SenderSession {
    command_sender: mpsc::Sender<StreamCommand>,
    has_audio: bool,
    has_video: bool,
    task: tokio::task::JoinHandle<Result<(), SenderError>>,
}

/// A handle for sending frames on a specific stream (audio or video).
///
/// Cheaply cloneable. Multiple handles can send to the same stream.
#[derive(Clone)]
pub struct StreamHandle {
    stream_type: StreamType,
    command_sender: mpsc::Sender<StreamCommand>,
}

impl SenderSession {
    /// Start a streaming session.
    ///
    /// - `offer`: the Offer you sent to the receiver (contains AES keys and SSRCs)
    /// - `answer`: the Answer received from the receiver
    /// - `remote_ip`: the receiver's IP, combined with `answer.udp_port`
    pub async fn start<T: Transport>(
        offer: &Offer,
        answer: &Answer,
        remote_ip: std::net::IpAddr,
        transport: T,
    ) -> Result<(Self, mpsc::Receiver<SenderEvent>), Error> {
        Self::start_address(
            offer,
            answer,
            SocketAddr::new(remote_ip, answer.udp_port),
            transport,
        )
        .await
    }

    /// Start a streaming session using a scoped receiver endpoint.
    ///
    /// The endpoint's port is replaced with the media port from `answer`.
    /// Unlike [`start`](Self::start), this preserves the scope identifier of
    /// an IPv6 link-local route returned by discovery.
    pub async fn start_address<T: Transport>(
        offer: &Offer,
        answer: &Answer,
        mut remote_address: SocketAddr,
        transport: T,
    ) -> Result<(Self, mpsc::Receiver<SenderEvent>), Error> {
        answer.validate().map_err(Error::NegotiationFailed)?;

        remote_address = media_address(remote_address, answer.udp_port);
        let max_packet_size = if remote_address.is_ipv6() {
            MAX_RTP_PACKET_SIZE_IPV6
        } else {
            MAX_RTP_PACKET_SIZE_IPV4
        };

        let mut audio_stream = None;
        let mut video_stream = None;

        for (accepted_index, &offer_index) in answer.send_indexes.iter().enumerate() {
            let sender_sync_source = offer.stream_sync_source(offer_index).ok_or_else(|| {
                Error::NegotiationFailed(format!("send_index {} not found in offer", offer_index))
            })?;
            let aes_key = *offer.stream_aes_key(offer_index).ok_or_else(|| {
                Error::NegotiationFailed(format!("stream {offer_index} omitted its AES key"))
            })?;
            let aes_iv_mask = *offer.stream_aes_iv_mask(offer_index).ok_or_else(|| {
                Error::NegotiationFailed(format!("stream {offer_index} omitted its AES IV mask"))
            })?;
            let rtp_timebase = offer.stream_rtp_timebase(offer_index).ok_or_else(|| {
                Error::NegotiationFailed(format!("stream {offer_index} omitted its RTP timebase"))
            })?;
            let rtp_payload_type = offer.stream_rtp_payload_type(offer_index).ok_or_else(|| {
                Error::NegotiationFailed(format!(
                    "stream {offer_index} omitted its RTP payload type"
                ))
            })?;
            let target_playout_delay = offer.stream_target_delay(offer_index).ok_or_else(|| {
                Error::NegotiationFailed(format!("stream {offer_index} omitted its target delay"))
            })?;
            let stream_type = offer.stream_type(offer_index).ok_or_else(|| {
                Error::NegotiationFailed(format!("send_index {} not found in offer", offer_index))
            })?;
            if answer.sync_sources[accepted_index] == sender_sync_source {
                return Err(Error::NegotiationFailed(format!(
                    "stream {offer_index} uses the same sender and receiver SSRC"
                )));
            }

            let state = StreamState::new(StreamParameters {
                sender_sync_source,
                receiver_sync_source: answer.sync_sources[accepted_index],
                rtp_payload_type,
                rtp_timebase,
                target_playout_delay,
                aes_key,
                aes_iv_mask,
                max_packet_size,
            });

            match stream_type {
                StreamType::Audio => {
                    if audio_stream.is_some() {
                        return Err(Error::NegotiationFailed(
                            "receiver accepted more than one audio stream".into(),
                        ));
                    }
                    audio_stream = Some(state);
                }
                StreamType::Video => {
                    if video_stream.is_some() {
                        return Err(Error::NegotiationFailed(
                            "receiver accepted more than one video stream".into(),
                        ));
                    }
                    video_stream = Some(state);
                }
            }
        }

        if audio_stream.is_none() && video_stream.is_none() {
            return Err(Error::NoAcceptedStreams);
        }

        let has_audio = audio_stream.is_some();
        let has_video = video_stream.is_some();

        let (command_sender, command_receiver) = mpsc::channel(64);
        let (event_sender, event_receiver) = mpsc::channel(64);

        let task = tokio::spawn(event_loop::run(
            transport,
            remote_address,
            audio_stream,
            video_stream,
            command_receiver,
            event_sender,
        ));

        Ok((
            SenderSession {
                command_sender,
                has_audio,
                has_video,
                task,
            },
            event_receiver,
        ))
    }

    /// Get a handle for the audio stream, if the receiver accepted one.
    pub fn audio(&self) -> Option<StreamHandle> {
        if self.has_audio {
            Some(StreamHandle {
                stream_type: StreamType::Audio,
                command_sender: self.command_sender.clone(),
            })
        } else {
            None
        }
    }

    /// Get a handle for the video stream, if the receiver accepted one.
    pub fn video(&self) -> Option<StreamHandle> {
        if self.has_video {
            Some(StreamHandle {
                stream_type: StreamType::Video,
                command_sender: self.command_sender.clone(),
            })
        } else {
            None
        }
    }

    /// Return a current pressure and receiver-feedback snapshot.
    pub async fn statistics(&self) -> Result<SessionStatistics, SenderError> {
        let (result_sender, result_receiver) = oneshot::channel();
        self.command_sender
            .send(StreamCommand::Statistics { result_sender })
            .await
            .map_err(|_| SenderError::SessionClosed)?;
        result_receiver
            .await
            .map_err(|_| SenderError::SessionClosed)
    }

    /// Shut down the session and return its terminal result.
    pub async fn shutdown(self) -> Result<(), SenderError> {
        let (result_sender, result_receiver) = oneshot::channel();
        if self
            .command_sender
            .send(StreamCommand::Shutdown { result_sender })
            .await
            .is_ok()
        {
            let _ = result_receiver.await;
        }
        self.task
            .await
            .map_err(|error| SenderError::TaskFailed(error.to_string()))?
    }
}

fn media_address(mut endpoint: SocketAddr, port: u16) -> SocketAddr {
    endpoint.set_port(port);
    endpoint
}

impl StreamHandle {
    /// Send an encoded frame. Returns the assigned FrameId on success.
    ///
    /// This is async only because it waits for the event loop to accept the
    /// command. The actual work (encryption, packetization) happens in the
    /// event loop task.
    pub async fn send(&self, frame: EncodedFrame) -> Result<FrameId, EnqueueError> {
        let (result_sender, result_receiver) = oneshot::channel();
        self.command_sender
            .send(StreamCommand::EnqueueFrame {
                stream: self.stream_type,
                frame,
                result_sender,
            })
            .await
            .map_err(|_| EnqueueError::SessionClosed)?;

        result_receiver
            .await
            .map_err(|_| EnqueueError::SessionClosed)?
    }

    pub fn stream_type(&self) -> StreamType {
        self.stream_type
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_address_preserves_ipv6_scope() {
        let endpoint: SocketAddr = "[fe80::1%7]:8009".parse().unwrap();
        let media = media_address(endpoint, 2344);

        assert_eq!(media, "[fe80::1%7]:2344".parse().unwrap());
    }
}
