//! USB boot tool for Amlogic GX/AXG SoCs.
//!
//! This tool loads U-Boot and optional device files onto Amlogic GX/AXG family SoCs

use anyhow::Context;
use clap::Parser;
use log::warn;
use rust_embed::Embed;
use std::path::{Path, PathBuf};
use std::process::exit;
use std::time::Duration;
use std::{fmt, fs};
use tokio::time::sleep;
use usboot::amlogic::{AmlogicSoC, SocId};
use usboot::common::*;

#[derive(Embed)]
#[folder = "assets/amlogic/"]
struct Asset;

/// GX family boards (GXBB/GXL/GXM)
const GX_BOARDS: &[&str] = &[
    "libretech-s905x-cc",
    "libretech-s805x-ac",
    "khadas-vim",
    "khadas-vim2",
    "odroid-c2",
    "nanopi-k2",
    "p212",
    "p230",
    "p231",
    "q200",
    "q201",
    "p281",
    "p241",
    "libretech-s912-pc",
];

/// AXG family boards
const AXG_BOARDS: &[&str] = &["s400", "s420", "apollo"];

#[derive(Parser, Debug)]
#[command(
    version,
    name = "boot-gx",
    about = "Load U-Boot binary onto an Amlogic GX/AXG SoC over USB in boot mode"
)]
struct Args {
    /// Board type to boot on
    board: String,

    /// Path to board-specific files directory
    #[arg(long, default_value = ".")]
    board_files: PathBuf,

    /// Image file to load
    #[arg(long)]
    image: Option<PathBuf>,

    /// Device tree binary file to load
    #[arg(long)]
    fdt: Option<PathBuf>,

    /// U-Boot script file to load
    #[arg(long)]
    script: Option<PathBuf>,

    /// RamFS/initramfs file to load
    #[arg(long)]
    ramfs: Option<PathBuf>,

    /// Timeout in seconds for the device to enumerate
    #[arg(long, value_parser = parse_timeout, default_value = "5")]
    timeout: Duration,
}

/// Boot parameters for different SoC families
struct BootParams {
    ddr_load: u32,
    bl2_params: u32,
    uboot_load: u32,
}

impl BootParams {
    fn for_board(board: &str) -> anyhow::Result<Self, String> {
        if GX_BOARDS.contains(&board) {
            let params = BootParams {
                ddr_load: 0xd9000000,
                bl2_params: 0xd900c000,
                uboot_load: 0x200c000,
            };

            println!("Using GX Family {}", params);
            Ok(params)
        } else if AXG_BOARDS.contains(&board) {
            let params = BootParams {
                ddr_load: 0xfffc0000,
                bl2_params: 0xfffcc000,
                uboot_load: 0x200c000,
            };
            println!("Using AXG Family {}", params);
            Ok(params)
        } else {
            Err(format!("Unsupported board: {}", board))
        }
    }
}

impl fmt::Display for BootParams {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "BootParams {{ ddr_load: 0x{:08x}, bl2_params: 0x{:08x}, uboot_load: 0x{:08x} }}",
            self.ddr_load,
            self.bl2_params,
            self.uboot_load
        )
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();

    let args = Args::parse();

    // Validate board
    let params = BootParams::for_board(&args.board).map_err(|e| {
        eprintln!("Error: {}", e);
        exit(1);
    }).unwrap();

    // Determine directory paths
    let board_dir = args.board_files;

    // Verify required files exist
    let bl2_file = board_dir.join("u-boot.bin.usb.bl2");
    let tpl_file = board_dir.join("u-boot.bin.usb.tpl");
    let ddr_file = board_dir.join("usbbl2runpara_ddrinit.bin");
    let fip_file = board_dir.join("usbbl2runpara_runfipimg.bin");

    if !bl2_file.exists() {
        eprintln!("Error: BL2 file not found: {:?}", bl2_file);
        exit(1);
    }
    if !tpl_file.exists() {
        eprintln!("Error: TPL file not found: {:?}", tpl_file);
        exit(1);
    }
    if !ddr_file.exists() {
        warn!("DDR init file not found: {:?}, using built-in file instead", ddr_file);
    }
    if !fip_file.exists() {
        warn!("FIP run file not found: {:?}, using built-in file instead", fip_file);
    }

    println!("Waiting for device to enumerate...");

    let dev = AmlogicSoC::with_defaults(args.timeout).await?;

    // Identify SoC
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

    // Load DDR initialization
    load_uboot(&dev, &params, &bl2_file, &ddr_file, &fip_file, &tpl_file).await?;

    // Load optional files
    if let Some(image_path) = args.image {
        if image_path.exists() {
            write_file(&dev, &image_path, 0x8080000, Some(512), true).await?;
        }
    }

    if let Some(fdt_path) = args.fdt {
        if fdt_path.exists() {
            write_file(&dev, &fdt_path, 0x8008000, None, false).await?;
        }
    }

    if let Some(script_path) = args.script {
        if script_path.exists() {
            write_file(&dev, &script_path, 0x8000000, None, false).await?;
        }
    }

    if let Some(ramfs_path) = args.ramfs {
        if ramfs_path.exists() {
            write_file(&dev, &ramfs_path, 0x13000000, Some(512), true).await?;
        }
    }

    // Run U-Boot
    run_uboot(&dev, &params, &socid).await?;

    Ok(())
}

async fn load_uboot(
    dev: &AmlogicSoC,
    params: &BootParams,
    bl2_file: &Path,
    ddr_file: &Path,
    fip_file: &Path,
    tpl_file: &Path,
) -> anyhow::Result<()> {
    // Initialize DDR
    init_ddr(dev, params, bl2_file, ddr_file).await?;

    // Load U-Boot files
    write_file(dev, bl2_file, params.ddr_load, Some(64), false).await?;
    write_file(dev, fip_file, params.bl2_params, Some(48), false).await?;
    write_file(dev, tpl_file, params.uboot_load, Some(64), true).await?;

    Ok(())
}

async fn init_ddr(
    dev: &AmlogicSoC,
    params: &BootParams,
    bl2_file: &Path,
    ddr_file: &Path,
) -> anyhow::Result<()> {
    // Load and run initial BL2
    write_file(dev, bl2_file, params.ddr_load, None, false).await?;
    write_file(dev, ddr_file, params.bl2_params, Some(32), false).await?;
    println!("Load and run initial BL2");
    dev.run(params.ddr_load, false).await?;
    println!("[DONE]");

    wait_ms(1000).await;

    // Check if we need to run BL2 params again
    // let socid_str = dev.identify().await?;
    // let socid = SocId::new(&socid_str);
    //
    // if socid.stage_minor() == 8 {
    //     dev.run(params.bl2_params, false).await?;
    //     println!("[DONE]");
    //     wait_ms(1000).await;
    // }

    Ok(())
}

async fn write_file(
    dev: &AmlogicSoC,
    path: &Path,
    addr: u32,
    block_size: Option<usize>,
    fill: bool,
) -> anyhow::Result<()> {
    let filename = path.file_name().unwrap().to_str().unwrap();
    println!("Writing {} at 0x{:08x}...", filename, addr);
    let data = if path.exists() {
        fs::read(path)?
    } else {
        let embedded_file = Asset::get(filename).context(format!("Embedded file not found {:?}", filename))?;
        embedded_file.data.to_vec()
    };

    match block_size {
        Some(size) => {
            dev.write_large_memory(addr, &data, size, fill).await?;
        }
        None => {
            dev.write_memory(addr, &data).await?;
        }
    }

    println!("[DONE]");
    Ok(())
}

async fn run_uboot(dev: &AmlogicSoC, params: &BootParams, socid: &SocId) -> anyhow::Result<()> {
    println!("Running U-Boot...");
    let addr = if socid.stage_minor() == 8 {
        params.bl2_params
    } else {
        params.ddr_load
    };
    dev.run(addr, false).await?;
    println!("[DONE]");
    Ok(())
}

async fn wait_ms(ms: u64) {
    println!("Waiting...");
    sleep(Duration::from_millis(ms)).await;
    println!("[DONE]");
}
