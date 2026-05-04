# Dshare

KVM-style device sharing between machines (Windows ↔ Linux).
Mouse, keyboard, clipboard, audio — over TCP, written in Rust.

## Status

Scaffolding — protocol, layout engine, GUI shell, and daemon plumbing
are in place. OS-specific input capture/inject and audio streaming are
the next milestones (see TODOs in `dshare-input/src/{windows,linux}.rs`).

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
# on Windows (cursor source)
cargo run -p dshare -- server

# on Ubuntu (cursor sink)
cargo run -p dshare -- client
```

Config lives at:
- Windows: `%APPDATA%\dshare\config.toml`
- Linux: `~/.config/dshare/config.toml`

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
2. Fill in `WinCapture` (low-level hooks + message pump on dedicated thread)
3. Fill in `LinuxCapture` (evdev grab on `/dev/input/event*`)
4. Wire capture → session → inject end to end with edge crossing
5. Keycode normalization table (Win VK → evdev)
6. TLS via `rustls` for the framed stream
7. Audio streaming crate (`cpal` capture + Opus + UDP/RTP)
```
