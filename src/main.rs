mod gpt;
mod log;
mod mbr;
mod partition;
mod size;

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::process;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use colored::Colorize;

use gpt::{GptDisk, GptPartitionEntry, MAX_PARTITION_ENTRIES};
use partition::{guid_to_string, new_guid, PartitionType};
use size::{format_size, parse_size, SECTOR_SIZE};

const VERSION: &str = env!("CARGO_PKG_VERSION");

// ─── CLI ─────────────────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(
    name = "gptool",
    version = VERSION,
    about = "GPT disk image utility",
    disable_help_subcommand = true,
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Create a new empty GPT disk image
    Create {
        /// Path to the image file to create
        image: String,
        /// Disk size (e.g. 32G, 256M, 1T)
        size: String,
    },

    /// Create a new partition
    Mkpart {
        /// Path to the image file
        image: String,
        /// Partition size (e.g. 512M, 4G)
        size: String,
        /// Partition type: efi, microsoft, linux, bios, swap
        #[arg(name = "type", value_name = "TYPE")]
        partition_type: String,
        /// Partition name (UTF-16, max 36 chars)
        name: String,
    },

    /// List all partitions in the image
    List {
        /// Path to the image file
        image: String,
    },

    /// Remove a partition by index
    Rmpart {
        /// Path to the image file
        image: String,
        /// Partition index (1-based, as shown by `list`)
        index: u32,
    },

    /// Edit partition properties
    Setpart {
        /// Path to the image file
        image: String,
        /// Partition index (1-based, as shown by `list`)
        index: u32,
        /// New partition name
        #[arg(long)]
        name: Option<String>,
        /// New partition size (e.g. 512M)
        #[arg(long)]
        size: Option<String>,
        /// New partition type: efi, microsoft, linux, bios, swap
        #[arg(long = "type", id = "ptype")]
        partition_type: Option<String>,
    },

    /// Dump partition data to a file
    Read {
        /// Path to the image file
        image: String,
        /// Partition index (1-based)
        index: u32,
        /// Output file path
        output: String,
    },

    /// Write data to a partition from a file
    Write {
        /// Path to the image file
        image: String,
        /// Partition index (1-based)
        index: u32,
        /// Input file path
        input: String,
    },

    /// Print version information
    Version,

    /// Print version and list of commands
    Help,
}

// ─── Entry point ─────────────────────────────────────────────────────────────

fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Create { image, size } => cmd_create(&image, &size),
        Commands::Mkpart {
            image,
            size,
            partition_type,
            name,
        } => cmd_mkpart(&image, &size, &partition_type, &name),
        Commands::List { image } => cmd_list(&image),
        Commands::Rmpart { image, index } => cmd_rmpart(&image, index),
        Commands::Setpart {
            image,
            index,
            name,
            size,
            partition_type,
        } => cmd_setpart(&image, index, name, size, partition_type),
        Commands::Read {
            image,
            index,
            output,
        } => cmd_read(&image, index, &output),
        Commands::Write {
            image,
            index,
            input,
        } => cmd_write(&image, index, &input),
        Commands::Version => {
            print_version();
            Ok(())
        }
        Commands::Help => {
            print_help();
            Ok(())
        }
    };

    if let Err(e) = result {
        log::error(&format!("{e:#}"));
        process::exit(1);
    }
}

// ─── Version / Help ──────────────────────────────────────────────────────────

fn print_version() {
    println!(
        "{} {}",
        "gptool".bold(),
        format!("v{VERSION}").dimmed()
    );
}

fn print_help() {
    print_version();
    println!();
    println!("{}", "COMMANDS:".bold().underline());
    let cmds: &[(&str, &str)] = &[
        (
            "create  <image> <size>",
            "Create a new empty GPT disk image",
        ),
        (
            "mkpart  <image> <size> <type> <name>",
            "Create a new partition",
        ),
        ("list    <image>", "List all partitions"),
        ("rmpart  <image> <index>", "Remove a partition by index"),
        (
            "setpart <image> <index> [--name=..] [--size=..] [--type=..]",
            "Edit partition",
        ),
        (
            "read    <image> <index> <output>",
            "Dump partition data to file",
        ),
        (
            "write   <image> <index> <input>",
            "Write data to a partition",
        ),
        ("version", "Print version"),
        ("help", "Print this help"),
    ];
    for (cmd, desc) in cmds {
        println!("    {:<55} {}", cmd.green(), desc);
    }
    println!();
    println!("{}", "PARTITION TYPES:".bold().underline());
    for (key, desc) in PartitionType::all_types() {
        println!("    {:<14} {}", key.cyan(), desc);
    }
    println!();
    println!("{}", "SIZE FORMAT:".bold().underline());
    println!("    Accepts: 512, 1K, 32M, 4G, 1T  (binary units, aligned to 512-byte sectors)");
}

// ─── create ──────────────────────────────────────────────────────────────────

fn cmd_create(image: &str, size_str: &str) -> Result<()> {
    let total_bytes = parse_size(size_str)?;
    let total_sectors = total_bytes / SECTOR_SIZE;

    // Minimum GPT: MBR + primary header + 32 entry sectors + 1 usable + 32 backup entry sectors + backup header = 68
    if total_sectors < 68 {
        bail!(
            "disk image too small ({} sectors); minimum is 68 sectors (34K)",
            total_sectors
        );
    }

    log::status("Creating", &format!("{image} ({})  [{total_sectors} sectors]", format_size(total_bytes)));

    let mut file = File::create(image).context("creating image file")?;
    file.set_len(total_bytes).context("setting image size")?;

    // Protective MBR
    mbr::write_protective_mbr(&mut file, total_sectors)?;
    log::status("Writing", "protective MBR with boot sector");

    // GPT
    let disk = GptDisk::new(total_sectors);
    disk.write_to(&mut file)?;
    log::status("Writing", "primary GPT header + partition table");
    log::status("Writing", "backup GPT header + partition table");

    let usable = disk.header.last_usable_lba - disk.header.first_usable_lba + 1;
    log::status(
        "Finished",
        &format!(
            "{image}  {} usable ({})",
            format_size(usable * SECTOR_SIZE),
            format!("{usable} sectors").dimmed()
        ),
    );
    Ok(())
}

// ─── mkpart ──────────────────────────────────────────────────────────────────

fn cmd_mkpart(image: &str, size_str: &str, type_str: &str, name: &str) -> Result<()> {
    let part_type = PartitionType::from_name(type_str)
        .ok_or_else(|| anyhow::anyhow!("unknown partition type '{type_str}'. valid: efi, microsoft, linux, bios, swap"))?;

    let part_bytes = parse_size(size_str)?;
    let part_sectors = part_bytes / SECTOR_SIZE;

    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(image)
        .context("opening image")?;

    let mut disk = GptDisk::read_from(&mut file)?;

    // Find an empty entry slot
    let slot = disk
        .find_empty_entry_index()
        .ok_or_else(|| anyhow::anyhow!("no empty partition entry slots (max {MAX_PARTITION_ENTRIES})"))?;

    // Find free space
    let start_lba = disk
        .find_free_space(part_sectors)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "not enough free space for {} partition",
                format_size(part_bytes)
            )
        })?;

    let last_lba = start_lba + part_sectors - 1;

    let mut entry = GptPartitionEntry::empty();
    entry.type_guid = part_type.guid_bytes();
    entry.unique_guid = new_guid();
    entry.first_lba = start_lba;
    entry.last_lba = last_lba;
    entry.set_name(name);

    disk.entries[slot] = entry;
    disk.write_to(&mut file)?;

    log::status(
        "Adding",
        &format!(
            "partition {} \"{}\" ({}, {})",
            slot + 1,
            name.bold(),
            part_type.display_name(),
            format_size(part_bytes),
        ),
    );
    log::status(
        "Placed",
        &format!(
            "LBA {}..{} ({})",
            start_lba,
            last_lba,
            format!("{part_sectors} sectors").dimmed()
        ),
    );
    Ok(())
}

// ─── list ────────────────────────────────────────────────────────────────────

fn cmd_list(image: &str) -> Result<()> {
    let mut file = File::open(image).context("opening image")?;
    let disk = GptDisk::read_from(&mut file)?;

    let total_bytes = file.metadata()?.len();
    println!();
    log::info(
        "Disk",
        &format!(
            "{}  {}  ({})",
            image.bold(),
            format_size(total_bytes),
            format!("GUID {}", guid_to_string(&disk.header.disk_guid)).dimmed()
        ),
    );
    println!();

    // Collect non-empty partitions with their real indices
    let parts: Vec<(usize, &GptPartitionEntry)> = disk
        .entries
        .iter()
        .enumerate()
        .filter(|(_, e)| !e.is_empty())
        .collect();

    if parts.is_empty() {
        log::info("", "  No partitions.");
        println!();
        return Ok(());
    }

    // Header
    println!(
        "    {:<6} {:<22} {:>10}   {}",
        "#".bold(),
        "Type".bold(),
        "Size".bold(),
        "Name".bold(),
    );
    println!("    {}", "─".repeat(60));

    for (idx, entry) in &parts {
        let type_name = PartitionType::from_guid(&entry.type_guid)
            .map(|t| t.display_name().to_string())
            .unwrap_or_else(|| guid_to_string(&entry.type_guid));

        println!(
            "    {:<6} {:<22} {:>10}   {}",
            format!("{}", idx + 1).cyan().bold(),
            type_name.yellow(),
            format_size(entry.size_in_bytes()).green(),
            entry.name_string().white().bold(),
        );
    }
    println!();
    Ok(())
}

// ─── rmpart ──────────────────────────────────────────────────────────────────

fn cmd_rmpart(image: &str, index: u32) -> Result<()> {
    if index == 0 {
        bail!("partition index is 1-based");
    }
    let slot = (index - 1) as usize;

    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(image)
        .context("opening image")?;

    let mut disk = GptDisk::read_from(&mut file)?;

    if slot >= disk.entries.len() || disk.entries[slot].is_empty() {
        bail!("partition {index} does not exist");
    }

    let old_name = disk.entries[slot].name_string();
    let old_type = PartitionType::from_guid(&disk.entries[slot].type_guid)
        .map(|t| t.display_name().to_string())
        .unwrap_or_else(|| "Unknown".into());
    let old_size = format_size(disk.entries[slot].size_in_bytes());

    disk.entries[slot] = GptPartitionEntry::empty();
    disk.write_to(&mut file)?;

    log::status(
        "Removing",
        &format!(
            "partition {index} \"{}\" ({}, {old_size})",
            old_name.bold(),
            old_type,
        ),
    );
    Ok(())
}

// ─── setpart ─────────────────────────────────────────────────────────────────

fn cmd_setpart(
    image: &str,
    index: u32,
    new_name: Option<String>,
    new_size: Option<String>,
    new_type: Option<String>,
) -> Result<()> {
    if index == 0 {
        bail!("partition index is 1-based");
    }
    let slot = (index - 1) as usize;

    if new_name.is_none() && new_size.is_none() && new_type.is_none() {
        bail!("no changes specified. use --name, --size, and/or --type");
    }

    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(image)
        .context("opening image")?;

    let mut disk = GptDisk::read_from(&mut file)?;

    if slot >= disk.entries.len() || disk.entries[slot].is_empty() {
        bail!("partition {index} does not exist");
    }

    if let Some(name) = &new_name {
        disk.entries[slot].set_name(name);
        log::status("Setting", &format!("partition {index} name → \"{}\"", name.bold()));
    }

    if let Some(type_str) = &new_type {
        let pt = PartitionType::from_name(type_str)
            .ok_or_else(|| anyhow::anyhow!("unknown partition type '{type_str}'"))?;
        disk.entries[slot].type_guid = pt.guid_bytes();
        log::status("Setting", &format!("partition {index} type → {}", pt.display_name()));
    }

    if let Some(size_str) = &new_size {
        let new_bytes = parse_size(size_str)?;
        let new_sectors = new_bytes / SECTOR_SIZE;
        let new_last = disk.entries[slot].first_lba + new_sectors - 1;

        if new_sectors > disk.entries[slot].size_in_sectors() {
            // Growing — check for overlap
            if !disk.can_extend_partition(slot, new_last) {
                bail!(
                    "cannot grow partition {index} to {} — would overlap or exceed disk",
                    format_size(new_bytes)
                );
            }
        }

        disk.entries[slot].last_lba = new_last;
        log::status("Setting", &format!("partition {index} size → {}", format_size(new_bytes)));
    }

    disk.write_to(&mut file)?;
    log::status("Updated", &format!("partition {index}"));
    Ok(())
}

// ─── read (dump partition) ───────────────────────────────────────────────────

fn cmd_read(image: &str, index: u32, output: &str) -> Result<()> {
    if index == 0 {
        bail!("partition index is 1-based");
    }
    let slot = (index - 1) as usize;

    let mut file = File::open(image).context("opening image")?;
    let disk = GptDisk::read_from(&mut file)?;

    if slot >= disk.entries.len() || disk.entries[slot].is_empty() {
        bail!("partition {index} does not exist");
    }

    let entry = &disk.entries[slot];
    let part_bytes = entry.size_in_bytes();

    log::status(
        "Reading",
        &format!(
            "partition {index} \"{}\" ({}) → {output}",
            entry.name_string().bold(),
            format_size(part_bytes),
        ),
    );

    file.seek(SeekFrom::Start(entry.first_lba * SECTOR_SIZE))?;
    let mut out = File::create(output).context("creating output file")?;

    copy_sectors(&mut file, &mut out, entry.size_in_sectors())?;

    log::status("Finished", &format!("wrote {} to {output}", format_size(part_bytes)));
    Ok(())
}

// ─── write (flash partition) ─────────────────────────────────────────────────

fn cmd_write(image: &str, index: u32, input: &str) -> Result<()> {
    if index == 0 {
        bail!("partition index is 1-based");
    }
    let slot = (index - 1) as usize;

    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(image)
        .context("opening image")?;

    let disk = GptDisk::read_from(&mut file)?;

    if slot >= disk.entries.len() || disk.entries[slot].is_empty() {
        bail!("partition {index} does not exist");
    }

    let entry = &disk.entries[slot];
    let part_bytes = entry.size_in_bytes();
    let part_sectors = entry.size_in_sectors();

    let mut input_file = File::open(input).context("opening input file")?;
    let input_len = input_file.metadata()?.len();

    log::status(
        "Writing",
        &format!(
            "{input} ({}) → partition {index} \"{}\" ({})",
            format_size(input_len),
            entry.name_string().bold(),
            format_size(part_bytes),
        ),
    );

    if input_len > part_bytes {
        log::warning(&format!(
            "input ({}) is larger than partition ({}); data will be cropped",
            format_size(input_len),
            format_size(part_bytes),
        ));
    }

    file.seek(SeekFrom::Start(entry.first_lba * SECTOR_SIZE))?;

    // Write input data (capped to partition size)
    let sectors_to_write = std::cmp::min(
        input_len.div_ceil(SECTOR_SIZE),
        part_sectors,
    );

    let mut buf = [0u8; 512];
    let mut written_sectors = 0u64;
    for _ in 0..sectors_to_write {
        buf.fill(0);
        let n = input_file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        file.write_all(&buf)?;
        written_sectors += 1;
    }

    // Zero-fill remaining sectors
    let remaining = part_sectors - written_sectors;
    if remaining > 0 {
        buf.fill(0);
        for _ in 0..remaining {
            file.write_all(&buf)?;
        }
        if input_len < part_bytes {
            log::info(
                "Zeroing",
                &format!(
                    "{} remaining sectors",
                    remaining,
                ),
            );
        }
    }

    file.flush()?;
    log::status("Finished", &format!("wrote partition {index}"));
    Ok(())
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn copy_sectors(src: &mut File, dst: &mut File, count: u64) -> Result<()> {
    let mut buf = [0u8; 512];
    for _ in 0..count {
        src.read_exact(&mut buf)?;
        dst.write_all(&buf)?;
    }
    dst.flush()?;
    Ok(())
}
