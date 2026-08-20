use bytes::Bytes;
use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

use chromiacast::*;

struct MockTransport {
    sent: Arc<Mutex<Vec<Vec<u8>>>>,
    inbound_receiver: Arc<tokio::sync::Mutex<mpsc::Receiver<Vec<u8>>>>,
    remote_address: SocketAddr,
}

struct MockControl {
    sent: Arc<Mutex<Vec<Vec<u8>>>>,
    inbound_sender: mpsc::Sender<Vec<u8>>,
}

fn mock_transport() -> (MockTransport, MockControl) {
    let (inbound_sender, inbound_receiver) = mpsc::channel(64);
    let sent = Arc::new(Mutex::new(Vec::new()));
    let remote_address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 2344);

    (
        MockTransport {
            sent: sent.clone(),
            inbound_receiver: Arc::new(tokio::sync::Mutex::new(inbound_receiver)),
            remote_address,
        },
        MockControl { sent, inbound_sender },
    )
}

impl Transport for MockTransport {
    async fn send_to(&self, data: &[u8], _address: SocketAddr) -> io::Result<usize> {
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
}

fn build_test_offer() -> Offer {
    Offer::builder()
        .audio(AudioStreamConfig::default())
        .video(VideoStreamConfig::default())
        .build()
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
async fn multiple_frames_sequential() {
    let offer = build_test_offer();
    let answer = build_test_answer();
    let (transport, control) = mock_transport();

    let (session, _events) =
        SenderSession::start(&offer, &answer, IpAddr::V4(Ipv4Addr::LOCALHOST), transport)
            .await
            .unwrap();

    let video = session.video().unwrap();

    for i in 0..10 {
        let dependency = if i == 0 {
            FrameDependency::KeyFrame
        } else {
            FrameDependency::Delta
        };
        let id = video
            .send(encoded_frame(
                dependency,
                Bytes::from(vec![i as u8; 1000]),
                Duration::from_millis(i as u64 * 10),
            ))
            .await
            .unwrap();

        assert_eq!(id, FrameId::first() + i);
    }

    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(control.sent_count() >= 10);

    session.shutdown().await.unwrap();
}
