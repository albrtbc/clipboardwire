# Self-hosting clipboardwire (fork notes)

This fork adds a **containerized, self-hostable hub** and documents a
**Wayland/Hyprland desktop client** setup. It tracks upstream
[`davefx/clipboardwire`](https://github.com/davefx/clipboardwire); only the
items below are fork-specific. For protocol/architecture details see
`PROTOCOL.md` and `ARCHITECTURE.md`.

> Replace every `PLACEHOLDER` (hostnames, usernames, paths, registry org)
> with your own values. **Never commit real passwords, certificates, private
> keys, internal hostnames or IPs** — `.dockerignore` already excludes
> `*.crt`, `*.key`, `*.pem` and `cw_pass.txt` from the image build context,
> and those files live outside the repo.

---

## What this fork adds

| Addition | File(s) | Why |
| --- | --- | --- |
| Headless hub binary | `server-bin/` | A `clipboardwire-server` binary that depends only on `clipboardwire-core` (no tray/GTK/X11), so it runs in a minimal container. |
| Docker image | `Dockerfile` | Multi-stage build → `distroless/cc` runtime (~30–50 MB, no shell). |
| Compose template | `docker-compose.yml` | Hub wired to an external (mkcert) cert + a password Docker secret. |
| Image CI | `.github/workflows/docker.yml` | Builds + pushes to GHCR, versioned from `Cargo.toml`. |
| Wayland clipboard fix | `core/Cargo.toml` | Enables arboard's `wayland-data-control` so the desktop client sees clipboard changes on wlroots compositors (Hyprland). |

---

## Server (self-hosted hub in Docker)

The hub is stateless. With an external TLS cert it has nothing to persist.

### 1. Image

CI publishes to GHCR on every push to `main` and on `v*` tags:

- `ghcr.io/PLACEHOLDER_ORG/clipboardwire:<version>` — e.g. `:0.4.6`
- `ghcr.io/PLACEHOLDER_ORG/clipboardwire:<version>-<sha>` — **immutable**, pin this
- `ghcr.io/PLACEHOLDER_ORG/clipboardwire:latest`

GHCR packages are **private by default**: either make the package public in
GitHub, or `docker login ghcr.io` on the server with a PAT that has
`read:packages`.

### 2. TLS with your own cert

The hub serves `wss://` directly when pointed at a cert + key
(`CLIPBOARDWIRE_TLS_CERT_FILE` / `CLIPBOARDWIRE_TLS_KEY_FILE`); it then skips
its own self-signed generation. Any PEM cert works, including
[`mkcert`](https://github.com/FiloSottile/mkcert):

```sh
# SANs MUST cover the host/IP that clients put in their `server =` URL.
mkcert -cert-file cw.crt -key-file cw.key PLACEHOLDER_HOST PLACEHOLDER_IP
```

Alternatively, terminate TLS at a reverse proxy (nginx/Caddy/Traefik) on 443
and run the hub with `CLIPBOARDWIRE_TLS_DISABLE=true` behind it — the proxy
must forward WebSocket upgrade headers. (In that setup the client `server =`
URL has no port, e.g. `wss://PLACEHOLDER_HOST/sync`.)

### 3. Run

See `docker-compose.yml`. Put the hub password in `cw_pass.txt` (one line),
the cert/key next to the compose file, then:

```sh
docker compose pull && docker compose up -d
docker compose logs        # expect: "listening" + "hub started"
```

Open the hub port (default `8484/tcp`, or `443` if fronted by a proxy) in the
firewall. Verify: `curl -sk https://PLACEHOLDER_HOST/healthz` → `ok`.

### Server env vars (full list in `ARCHITECTURE.md` §2.4)

| Var | Note |
| --- | --- |
| `CLIPBOARDWIRE_USER` | Basic-auth username (required) |
| `CLIPBOARDWIRE_PASSWORD_FILE` | Path to a file with the password (Docker-secret friendly) |
| `CLIPBOARDWIRE_BIND` | default `0.0.0.0:8484` |
| `CLIPBOARDWIRE_TLS_CERT_FILE` / `_KEY_FILE` | serve `wss://` with these |
| `CLIPBOARDWIRE_TLS_DISABLE` | plain `ws://` (only behind a TLS proxy) |

---

## Desktop client on Linux / Wayland (Hyprland, Omarchy)

### 1. Binary

Either build from this checkout (`cargo build --release -p clipboardwire`,
binary at `target/release/clipboardwire`) or download the upstream prebuilt
`clipboardwire-linux-x86_64` from
[upstream releases](https://github.com/davefx/clipboardwire/releases/latest).

Runtime deps on Arch/CachyOS (the tray links libxdo):

```sh
sudo pacman -S --needed gtk3 libayatana-appindicator xdotool
```

### 2. Trust the CA — clipboardwire ignores the OS trust store

The client trusts the bundled Mozilla roots **plus** whatever you put in
`tls_ca_file`. Running `mkcert -install` on the client does **not** help.
Copy your mkcert root CA (find it with `mkcert -CAROOT`, file `rootCA.pem`)
onto the client and point `tls_ca_file` at it with an **absolute** path (the
client does not expand `~`).

### 3. Config

`~/.config/clipboardwire/config.toml` (must not be world-readable → `chmod 600`):

```toml
server      = "wss://PLACEHOLDER_HOST/sync"   # add :PORT if not behind a 443 proxy
user        = "PLACEHOLDER_USER"
password    = "PLACEHOLDER_PASSWORD"
poll_ms     = 300
tls_ca_file = "/home/PLACEHOLDER_USER/.config/clipboardwire/rootCA.crt"
```

The client must resolve `PLACEHOLDER_HOST` (LAN DNS / Pi-hole / `/etc/hosts`).

### 4. Autostart as a systemd user service (tray + sync in one process)

`~/.config/systemd/user/clipboardwire.service`:

```ini
[Unit]
Description=clipboardwire clipboard sync (tray + client)
After=graphical-session.target
PartOf=graphical-session.target

[Service]
Type=simple
ExecStart=%h/.local/bin/clipboardwire
Restart=on-failure
RestartSec=3

[Install]
WantedBy=graphical-session.target
```

```sh
systemctl --user daemon-reload
systemctl --user enable --now clipboardwire.service
journalctl --user -u clipboardwire -f        # expect "tray icon shown" + "connected"
```

The systemd user manager must have the session env (`WAYLAND_DISPLAY`,
`DBUS_SESSION_BUS_ADDRESS`); on Omarchy/uwsm it does. The tray icon appears in
Waybar's `tray` module (Omarchy hides it behind a tray-expander group).

### Notes / gotchas

- **Wayland clipboard:** the desktop client needs arboard's
  `wayland-data-control` backend (this fork enables it). Without it, on
  Hyprland/wlroots a background poller only sees the X11 (XWayland) clipboard
  and misses copies from native Wayland apps. The server binary is unaffected
  (it never touches the clipboard; no libwayland link dependency).
- **Single instance only:** a singleton lock means one client per machine.
  Don't launch `clipboardwire` by hand while the service runs. Edit config via
  the tray menu or `clipboardwire settings` (a pure GUI, no connection, no
  lock conflict), then `systemctl --user restart clipboardwire`.
- **`tls_insecure = true`** skips cert verification entirely. Trusted LAN/VPN
  testing only — prefer `tls_ca_file`.

---

## Versioning / staying in sync with upstream

```sh
git remote add upstream https://github.com/davefx/clipboardwire   # once
git fetch upstream && git merge upstream/main                     # pull updates
```

Fork-specific changes are almost all new files (`server-bin/`, `Dockerfile`,
`docker-compose.yml`, this doc, the docker workflow) plus two one-line edits
(`Cargo.toml` workspace member, `core/Cargo.toml` arboard feature), so merges
rarely conflict. The image version is read from `Cargo.toml`, so pulling an
upstream version bump republishes the image at the new version automatically.
```
