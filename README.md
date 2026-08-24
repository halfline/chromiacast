# chromiacast

`chromiacast` is a codec-agnostic, sender-side implementation of Cast
Streaming in Rust. It connects to a Cast receiver, authenticates the device,
launches the mirroring application, negotiates audio and video streams, and
sends already-encoded access units over encrypted Cast RTP.

The crate deliberately does not depend on PipeWire, GStreamer, GNOME, or an
encoder. Those belong in adapters and applications built on top of this
protocol boundary.

## Status

The library currently provides:

- Cast V2 framing over TLS and Cast device authentication;
- receiver status, app availability, launch, stop, and unsolicited lifecycle
  events;
- request-correlated authenticated `GET_DEVICE_INFO` and optional
  `eureka_info` product metadata, accepting both deployed `DEVICE_INFO`
  responses and receivers that echo `GET_DEVICE_INFO`;
- a bounded HTTPS setup-endpoint query for deployed receiver manufacturer and
  product metadata;
- heartbeat replies and timeout detection;
- OFFER/ANSWER negotiation for H.264, VP8, VP9, HEVC, AV1, Opus, and AAC
  configurations;
- frame encryption and Cast RTP packetization;
- RTCP Sender Reports, receiver reports, ACKs, NACKs, retransmission, and
  picture-loss handling;
- simultaneous audio and video sender streams;
- observable sender failures, receiver timeouts, and flow-control statistics;
- a pluggable datagram `Transport`; and
- optional mDNS discovery that retains stable device IDs and every advertised
  scoped endpoint.

Bitrate selection and congestion control remain application policy. Capture,
encoding, media-pipeline integration, and desktop transaction management are
also intentionally outside this crate.

## Security

`CastConnection::connect` and `CastConnection::connect_address` authenticate
the receiver after TLS. Cast receivers use self-signed TLS certificates, so
authentication is performed by the Cast device-auth challenge: the response's
certificate chain is checked against the bundled Cast trust roots and its
signature is bound to the live TLS certificate and a fresh nonce. Revocation
data is validated when the receiver supplies it.

`connect_unverified_for_testing` and
`connect_address_unverified_for_testing` exist only for protocol test doubles
such as shanocast. They are hidden from generated documentation and only
compiled when the explicitly named `dangerous-unverified` feature is enabled.
They make no claim about the peer's identity and must not be used in production
software.

Authenticated product queries are available only on a verified
`CastConnection`. The control task uses the protocol's distinct `requestId`
and `request_id` spellings, requires the exact platform route and namespace,
bounds every returned string, and distinguishes an unsupported or omitted
`eureka_info` reply from a malformed successful reply. Applications remain
responsible for matching the returned device ID to the device the user
selected; the library does not treat presentation strings as authority.

See [SECURITY.md](SECURITY.md) for the security model and reporting guidance.

## Encoded-frame contract

Each `EncodedFrame` represents exactly one complete encoded access unit:

- `media_timestamp` is monotonic on that stream and uses the shared media
  timeline supplied by the producer;
- `reference_time` is the corresponding local monotonic-clock observation,
  used for Sender Reports and latency measurements;
- `duration` should be supplied when known so callers can measure queued media
  time;
- the first frame is a key frame, and a delta frame references the immediately
  preceding frame;
- reordered frames and B-frames are not supported initially;
- H.264 access units use Annex B byte-stream format and include parameter sets
  where needed for independent decoding; and
- an Opus access unit contains one complete Opus packet, with its duration
  represented by `duration`.

The RTP timestamp is derived from `media_timestamp`, not from the thread's
arrival time. Audio and video produced under one media clock can therefore be
submitted from different tasks without losing their shared timeline.

Applications should watch `SenderEvent::NeedsKeyFrame`, react to terminal
events, and use `SenderSession::statistics` or `StatisticsUpdated` events to
throttle the encoder before `send` reaches the maximum frame-ID span.

`max_acceptable_in_flight_duration` is an advisory encoder budget. It keeps a
66 ms floor, tracks roughly two measured network round trips, and reserves most
of the receiver's playout window by capping the sender at one third of that
window. The library reports this pressure but does not change encoder bitrate
or drop already-accepted frames on the application's behalf.

Until the first frame acknowledgement arrives, a video producer should still
send occasional key-frame probes even when that advisory budget is exhausted.
This lets a receiver recover when its media path becomes ready after the first
key frame was sent. The red-screen example demonstrates this startup policy.

## Examples

Discover the first receiver, authenticate it, and display decodable solid-red
H.264 video:

```sh
cargo run --features discovery --example red_screen
```

Press Ctrl-C to stop the mirroring application and close both sender sessions
cleanly. The example is intentionally small, but it demonstrates key-frame
requests, a media timeline, session negotiation, and deliberate teardown.

For packetization and load diagnostics with deliberately non-decodable data:

```sh
cargo run --example packet_transport_stress -- RECEIVER_ADDRESS
```

An IP address uses port 8009. For an IPv6 link-local receiver, pass a scoped
socket address such as `[fe80::1234%3]:8009`.

## Testing

Run the in-process protocol and mock-transport coverage with:

```sh
cargo test --all-features
```

An ignored end-to-end test starts shanocast in Podman, encodes a red H.264
image, sends it through the complete stack, captures the receiver window, and
checks the rendered center pixel:

```sh
cargo test --all-features --test pixel_verify -- --ignored --nocapture
```

That test requires Podman, Xvfb, and ImageMagick's `import` and `convert`
commands. The shanocast container image may be downloaded on its first run.

## Diagnostics

Control-message routes, message types, request/sequence IDs, and RTCP feedback
shapes are instrumented through the `tracing` crate. Encoded media, encryption
keys, certificates, and complete control payloads are not logged. Applications
can install their preferred `tracing` subscriber when collecting an
interoperability trace from a production receiver.

When discovery selects an IPv6 link-local receiver route, pass its complete
scoped `SocketAddr` to `SenderSession::start_address()` and bind the transport
to an IPv6 local address. `SenderSession::start()` remains the convenience API
for callers that already have a routable `IpAddr`.

## Protocol references and provenance

The implementation is original Rust code informed by the published
[Open Screen Cast Streaming protocol](https://chromium.googlesource.com/openscreen/+/refs/heads/main/cast/protocol/streaming_session_protocol.md),
[Cast channel schema](https://chromium.googlesource.com/openscreen/+/refs/heads/main/cast/common/channel/proto/cast_channel.proto),
and [sender authentication reference](https://chromium.googlesource.com/openscreen/+/refs/heads/main/cast/sender/channel/cast_auth_util.cc).
It does not embed Open Screen's GN/C++ runtime. The bundled Cast trust anchors
were sourced from Open Screen; their attribution and license are recorded in
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

## License

The original chromiacast code is available under the MIT License. See
[LICENSE](LICENSE) and [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
