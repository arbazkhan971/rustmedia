//! Stream a file's packets and print a one-line summary of each.
//!
//! Demonstrates the pull-based demux API: nothing is decoded, packets arrive in
//! file order, and each carries timing plus the keyframe flag.
//!
//! ```bash
//! cargo run -p rustmedia --example packets -- movie.mp4
//! ```

use rustmedia::Media;

fn main() -> rustmedia::Result<()> {
    let path = std::env::args().nth(1).unwrap_or_else(|| {
        eprintln!("usage: packets <file>");
        std::process::exit(2);
    });

    let mut media = Media::open(&path)?;
    let mut count = 0u64;
    let mut bytes = 0u64;

    for packet in media.packets() {
        let packet = packet?;
        count += 1;
        bytes += packet.data.len() as u64;
        if count <= 10 {
            println!(
                "track {} · pts {:?} · {} bytes{}",
                packet.track_id,
                packet.pts,
                packet.data.len(),
                if packet.is_keyframe {
                    " · keyframe"
                } else {
                    ""
                },
            );
        }
    }

    println!("… {count} packets, {bytes} bytes total");
    Ok(())
}
