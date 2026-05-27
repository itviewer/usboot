//! USB boot tool for Amlogic G12 SoCs.

use clap::Parser;
use std::fs;
use std::path::Path;
use std::process::exit;
use std::time::Duration;
use tokio::time::sleep;
use usboot::amlogic::{AmlogicSoC, SocId};
use usboot::common::*;

#[derive(Parser, Debug)]
#[command(
    version,
    name = "boot-g12",
    about = "Load U-Boot binary onto an Amlogic G12 SoC over USB in boot mode"
)]
struct Args {
    /// Binary to load
    binary: String,

    /// Timeout in seconds for the device to enumerate.
    #[arg(long, value_parser = parse_timeout, default_value = "5")]
    timeout: Duration,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();

    let args = Args::parse();

    let bpath = Path::new(&args.binary);
    if !bpath.exists() {
        eprintln!("Error: Binary file not found: {:?}", bpath);
        exit(1);
    }
    println!("Waiting for device to enumerate...");

    let dev = AmlogicSoC::with_defaults(args.timeout).await?;

    let socid_str = dev.identify().await?;
    let socid = SocId::new(&socid_str);

    println!("Firmware Version :");
    println!(
        "ROM: {}.{} Stage: {}.{}",
        socid.major(),
        socid.minor(),
        socid.stage_major(),
        socid.stage_minor(),
    );
    println!(
        "Need Password: {} Password OK: {}",
        socid.need_password() as u8,
        socid.password_ok() as u8,
    );

    let data = fs::read(&bpath)?;
    let load_addr: u32 = 0xfffa_0000;

    let first_chunk_end = data.len().min(0x10000);
    println!("Writing {} at 0x{:08x}...", bpath.display(), load_addr);
    dev.write_large_memory(load_addr, &data[..first_chunk_end], 4096, false)
        .await?;
    println!("[DONE]");

    println!("Running at 0x{:08x}...", load_addr);
    dev.run(load_addr, true).await?;
    println!("[DONE]");

    sleep(Duration::from_secs(1)).await;

    let mut seq: u8 = 0;
    let mut prev_length: u32 = u32::MAX;
    let mut prev_offset: u32 = u32::MAX;

    loop {
        let (length, offset) = dev.get_boot_amlc().await?;

        if length == prev_length && offset == prev_offset {
            println!("[BL2 END]");
            break;
        }

        prev_length = length;
        prev_offset = offset;

        println!(
            "AMLC dataSize={}, offset={}, seq={}...",
            length, offset, seq
        );

        let start = offset as usize;
        let end = (offset as usize + length as usize).min(data.len());
        dev.write_amlc_data(seq, start, &data[start..end]).await?;
        println!("[DONE]");

        seq = seq.wrapping_add(1);
    }

    Ok(())
}