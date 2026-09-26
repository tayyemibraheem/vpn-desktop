# TayyemVPN Desktop (Windows)

A native Windows desktop app (React UI inside a Tauri/Rust shell — not a website, not Electron)
for connecting to the Tayyem VPN, with split tunneling (per-app and per-destination) and
in-place auto-updates.

## Status: WireGuard fully embedded, not yet verified on real Windows hardware

v0.1.x connected via OpenVPN (removed entirely) and went through several real releases tested on
Windows. v0.2.0 switches to WireGuard, and drives it directly via the official
[wireguard-nt](https://git.zx2c4.com/wireguard-nt/about/) embeddable driver library — bundled as
`resources/wireguard-nt/amd64/wireguard.dll` — rather than shelling out to a separately-installed
WireGuard client. There is nothing else to install; the app creates and configures its own network
adapter. This connect/disconnect path is brand new and **has not yet run on a real Windows
machine**.

| Piece | Confidence | Why |
|---|---|---|
| Login against `tayyem_platform` | High | Plain HTTP JSON, same as every other client on the platform — unchanged from v0.1.x |
| React UI | High | Built and verified with `npm run build` from this repo |
| Destination-based split tunneling (`route`) | High | Unchanged from v0.1.x, already verified working on Windows |
| WireGuard keypair generation + device registration | Medium | Standard `x25519-dalek` keygen + a plain HTTPS call to `vpn_manager`, but never run end-to-end |
| Interface IP/DNS/default-route setup (PowerShell) | Medium-high | Standard `New-NetIPAddress`/`Set-DnsClientServerAddress`/`New-NetRoute` cmdlets, same pattern already verified in `destination_routes.rs` |
| Auto-updater | Medium | Tauri's official plugin, already verified working in v0.1.x releases |
| Embedded WireGuard driver (wireguard-nt via `wireguard_nt.rs`) | **Low — needs real testing** | Raw FFI into the official `wireguard.dll`'s C ABI — the `WgInterface`/`WgPeer`/`WgAllowedIp` struct layouts were hand-derived from wireguard-nt's public header with no Windows machine to verify the result against. Each struct has a compile-time size assertion recording the exact byte count that derivation produced; if `WireGuardSetConfiguration` fails outright or a tunnel comes up but never handshakes, this is the first file to re-check against a fresh `wireguard.h`. |
| Per-app split tunneling (WinDivert) | **Low — needs real testing** | Raw FFI into `WinDivert.dll`'s C ABI with hand-written struct byte offsets (`src-tauri/src/split_tunnel/windivert.rs`). Unchanged from v0.1.x and still unverified on real hardware. If it's wrong, everything else in the app still works — destination-based split tunneling and the VPN connection don't depend on it. |

**First thing to test on a real Windows machine:** just install the app and sign in — no other
downloads needed. Watch the in-app connection log; device registration, adapter creation, and
interface configuration each log their own step, so a failure points straight at which one broke.

## Requirements

- Windows 10/11
- [Rust + Cargo](https://rustup.rs)
- [Node.js](https://nodejs.org) 18+
- [Tauri CLI prerequisites](https://tauri.app/start/prerequisites/) (WebView2 — usually already
  present on Windows 11; Visual Studio Build Tools with the "Desktop development with C++"
  workload)
- Nothing extra — WireGuard itself is bundled (`resources/wireguard-nt/amd64/wireguard.dll`, the
  official embeddable driver-loading library from wireguard.com/wireguard-nt) and driven directly
  from the app; there is no separate WireGuard client to install
- (Optional, for per-app split tunneling) [WinDivert](https://github.com/basil00/WinDivert) — see
  `resources/windivert/README.md`

## Develop

```
npm install
npm run tauri dev
```

## Build an installer

```
npm run tauri build
```

Produces an NSIS installer under `src-tauri/target/release/bundle/nsis/`. The app requests
Administrator elevation on launch (`src-tauri/app.manifest`) — split tunneling needs to edit the
routing table and (for per-app rules) open a WinDivert handle, both of which require it.

## CI builds — you never have to build this locally

**GitHub Actions** (`.github/workflows/build.yml`) on `github.com/tayyemibraheem/vpn-desktop` is
the live CI — every push to `main` builds on a real Windows VM, and a tag push cuts a signed
release. (`azure-pipelines.yml` is a leftover from before that repo existed and isn't used.)

1. **Every push builds automatically** and Actions will tell you (green check / red X) if it
   compiles — including the parts that can't be verified from this dev environment. This is the
   fastest way to find out whether the Rust code (especially `windivert.rs` or the new
   `wireguard.rs`) actually compiles.

2. **To cut a real, installable, auto-updating release:**
   - Generate a signing keypair once: `npx @tauri-apps/cli signer generate -w ~/.tauri/tayyem-vpn.key`
   - Put the printed **public** key into `src-tauri/tauri.conf.json`'s `plugins.updater.pubkey`
     (replacing the `REPLACE_ME_...` placeholder), commit, push.
   - In the GitHub repo's Settings → Secrets and variables → Actions, add two secrets:
     `TAURI_SIGNING_PRIVATE_KEY` (the full contents of the private key file) and
     `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` (the password you set when generating it).
   - Push a tag: `git tag app-v0.1.0 && git push origin app-v0.1.0`
   - Actions builds, signs, and publishes a **draft** GitHub Release with the installer and the
     `latest.json` the auto-updater checks against. Review it, hit "Publish release" — from then
     on, anyone running an older version gets offered the update automatically.

Until you've done the signing-key step, plain pushes to `main` still build (proving the code
compiles) but don't produce a signed release, and "Check for updates" in the app will fail to
find anything — that's expected, not a bug.

## What's real vs. placeholder

- **Real**: login/entitlement check (same rules as every other Tayyem client — completed
  account, verified email, `vpn-access` grant), destination-based split tunneling, settings
  persistence, the whole UI.
- **Experimental**: the embedded WireGuard connect/disconnect path and per-app split tunneling
  (see table above) — both are raw FFI into a vendor DLL, unverified on real hardware.
- **Placeholder only**: the "File Server" sidebar entry is a stub screen with no backend —
  added because it was asked for, not because anything exists to back it yet.
- **Not wired up**: public IP / location / ISP display on the home screen (shown as a note in
  the UI rather than faked with a fabricated value).
