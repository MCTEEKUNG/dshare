# Dshare

KVM-style device sharing between machines (Windows ↔ Linux).
Mouse, keyboard, clipboard, audio — over TCP, written in Rust.

## Status

End-to-end mouse + keyboard works: capture from a Windows server,
forward over TCP, inject into a Linux client through `uinput`.

Still TODO: edge crossing (currently a hotkey toggle), Linux capture,
clipboard wiring, audio.

## Layout

```
crates/
  dshare-protocol/   wire format (bincode + length prefix), message types
  dshare-core/       config (TOML), screen layout + edge-cross detection,
                     framed peer session
  dshare-input/      InputCapture / InputInject traits, Win + Linux backends
  dshare-clipboard/  arboard-based polling watcher + apply
  dshare-gui/        eframe/egui config GUI: General / Layout / Status tabs
  dshare/            binary: gui | server | client subcommands
```

## Build

```bash
cargo check --workspace
cargo run -p dshare -- gui
```

Run the daemon with explicit role:

```bash
# on Windows (cursor source) — listens on 0.0.0.0:24800 by default
cargo run -p dshare -- server

# on Ubuntu (cursor sink) — needs server_addr in config
cargo run -p dshare -- client
```

Config lives at:
- Windows: `%APPDATA%\dshare\config.toml`
- Linux: `~/.config/dshare/config.toml`

Minimal Linux client config (`~/.config/dshare/config.toml`):

```toml
role = "client"
bind_addr = "0.0.0.0:24800"
server_addr = "<windows-host-ip>:24800"
clipboard_sync = true

[layout.server_screen]
width = 1920
height = 1080
```

## Using it

1. Start `dshare client` on Ubuntu — it connects, exchanges Hello, then idles.
2. Start `dshare server` on Windows — accepts the client.
3. Press **Ctrl+Alt+Shift+G** on Windows to take over: mouse and keyboard
   are now forwarded to Ubuntu and swallowed locally.
4. Press the same hotkey again to release.

The server log prints `dshare grab ON` / `OFF` on each toggle.

## Linux setup (uinput permissions)

Inject needs write access to `/dev/uinput`. Either run as root, or:

```bash
sudo tee /etc/udev/rules.d/99-dshare-uinput.rules <<'EOF'
KERNEL=="uinput", GROUP="input", MODE="0660"
EOF
sudo udevadm control --reload-rules
sudo udevadm trigger
sudo usermod -aG input $USER   # log out / back in to apply
```

Then smoke-test the inject backend without networking:

```bash
cargo run -p dshare -- test-inject
```

It waits 2 s (focus a text field), nudges the cursor right, clicks once,
and types `hi<Enter>` via the virtual device.

## Keycode convention

The wire `Message::KeyEvent.keycode` is a Linux evdev key code (u16, see
`<linux/input-event-codes.h>`). The Windows capture backend translates
Win32 VK → evdev before sending; the Linux inject backend writes it
straight to uinput.

## Roadmap

1. ~~Fill in `LinuxInject` (uinput virtual device)~~ ✅
2. ~~Fill in `WinCapture` (low-level hooks + message pump)~~ ✅
3. ~~Keycode normalization (Win VK → evdev)~~ ✅
4. ~~Wire capture → session → inject end to end (hotkey toggle)~~ ✅
5. Edge crossing instead of hotkey (cursor jail + virtual position tracking)
6. Fill in `LinuxCapture` (evdev grab on `/dev/input/event*`)
7. Wire clipboard sync into `handle_peer`
8. TLS via `rustls` for the framed stream
9. Audio streaming crate (`cpal` capture + Opus + UDP/RTP)
```
