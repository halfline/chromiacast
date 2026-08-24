# Security policy

## Receiver authentication

Production callers must create control connections with
`CastConnection::connect` or `CastConnection::connect_address`. These methods
complete the Cast device-authentication exchange before returning, including
certificate-chain, nonce, TLS-certificate binding, signature, and optional
revocation checks.

The explicitly named `connect_unverified_for_testing` variants are provided
for receiver emulators that do not implement device authentication. They are
only compiled by the `dangerous-unverified` Cargo feature. They accept any peer
identity and therefore permit an on-path attacker on the local network to
impersonate a receiver. Do not expose them as a production fallback or select
them from ordinary user configuration.

The library authenticates the receiver; it does not authorize which local
users may discover, connect to, or cast to a device. Applications remain
responsible for local-user policy, consent, and protecting encoded media
before it reaches this crate.

`CastConnection::get_setup_device_info` is deliberately outside that
authentication boundary. It queries a self-signed HTTPS service at the same
selected receiver address and returns bounded presentation metadata. Callers
must not treat those manufacturer, model, product, or SSDP strings as a
credential or authorization decision.

## Supported versions

Security fixes are applied to the current development branch. No stable
release series exists yet.

## Reporting a vulnerability

Please report suspected vulnerabilities privately through the repository's
[GitHub security advisory form](https://github.com/halfline/chromiacast/security/advisories/new).
Include the affected revision, a description of the impact, and reproduction
details when possible. Avoid filing a public issue until a fix is available.
