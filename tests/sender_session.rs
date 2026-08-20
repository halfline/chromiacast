use bytes::Bytes;
use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

use chromiacast::*;

struct MockTransport {
    sent: Arc<Mutex<Vec<Vec<u8>>>>,
    inbound_receiver: Arc<tokio::sync::Mutex<mpsc::Receiver<Vec<u8>>>>,
    remote_address: SocketAddr,
    fail_sends: Arc<AtomicBool>,
}

struct MockControl {
    sent: Arc<Mutex<Vec<Vec<u8>>>>,
    inbound_sender: mpsc::Sender<Vec<u8>>,
    fail_sends: Arc<AtomicBool>,
}

fn mock_transport() -> (MockTransport, MockControl) {
    let (inbound_sender, inbound_receiver) = mpsc::channel(64);
    let sent = Arc::new(Mutex::new(Vec::new()));
    let remote_address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 2344);
    let fail_sends = Arc::new(AtomicBool::new(false));

    (
        MockTransport {
            sent: sent.clone(),
            inbound_receiver: Arc::new(tokio::sync::Mutex::new(inbound_receiver)),
            remote_address,
            fail_sends: fail_sends.clone(),
        },
        MockControl {
            sent,
            inbound_sender,
            fail_sends,
        },
    )
}

impl Transport for MockTransport {
    async fn send_to(&self, data: &[u8], _address: SocketAddr) -> io::Result<usize> {
        if self.fail_sends.load(Ordering::Acquire) {
            return Err(io::Error::new(
                io::ErrorKind::NetworkUnreachable,
                "mock send failure",
            ));
        }
        self.sent.lock().unwrap().push(data.to_vec());
        Ok(data.len())
    }

    async fn recv_from(&self, buf: &mut [u8]) -> io::Result<(usize, SocketAddr)> {
        let mut rx = self.inbound_receiver.lock().await;
        match rx.recv().await {
            Some(data) => {
                let n = data.len().min(buf.len());
                buf[..n].copy_from_slice(&data[..n]);
                Ok((n, self.remote_address))
            }
            None => Err(io::Error::new(io::ErrorKind::ConnectionReset, "closed")),
        }
    }

    fn local_addr(&self) -> io::Result<SocketAddr> {
        Ok(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0))
    }
}

impl MockControl {
    fn sent_count(&self) -> usize {
        self.sent.lock().unwrap().len()
    }

    fn take_sent(&self) -> Vec<Vec<u8>> {
        std::mem::take(&mut *self.sent.lock().unwrap())
    }

    async fn inject(&self, data: Vec<u8>) {
        self.inbound_sender.send(data).await.unwrap();
    }

    fn fail_sends(&self) {
        self.fail_sends.store(true, Ordering::Release);
    }
}

fn build_test_offer() -> Offer {
    Offer::builder()
        .audio(AudioStreamConfig::default())
        .video(VideoStreamConfig::default())
        .build()
}

fn offer_sync_source(offer: &Offer, index: usize) -> u32 {
    serde_json::to_value(offer).unwrap()["supportedStreams"]
        .as_array()
        .unwrap()
        .iter()
        .find(|stream| stream["index"].as_u64() == Some(index as u64))
        .and_then(|stream| stream["ssrc"].as_u64())
        .and_then(|sync_source| u32::try_from(sync_source).ok())
        .unwrap()
}

fn build_test_answer() -> Answer {
    serde_json::from_value(serde_json::json!({
        "udpPort": 2344,
        "sendIndexes": [0, 1],
        "ssrcs": [60000, 30000]
    }))
    .unwrap()
}

fn encoded_frame(
    dependency: FrameDependency,
    data: impl Into<Bytes>,
    timestamp: Duration,
) -> EncodedFrame {
    EncodedFrame::new(dependency, data.into(), timestamp, Instant::now())
        .with_duration(Duration::from_millis(10))
}

fn build_cast_feedback_with_nack(
    receiver_sync_source: u32,
    sender_sync_source: u32,
    checkpoint: u8,
    frame_id: u8,
    packet_id: u16,
) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.push(0x80 | 15);
    buf.push(206);
    buf.extend_from_slice(&5u16.to_be_bytes());
    buf.extend_from_slice(&receiver_sync_source.to_be_bytes());
    buf.extend_from_slice(&sender_sync_source.to_be_bytes());
    buf.extend_from_slice(&0x4341_5354u32.to_be_bytes());
    buf.push(checkpoint);
    buf.push(1);
    buf.extend_from_slice(&400u16.to_be_bytes());
    buf.push(frame_id);
    buf.extend_from_slice(&packet_id.to_be_bytes());
    buf.push(0);
    buf
}

fn build_receiver_report(
    receiver_sync_source: u32,
    sender_sync_source: u32,
    last_sender_report: u32,
) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.push(0x80 | 1);
    buf.push(201);
    buf.extend_from_slice(&7u16.to_be_bytes());
    buf.extend_from_slice(&receiver_sync_source.to_be_bytes());
    buf.extend_from_slice(&sender_sync_source.to_be_bytes());
    buf.push(3);
    buf.extend_from_slice(&[0, 0, 2]);
    buf.extend_from_slice(&1234u32.to_be_bytes());
    buf.extend_from_slice(&7u32.to_be_bytes());
    buf.extend_from_slice(&last_sender_report.to_be_bytes());
    buf.extend_from_slice(&0u32.to_be_bytes());
    buf
}

fn compact_ntp_from_sender_report(packet: &[u8]) -> u32 {
    let seconds = u32::from_be_bytes(packet[8..12].try_into().unwrap());
    let fraction = u32::from_be_bytes(packet[12..16].try_into().unwrap());
    seconds.wrapping_shl(16) | (fraction >> 16)
}

#[tokio::test]
async fn start_session_and_send_frame() {
    let offer = build_test_offer();
    let answer = build_test_answer();
    let (transport, control) = mock_transport();

    let (session, _events) =
        SenderSession::start(&offer, &answer, IpAddr::V4(Ipv4Addr::LOCALHOST), transport)
            .await
            .unwrap();

    assert!(session.audio().is_some());
    assert!(session.video().is_some());

    let video = session.video().unwrap();
    let frame_id = video
        .send(encoded_frame(
            FrameDependency::KeyFrame,
            Bytes::from_static(b"fake h264 keyframe data"),
            Duration::ZERO,
        ))
        .await
        .unwrap();

    assert_eq!(frame_id, FrameId::first());

    // Let the burst timer fire
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Should have sent RTP packets
    assert!(control.sent_count() > 0);

    session.shutdown().await.unwrap();
}

#[tokio::test]
async fn audio_and_video_both_stream() {
    let offer = build_test_offer();
    let answer = build_test_answer();
    let (transport, control) = mock_transport();

    let (session, _events) =
        SenderSession::start(&offer, &answer, IpAddr::V4(Ipv4Addr::LOCALHOST), transport)
            .await
            .unwrap();

    let audio = session.audio().unwrap();
    let video = session.video().unwrap();

    audio
        .send(encoded_frame(
            FrameDependency::KeyFrame,
            Bytes::from_static(b"opus audio frame"),
            Duration::ZERO,
        ))
        .await
        .unwrap();

    video
        .send(encoded_frame(
            FrameDependency::KeyFrame,
            Bytes::from_static(b"h264 video frame"),
            Duration::ZERO,
        ))
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(50)).await;

    let sent = control.take_sent();
    assert!(sent.len() >= 2);

    session.shutdown().await.unwrap();
}

#[tokio::test]
async fn reject_invalid_answer() {
    let offer = build_test_offer();
    let bad_answer: Answer = serde_json::from_value(serde_json::json!({
        "udpPort": 0,
        "sendIndexes": [],
        "ssrcs": []
    }))
    .unwrap();

    let (transport, _control) = mock_transport();

    let result = SenderSession::start(
        &offer,
        &bad_answer,
        IpAddr::V4(Ipv4Addr::LOCALHOST),
        transport,
    )
    .await;

    assert!(result.is_err());
}
#[tokio::test]
async fn offer_json_roundtrip() {
    let offer = build_test_offer();
    let json = serde_json::to_value(&offer).unwrap();

    // Verify the JSON has the expected Cast protocol structure
    assert_eq!(json["castMode"], "mirroring");
    let streams = json["supportedStreams"].as_array().unwrap();
    assert_eq!(streams.len(), 2);

    // Audio stream
    let audio = &streams[0];
    assert_eq!(audio["type"], "audio_source");
    assert_eq!(audio["codecName"], "opus");
    assert_eq!(audio["rtpProfile"], "cast");
    assert_eq!(audio["channels"], 2);
    assert_eq!(audio["timeBase"], "1/48000");
    assert!(audio["ssrc"].as_u64().unwrap() <= 50_000);
    assert_eq!(audio["aesKey"].as_str().unwrap().len(), 32);
    assert_eq!(audio["aesIvMask"].as_str().unwrap().len(), 32);

    // Video stream
    let video = &streams[1];
    assert_eq!(video["type"], "video_source");
    assert_eq!(video["codecName"], "h264");
    assert_eq!(video["timeBase"], "1/90000");
    assert!(video["ssrc"].as_u64().unwrap() > 50_000);
    assert!(!video["resolutions"].as_array().unwrap().is_empty());
}
#[tokio::test]
async fn multiple_frames_sequential() {
    let offer = build_test_offer();
    let answer = build_test_answer();
    let (transport, control) = mock_transport();

    let (session, _events) =
        SenderSession::start(&offer, &answer, IpAddr::V4(Ipv4Addr::LOCALHOST), transport)
            .await
            .unwrap();

    let video = session.video().unwrap();

    // Send 10 frames
    for i in 0..10 {
        let dep = if i == 0 {
            FrameDependency::KeyFrame
        } else {
            FrameDependency::Delta
        };
        let id = video
            .send(encoded_frame(
                dep,
                Bytes::from(vec![i as u8; 1000]),
                Duration::from_millis(i as u64 * 10),
            ))
            .await
            .unwrap();

        assert_eq!(id, FrameId::first() + i);
    }

    // Wait for burst timer
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Should have sent packets for all 10 frames
    assert!(control.sent_count() >= 10);

    session.shutdown().await.unwrap();
}

#[tokio::test]
async fn statistics_report_pressure_ack_and_retransmission() {
    let offer = build_test_offer();
    let answer = build_test_answer();
    let (transport, control) = mock_transport();
    let (session, mut events) =
        SenderSession::start(&offer, &answer, IpAddr::V4(Ipv4Addr::LOCALHOST), transport)
            .await
            .unwrap();
    let video = session.video().unwrap();
    let video_sync_source = offer_sync_source(&offer, 1);

    video
        .send(encoded_frame(
            FrameDependency::KeyFrame,
            Bytes::from(vec![1; 2_000]),
            Duration::ZERO,
        ))
        .await
        .unwrap();
    video
        .send(encoded_frame(
            FrameDependency::Delta,
            Bytes::from(vec![2; 2_000]),
            Duration::from_millis(10),
        ))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(30)).await;

    control
        .inject(build_cast_feedback_with_nack(
            30000,
            video_sync_source,
            0,
            1,
            0,
        ))
        .await;

    let retransmission = tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if let Some(SenderEvent::PacketRetransmission {
                stream,
                frame_id,
                packets_queued,
                ..
            }) = events.recv().await
            {
                break (stream, frame_id, packets_queued);
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(retransmission, (StreamType::Video, FrameId::first() + 1, 1));

    tokio::time::sleep(Duration::from_millis(30)).await;
    let statistics = session.statistics().await.unwrap();
    let video_statistics = statistics.video.unwrap();
    assert_eq!(video_statistics.in_flight_frames, 1);
    assert_eq!(video_statistics.frames_acked, 1);
    assert_eq!(video_statistics.nack_count, 1);
    assert_eq!(video_statistics.packets_retransmitted, 1);
    assert_eq!(
        video_statistics.receiver_playout_delay,
        Some(Duration::from_millis(400))
    );
    assert!(video_statistics.packets_sent >= 4);

    session.shutdown().await.unwrap();
}

#[tokio::test]
async fn receiver_report_updates_rtt_and_loss_statistics() {
    let offer = build_test_offer();
    let answer = build_test_answer();
    let (transport, control) = mock_transport();
    let (session, _events) =
        SenderSession::start(&offer, &answer, IpAddr::V4(Ipv4Addr::LOCALHOST), transport)
            .await
            .unwrap();
    let video_sync_source = offer_sync_source(&offer, 1);

    tokio::time::sleep(Duration::from_millis(550)).await;
    let sender_report = control
        .take_sent()
        .into_iter()
        .find(|packet| {
            packet.len() >= 28
                && packet[1] == 200
                && u32::from_be_bytes(packet[4..8].try_into().unwrap()) == video_sync_source
        })
        .expect("video Sender Report");
    control
        .inject(build_receiver_report(
            30000,
            video_sync_source,
            compact_ntp_from_sender_report(&sender_report),
        ))
        .await;
    tokio::time::sleep(Duration::from_millis(20)).await;

    let statistics = session.statistics().await.unwrap();
    let video = statistics.video.unwrap();
    assert!(video.current_rtt.is_some());
    assert_eq!(video.fraction_lost, Some(3));
    assert_eq!(video.cumulative_packets_lost, Some(2));
    assert_eq!(video.highest_sequence_number, Some(1234));
    assert_eq!(video.jitter, Some(7));

    session.shutdown().await.unwrap();
}

#[tokio::test]
async fn malformed_rtcp_is_reported_without_killing_session() {
    let offer = build_test_offer();
    let answer = build_test_answer();
    let (transport, control) = mock_transport();
    let (session, mut events) =
        SenderSession::start(&offer, &answer, IpAddr::V4(Ipv4Addr::LOCALHOST), transport)
            .await
            .unwrap();

    control.inject(vec![0x80, 201, 0, 6, 0, 0, 0, 0]).await;
    let event = tokio::time::timeout(Duration::from_secs(1), events.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(event, SenderEvent::MalformedPacket { .. }));
    session.shutdown().await.unwrap();
}

#[tokio::test]
async fn udp_send_failure_is_terminal_and_observable() {
    let offer = build_test_offer();
    let answer = build_test_answer();
    let (transport, control) = mock_transport();
    let (session, mut events) =
        SenderSession::start(&offer, &answer, IpAddr::V4(Ipv4Addr::LOCALHOST), transport)
            .await
            .unwrap();
    control.fail_sends();
    session
        .video()
        .unwrap()
        .send(encoded_frame(
            FrameDependency::KeyFrame,
            Bytes::from_static(b"keyframe"),
            Duration::ZERO,
        ))
        .await
        .unwrap();

    let event = tokio::time::timeout(Duration::from_secs(1), events.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        event,
        SenderEvent::FatalError(SenderError::Transport { .. })
    ));
    assert!(matches!(
        session.shutdown().await,
        Err(SenderError::Transport { .. })
    ));
}

#[tokio::test(start_paused = true)]
async fn missing_receiver_feedback_times_out_session() {
    let offer = build_test_offer();
    let answer = build_test_answer();
    let (transport, _control) = mock_transport();
    let (session, mut events) =
        SenderSession::start(&offer, &answer, IpAddr::V4(Ipv4Addr::LOCALHOST), transport)
            .await
            .unwrap();
    session
        .video()
        .unwrap()
        .send(encoded_frame(
            FrameDependency::KeyFrame,
            Bytes::from_static(b"keyframe"),
            Duration::ZERO,
        ))
        .await
        .unwrap();

    let timeout_event = tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            if matches!(events.recv().await, Some(SenderEvent::ReceiverTimedOut)) {
                break;
            }
        }
    })
    .await;
    assert!(timeout_event.is_ok());
    assert_eq!(session.shutdown().await, Err(SenderError::ReceiverTimedOut));
}

#[tokio::test(start_paused = true)]
async fn receiver_reports_do_not_mask_missing_acknowledgements() {
    let offer = build_test_offer();
    let answer = build_test_answer();
    let (transport, control) = mock_transport();
    let (session, mut events) =
        SenderSession::start(&offer, &answer, IpAddr::V4(Ipv4Addr::LOCALHOST), transport)
            .await
            .unwrap();
    let video_sync_source = offer_sync_source(&offer, 1);
    session
        .video()
        .unwrap()
        .send(encoded_frame(
            FrameDependency::KeyFrame,
            Bytes::from_static(b"keyframe"),
            Duration::ZERO,
        ))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(20)).await;

    for _ in 0..4 {
        control
            .inject(build_receiver_report(30000, video_sync_source, 0))
            .await;
        tokio::time::advance(Duration::from_secs(1)).await;
        tokio::task::yield_now().await;
    }

    let event = tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            if let Some(SenderEvent::ReceiverTimedOut) = events.recv().await {
                break SenderEvent::ReceiverTimedOut;
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(event, SenderEvent::ReceiverTimedOut);
    assert_eq!(session.shutdown().await, Err(SenderError::ReceiverTimedOut));
}
