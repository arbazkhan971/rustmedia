//! Print a summary of a media file.
//!
//! ```bash
//! cargo run -p rustmedia --example inspect -- movie.mp4
//! ```

use rustmedia::{format_duration, Media};

fn main() -> rustmedia::Result<()> {
    let path = std::env::args().nth(1).unwrap_or_else(|| {
        eprintln!("usage: inspect <file>");
        std::process::exit(2);
    });

    let media = Media::open(&path)?;
    println!("format:   {}", media.format());
    if let Some(d) = media.duration() {
        println!("duration: {}", format_duration(d));
    }

    for track in media.tracks() {
        print!("  #{} {} · {}", track.id, track.media_type, track.codec);
        if let Some(v) = track.video() {
            print!(" · {}x{}", v.width, v.height);
            if let Some(fps) = v.fps() {
                print!(" @ {fps:.3} fps");
            }
        }
        if let Some(a) = track.audio() {
            print!(" · {} Hz · {} ch", a.sample_rate, a.channels);
        }
        println!();
    }

    for (key, value) in media.metadata().iter() {
        println!("  meta: {key} = {value}");
    }
    Ok(())
}
