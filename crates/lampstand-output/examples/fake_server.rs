//! A fake Lampstand server for trying lampstand-output by hand.
//!
//!     cargo run -p lampstand-output --example fake_server
//!     cargo run -p lampstand-output -- --windowed --display 0
//!
//! Plays a scripted show (black, a lyric, two deck slides, black, logo,
//! clear, the lyric again), one step every 2 s, looping. Keys pressed in an
//! output window are printed; `next` and `prev` also step the script.
//!
//! Options: --port N (default 7420), --manual (step only on next/prev),
//! --slide PATH (serve this PNG as the first deck slide), --interval MS.

#[path = "../tests/support/fake_server.rs"]
mod fake_server;

use std::time::Duration;

use fake_server::{script, FakeServer};
use lampstand_show::protocol::{ClientMessage, Command};

#[tokio::main]
async fn main() {
    let mut port = 7420;
    let mut manual = false;
    let mut slide = None;
    let mut interval = 2000;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--port" => port = args.next().and_then(|v| v.parse().ok()).expect("--port N"),
            "--manual" => manual = true,
            "--interval" => interval = args.next().and_then(|v| v.parse().ok()).expect("--interval MS"),
            "--slide" => slide = Some(std::fs::read(args.next().expect("--slide PATH")).expect("read slide PNG")),
            _ => return eprintln!("usage: fake_server [--port N] [--manual] [--interval MS] [--slide PATH]"),
        }
    }
    let server = FakeServer::start(&format!("127.0.0.1:{port}"), slide).await.expect("listen");
    println!("fake server on {}", server.ws_url());

    let steps = script();
    let mut step = 0usize;
    let mut revision = 0;
    let mut seen = 0;
    let mut since = Duration::ZERO;
    let tick = Duration::from_millis(50);
    let show = |step: usize, revision: &mut u64| {
        let (what, mut snapshot) = steps[step].clone();
        *revision += 1;
        snapshot.revision = *revision;
        println!("step {}: {what}", step + 1);
        server.set(snapshot);
    };
    show(step, &mut revision);
    loop {
        tokio::time::sleep(tick).await;
        since += tick;
        let received: Vec<_> = server.received.lock().unwrap()[seen..].to_vec();
        seen += received.len();
        let mut moved = false;
        for message in received {
            println!("client: {}", serde_json::to_string(&message).unwrap());
            match message {
                ClientMessage::Command { command: Command::Next | Command::GoLive } => {
                    step = (step + 1) % steps.len();
                    moved = true;
                }
                ClientMessage::Command { command: Command::Prev } => {
                    step = (step + steps.len() - 1) % steps.len();
                    moved = true;
                }
                _ => {}
            }
        }
        if !manual && since >= Duration::from_millis(interval) {
            step = (step + 1) % steps.len();
            moved = true;
        }
        if moved {
            since = Duration::ZERO;
            show(step, &mut revision);
        }
    }
}
