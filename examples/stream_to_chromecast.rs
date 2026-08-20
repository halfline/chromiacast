/// Stream test frames to a Chromecast at 30fps.
///
/// Usage: cargo run --example stream_to_chromecast -- <receiver_ip>
use bytes::Bytes;
use std::net::{IpAddr, SocketAddr};
use std::time::{Duration, Instant};

use chromiacast::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let receiver_ip: IpAddr = std::env::args()
        .nth(1)
        .unwrap_or_else(|| {
            eprintln!("Usage: stream_to_chromecast <receiver_ip>");
            std::process::exit(1);
        })
        .parse()?;

    println!("Connecting to {}:{}...", receiver_ip, CAST_PORT);
    let conn = CastConnection::connect(receiver_ip).await?;
    println!("Connected.");

    println!("Launching mirroring receiver...");
    let app = conn.launch(APP_MIRRORING).await?;
    println!("App launched (transport: {})", app.transport_id());

    let offer = Offer::builder()
        .audio(AudioStreamConfig {
            codec: AudioCodec::Opus,
            bit_rate: 128_000,
            sample_rate: 48_000,
            channels: 2,
            target_delay: Duration::from_millis(400),
        })
        .video(VideoStreamConfig {
            codec: VideoCodec::H264,
            max_bit_rate: 5_000_000,
            max_frame_rate: Framerate::new(30, 1),
            resolutions: vec![Resolution::new(1920, 1080)],
            target_delay: Duration::from_millis(400),
        })
        .build();

    let answer = conn.exchange_offer(&offer, &app).await?;
    println!("Negotiated. UDP port: {}", answer.udp_port);

    let transport = UdpTransport::bind(SocketAddr::new("0.0.0.0".parse().unwrap(), 0)).await?;
    let (session, mut events) =
        SenderSession::start(&offer, &answer, receiver_ip, transport).await?;

    println!("Streaming to {}:{}", receiver_ip, answer.udp_port);

    let event_task = tokio::spawn(async move {
        while let Some(event) = events.recv().await {
            if let SenderEvent::FatalError(error) = event {
                eprintln!("sender failed: {error}");
            }
        }
    });

    if let Some(video) = session.video() {
        let frame_interval = Duration::from_millis(33);
        let start = Instant::now();

        for frame_index in 0u32..300 {
            let dep = if frame_index % 30 == 0 {
                FrameDependency::KeyFrame
            } else {
                FrameDependency::Delta
            };

            let fake_data = vec![
                0u8;
                if dep == FrameDependency::KeyFrame {
                    50_000
                } else {
                    5_000
                }
            ];

            match video
                .send(
                    EncodedFrame::new(
                        dep,
                        Bytes::from(fake_data),
                        frame_interval * frame_index,
                        start + frame_interval * frame_index,
                    )
                    .with_duration(frame_interval),
                )
                .await
            {
                Ok(id) => {
                    if frame_index % 30 == 0 {
                        println!("Sent keyframe {}", id);
                    }
                }
                Err(e) => {
                    eprintln!("Send error: {}", e);
                }
            }

            let target = start + frame_interval * (frame_index + 1);
            if let Some(delay) = target.checked_duration_since(Instant::now()) {
                tokio::time::sleep(delay).await;
            }
        }
    }

    println!("Done streaming. Shutting down.");
    session.shutdown().await?;
    event_task.await?;
    conn.stop(&app).await?;
    conn.close().await?;
    Ok(())
}
