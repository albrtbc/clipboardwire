# Security policy

## Threat model

clipboardwire is built for the **α threat model**: clipboard sync between
trusted devices over a LAN or VPN. Specifically:

- **Trusted:** the user's devices, the hub host (its operator, RAM, disk).
- **Untrusted:** the network path between the devices and the hub. TLS
  via `rustls` is what protects clipboard contents in transit.
- **Out of scope:** denial-of-service, side channels on the client
  devices, end-to-end encryption from one client to another (the hub
  sees clipboard plaintext).

This is intentionally **not** a tool for syncing clipboards over the
public internet to a server you don't control. The right upgrade path
for that scenario is end-to-end encryption with TOFU device pairing,
sketched in `PROTOCOL.md` §4.

### TLS posture

Since v0.3.1 the hub auto-generates a self-signed cert on first run
(`<state_dir>/self-signed.{crt,key}`) with sensible SANs and logs its
SHA-256 fingerprint. Self-signed certs **encrypt** the wire but do
**not** authenticate the hub to clients. Clients must either:

- pin the cert via `tls_ca_file = "/path/to/self-signed.crt"`
  (recommended; works without any external CA), or
- skip cert verification with `tls_insecure = true` (only safe on a
  fully trusted network — your LAN, your VPN), or
- run with `tls_disabled = true` on the hub so the wire is plain
  `ws://` (loses confidentiality; only acceptable on a network you
  fully trust).

For a stronger posture, bring your own cert: point `tls_cert_file`
and `tls_key_file` at a cert signed by a CA the clients already
trust (e.g., a small internal CA whose root the clients ship).

## Reporting a vulnerability

Please **do not** open a public GitHub issue for a security report.

Email <davefx@gmail.com> with `[clipboardwire-security]` in the subject.
Include:

- the version (`clipboardwire --version`),
- a minimal reproduction or a description of the failure mode,
- whether you've already disclosed the issue anywhere else.

I'll acknowledge within 7 days and aim to ship a fix within 30. For
issues that have a clear scope and a clear fix, expect faster. I'll
coordinate a CVE if appropriate.

Out-of-scope reports I'll close without a fix:

- "Clipboard contents are visible to a process running as my user on
  the same machine." Yes — arboard talks to the OS clipboard, and the
  OS lets local processes read it. clipboardwire is not a sandbox.
- "The hub can read clipboard contents." Yes — see the threat model.
  E2EE is the upgrade path; PRs welcome.
- DoS or resource-exhaustion findings against a hub exposed directly
  to the public internet. clipboardwire is not built for that
  deployment shape.

## Supported versions

Only the latest minor version receives security fixes.

| Version | Supported |
| ------- | --------- |
| 0.3.x   | ✅        |
| ≤ 0.2.x | ❌        |
