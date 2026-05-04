//! `dshare` entry point. Three subcommands:
//!   gui     — launch the configuration GUI
//!   server  — run as the input source (the machine whose mouse/kbd is shared)
//!   client  — run as a sink, receiving events
//!
//! For now `server`/`client` are skeleton loops that connect, exchange Hello,
//! and pump the clipboard. Input forwarding is wired up once the OS-specific
//! capture/inject is filled in (see TODOs in dshare-input).

use clap::{Parser, Subcommand};
use dshare_core::config::{Config, Role};
use dshare_protocol::{
    codec::MessageCodec, Hello, KeyModifiers, Message, MouseButton, ScreenInfo, PROTOCOL_VERSION,
};
use futures::SinkExt;
use std::path::PathBuf;
use std::time::Duration;
use tokio::net::{TcpListener, TcpStream};
use tokio_util::codec::Framed;
use tracing::{info, warn};
use uuid::Uuid;

#[derive(Parser)]
#[command(name = "dshare", version)]
struct Cli {
    #[arg(long)]
    config: Option<PathBuf>,
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    Gui,
    Server,
    Client,
    /// Drive the local inject backend with a canned sequence (smoke test).
    /// On Linux this exercises the uinput virtual device.
    TestInject,
    /// Watch local input for a few seconds and print each event.
    /// Does NOT grab — events still reach other applications normally.
    TestCapture {
        /// Seconds to observe before exiting.
        #[arg(long, default_value_t = 10)]
        seconds: u64,
        /// After installing hooks, drive the inject backend to round-trip
        /// a few synthetic events through the OS. Verifies the whole pipe
        /// without needing human interaction.
        #[arg(long)]
        self_test: bool,
    },
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,dshare=debug")),
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        Cmd::Gui => dshare_gui::run(),
        Cmd::Server => run_async(|cfg| run_server(cfg), cli.config),
        Cmd::Client => run_async(|cfg| run_client(cfg), cli.config),
        Cmd::TestInject => {
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(test_inject())
        }
        Cmd::TestCapture { seconds, self_test } => {
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(test_capture(seconds, self_test))
        }
    }
}

/// Observe local input for `seconds` without blocking it. Useful to confirm
/// the capture backend wires up. On Windows this exercises the low-level
/// hook chain; on Linux it would go through evdev (not yet implemented).
async fn test_capture(seconds: u64, self_test: bool) -> anyhow::Result<()> {
    let mut cap = dshare_input::default_capture()?;
    cap.set_grabbed(false);

    let (tx, mut rx) = tokio::sync::mpsc::channel::<Message>(256);
    let cap_task = tokio::spawn(async move {
        if let Err(e) = cap.run(tx).await {
            warn!("capture exited: {e}");
        }
    });

    if self_test {
        // Drive a balanced sequence: cursor returns to start, no net change.
        let inject_task = tokio::spawn(async {
            tokio::time::sleep(Duration::from_millis(300)).await;
            let mut inj = match dshare_input::default_inject() {
                Ok(i) => i,
                Err(e) => {
                    warn!("inject backend init failed: {e}");
                    return;
                }
            };
            for _ in 0..3 {
                let _ = inj.handle(&Message::MouseMove { dx: 2, dy: 0 }).await;
                tokio::time::sleep(Duration::from_millis(50)).await;
                let _ = inj.handle(&Message::MouseMove { dx: -2, dy: 0 }).await;
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        });
        // Don't await it — let the capture loop see events as they fire.
        drop(inject_task);
        info!("self-test: injecting 6 mouse moves through SendInput");
    } else {
        info!(
            "observing input for {seconds}s (events not blocked) — press keys / move mouse"
        );
    }

    let deadline = std::time::Instant::now() + Duration::from_secs(seconds);
    let mut count = 0usize;
    while std::time::Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_millis(500), rx.recv()).await {
            Ok(Some(msg)) => {
                count += 1;
                info!("event #{count}: {msg:?}");
            }
            Ok(None) => break,
            Err(_) => continue,
        }
    }

    info!("test-capture done, {count} events seen");
    cap_task.abort();
    Ok(())
}

/// Canned input sequence: nudge cursor right, click, type three letters.
/// Useful to confirm uinput permissions and the inject path without networking.
async fn test_inject() -> anyhow::Result<()> {
    let mut inj = dshare_input::default_inject()?;
    info!("inject backend ready, sending test sequence in 2s — focus a text field");
    tokio::time::sleep(Duration::from_secs(2)).await;

    for _ in 0..50 {
        inj.handle(&Message::MouseMove { dx: 4, dy: 0 }).await?;
        tokio::time::sleep(Duration::from_millis(8)).await;
    }
    inj.handle(&Message::MouseButton {
        button: MouseButton::Left,
        pressed: true,
    })
    .await?;
    inj.handle(&Message::MouseButton {
        button: MouseButton::Left,
        pressed: false,
    })
    .await?;

    // Linux evdev codes: KEY_H=35, KEY_I=23, KEY_ENTER=28
    for code in [35u32, 23, 28] {
        inj.handle(&Message::KeyEvent {
            keycode: code,
            pressed: true,
            modifiers: KeyModifiers::default(),
        })
        .await?;
        tokio::time::sleep(Duration::from_millis(20)).await;
        inj.handle(&Message::KeyEvent {
            keycode: code,
            pressed: false,
            modifiers: KeyModifiers::default(),
        })
        .await?;
        tokio::time::sleep(Duration::from_millis(40)).await;
    }
    info!("test sequence complete");
    Ok(())
}

fn run_async<F, Fut>(f: F, config: Option<PathBuf>) -> anyhow::Result<()>
where
    F: FnOnce(Config) -> Fut,
    Fut: std::future::Future<Output = anyhow::Result<()>>,
{
    let path = config.unwrap_or_else(Config::default_path);
    let cfg = Config::load(&path).unwrap_or_else(|_| {
        info!("no config at {}, using defaults", path.display());
        Config::default()
    });
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(f(cfg))
}

async fn run_server(cfg: Config) -> anyhow::Result<()> {
    if cfg.role != Role::Server {
        warn!("config role is {:?}, but invoked as server", cfg.role);
    }
    let listener = TcpListener::bind(&cfg.bind_addr).await?;
    info!("server listening on {}", cfg.bind_addr);

    loop {
        let (stream, addr) = listener.accept().await?;
        info!("peer connected: {addr}");
        tokio::spawn(handle_peer(stream, cfg.clone(), true));
    }
}

async fn run_client(cfg: Config) -> anyhow::Result<()> {
    let server = cfg
        .server_addr
        .clone()
        .ok_or_else(|| anyhow::anyhow!("client mode requires server_addr in config"))?;
    info!("connecting to {server}");
    let stream = TcpStream::connect(&server).await?;
    handle_peer(stream, cfg, false).await
}

async fn handle_peer(stream: TcpStream, cfg: Config, is_server: bool) -> anyhow::Result<()> {
    let mut framed = Framed::new(stream, MessageCodec);

    let hello = Hello {
        protocol_version: PROTOCOL_VERSION,
        peer_id: Uuid::new_v4(),
        hostname: hostname(),
        screen: ScreenInfo {
            width: cfg.layout.server_screen.width,
            height: cfg.layout.server_screen.height,
        },
    };
    framed.send(Message::Hello(hello)).await?;
    info!("hello sent (role={})", if is_server { "server" } else { "client" });

    // TODO: handshake → spawn capture (server) / inject (client) → spawn
    // clipboard watcher → run main loop forwarding messages between
    // local sources and the framed stream.
    Ok(())
}

fn hostname() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "unknown".into())
}
