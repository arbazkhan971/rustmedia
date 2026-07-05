//! Losslessly remux one container into another (e.g. MOV → MP4).
//!
//! ```bash
//! cargo run -p rustmedia --example remux -- input.mov output.mp4
//! ```

fn main() -> rustmedia::Result<()> {
    let mut args = std::env::args().skip(1);
    let (Some(input), Some(output)) = (args.next(), args.next()) else {
        eprintln!("usage: remux <input> <output>");
        std::process::exit(2);
    };

    let stats = rustmedia::ops::remux(&input, &output)?;
    println!(
        "remuxed {input} → {output}: {} tracks, {} packets, {} bytes",
        stats.tracks, stats.packets, stats.bytes
    );
    Ok(())
}
