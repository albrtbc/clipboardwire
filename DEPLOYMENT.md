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
| Wayland clipboard fix | `core/Cargo.toml` | **Now upstream** (since 0.5.4), no longer a fork change. It enables arboard's `wayland-data-control` so the desktop client sees clipboard changes on wlroots compositors (Hyprland). |
| Reconnect-loop fix | `core/src/client/transport.rs`, `core/src/client/mod.rs` | After a tray settings save the old transport kept reconnecting forever (1/2/4/8… s). It now stops when its handle is dropped, and cancelling the supervisor aborts it. |

---

## Server (self-hosted hub in Docker)

The hub is stateless. With an external TLS cert it has nothing to persist.

### 1. Image

CI publishes to GHCR on every push to `main` and on `v*` tags:

- `ghcr.io/PLACEHOLDER_ORG/clipboardwire:<version>` — e.g. `:0.5.4`
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

Build from this checkout (`cargo build --release -p clipboardwire`, binary at
`target/release/clipboardwire`). On Arch/Omarchy this is the way: the prebuilt
`clipboardwire-linux-x86_64` on the releases pages links `libxdo.so.3`, and
Arch ships `libxdo.so.4`, so the prebuilt binary won't start. Other distros
can use the prebuilt binary from the
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
  `wayland-data-control` backend (upstream enables it since 0.5.4; the fork
  used to). Without it, on Hyprland/wlroots a background poller only sees the
  X11 (XWayland) clipboard and misses copies from native Wayland apps. The
  server binary is unaffected (it never touches the clipboard; no libwayland
  link dependency).
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
git fetch upstream && git merge --no-ff upstream/main             # on a branch, not on main
```

Most fork changes are new files (`server-bin/`, this doc, the other workflows)
plus a one-line edit (the `Cargo.toml` workspace member), which merge cleanly.
But the fork replaces four files that upstream also has: `Dockerfile`,
`docker-compose.yml`, `.dockerignore` and `.github/workflows/docker.yml`. They
conflict whenever upstream edits them. The image version is read from
`Cargo.toml`, so pulling an upstream version bump republishes the image at the
new version automatically.

Conflict rules:

- `Dockerfile`, `docker-compose.yml` and `.github/workflows/docker.yml`: keep
  the fork's side (`git checkout --ours`), because they build and run the
  headless hub image that the server uses.
- `.dockerignore`: the union of both lists.
- `CHANGELOG.md`: keep both, upstream's new sections plus the fork's
  `-fork.N` sections, newest first. Never take upstream's file with
  `--theirs`: it would delete the fork sections.
- Everything else: upstream's version wins.
- `.env.example` is upstream's, for its Traefik setup (`CLIPBOARDWIRE_PASSWORD`
  in `.env`). The fork's `docker-compose.yml` ignores it and reads
  `CLIPBOARDWIRE_PASSWORD_FILE`; it is left untouched to avoid conflicts.

Rules for a sync and a release:

- **Land the sync with a merge commit, never squash or rebase-merge.** A
  squash drops the upstream history, and every later sync would conflict
  again. Run `git merge --no-ff upstream/main` on a branch, open a PR, and
  land it with `gh pr merge --merge`.
- **Push only `main` and the one fork tag.** Never `git push --tags` or
  `--follow-tags`: this clone holds upstream's release tags (`v0.1.0`
  onwards), and pushing them would run the release and image workflows on
  upstream's trees. Push the tag by its full ref name:
  `git push origin refs/tags/v<upstream>-fork.N`.
- **Fork releases are tagged `v<upstream>-fork.N`**, for example
  `v0.5.4-fork.1`, with the `Cargo.toml` version left at upstream's. Add a
  `## [<upstream>-fork.N]` section to `CHANGELOG.md` first: `release.yml`
  takes the release notes from it. release-please is manual-only
  (`workflow_dispatch`) and does not tag.
- **A CI-built APK can't upgrade an installed one.** Every CI run signs the
  debug APK with a new throwaway key, and Android refuses an update signed
  with a different key. Build locally instead (`./gradlew assembleDebug` in
  `android/`) with a kept debug keystore (the machine's `~/.android/debug.keystore` by
  default, or under `$ANDROID_USER_HOME`; keep it backed up); builds signed
  with the same key then upgrade in place. Switching from a CI build to a local one
  needs one uninstall, which clears the app's settings.
