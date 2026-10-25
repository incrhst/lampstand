//! A manual check of the Audio role's player on a real sound output.
//!
//! ```sh
//! cargo run -p lampstand-media --example play_check -- path/to/music.mp3 [output name]
//! cargo run -p lampstand-media --example play_check -- --list
//! ```
//!
//! It plays the file through the same `Player` Lampstand uses, in this order,
//! and prints what it does with the time of each step so a recording can be
//! lined up with it: play for 1.5 s, pause for 0.7 s, resume for 1.5 s, fade
//! out over 1 s; then play again for 0.6 s and cut it with the panic stop.
//! You should hear the music, a gap, the music again, a smooth fade, a short
//! burst and then silence at once, with no clicks.

use std::time::{Duration, Instant};

use lampstand_media::{default_output, load_sound, output_devices, Player};

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(first) = args.next() else {
        eprintln!("usage: play_check <file> [output name] | --list");
        std::process::exit(2);
    };
    if first == "--list" {
        println!("default: {:?}", default_output());
        for name in output_devices() {
            println!("output: {name}");
        }
        return;
    }
    let output = args.next();
    let pcm = match load_sound(std::path::Path::new(&first)) {
        Ok(pcm) => pcm,
        Err(e) => {
            eprintln!("cannot read {first}: {e}");
            std::process::exit(1);
        }
    };
    println!(
        "file: {first}: {} Hz, {} channel(s), {:.2} s",
        pcm.rate,
        pcm.channels,
        pcm.duration()
    );
    let player = Player::start(output.clone());
    let begun = Instant::now();
    let at = || begun.elapsed().as_secs_f64();
    let wait_open = Instant::now();
    while !player.output_open() && wait_open.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(20));
    }
    println!(
        "{:.3} output {} ({:?})",
        at(),
        if player.output_open() {
            "open"
        } else {
            "NOT open"
        },
        output
    );
    if !player.output_open() {
        std::process::exit(1);
    }
    let sleep = |s: f64| std::thread::sleep(Duration::from_secs_f64(s));

    println!("{:.3} play", at());
    player.play(pcm.clone(), 0.0);
    sleep(1.5);
    println!(
        "{:.3} pause (position {:.2} s)",
        at(),
        player.playing().map_or(-1.0, |p| p.position)
    );
    player.pause();
    sleep(0.7);
    println!(
        "{:.3} resume (position {:.2} s)",
        at(),
        player.playing().map_or(-1.0, |p| p.position)
    );
    player.resume();
    sleep(1.5);
    println!("{:.3} stop with a 1 s fade", at());
    player.stop(1.0);
    let fade = Instant::now();
    while player.playing().is_some() && fade.elapsed() < Duration::from_secs(5) {
        sleep(0.02);
    }
    println!(
        "{:.3} ended after {:.2} s of fade",
        at(),
        fade.elapsed().as_secs_f64()
    );
    sleep(0.4);

    println!("{:.3} play again", at());
    player.play(pcm, 0.0);
    sleep(0.6);
    println!("{:.3} panic", at());
    player.panic();
    let cut = Instant::now();
    while player.playing().is_some() && cut.elapsed() < Duration::from_secs(2) {
        sleep(0.002);
    }
    println!(
        "{:.3} silent after {:.3} s",
        at(),
        cut.elapsed().as_secs_f64()
    );
    sleep(0.5);
    println!("{:.3} done", at());
}
