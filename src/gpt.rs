use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};

use anyhow::{bail, Context, Result};

use crate::partition::new_guid;
use crate::size::SECTOR_SIZE;

const GPT_SIGNATURE: &[u8; 8] = b"EFI PART";
const GPT_REVISION: u32 = 0x0001_0000;
const GPT_HEADER_SIZE: u32 = 92;
pub const PARTITION_ENTRY_SIZE: u32 = 128;
pub const MAX_PARTITION_ENTRIES: u32 = 128;
/// 128 entries × 128 bytes / 512 bytes per sector = 32 sectors
pub const PARTITION_ARRAY_SECTORS: u64 =
    (MAX_PARTITION_ENTRIES as u64 * PARTITION_ENTRY_SIZE as u64) / SECTOR_SIZE;

// ─── GPT Header ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct GptHeader {
    pub my_lba: u64,
    pub alternate_lba: u64,
    pub first_usable_lba: u64,
    pub last_usable_lba: u64,
    pub disk_guid: [u8; 16],
    pub partition_entry_lba: u64,
    pub num_partition_entries: u32,
    pub partition_entry_size: u32,
    pub partition_entries_crc32: u32,
}

impl GptHeader {
    pub fn serialize(&self) -> [u8; 92] {
        let mut buf = [0u8; 92];
        buf[0..8].copy_from_slice(GPT_SIGNATURE);
        buf[8..12].copy_from_slice(&GPT_REVISION.to_le_bytes());
        buf[12..16].copy_from_slice(&GPT_HEADER_SIZE.to_le_bytes());
        // header_crc32 at [16..20] — zeroed for computation
        // reserved at [20..24] — zero
        buf[24..32].copy_from_slice(&self.my_lba.to_le_bytes());
        buf[32..40].copy_from_slice(&self.alternate_lba.to_le_bytes());
        buf[40..48].copy_from_slice(&self.first_usable_lba.to_le_bytes());
        buf[48..56].copy_from_slice(&self.last_usable_lba.to_le_bytes());
        buf[56..72].copy_from_slice(&self.disk_guid);
        buf[72..80].copy_from_slice(&self.partition_entry_lba.to_le_bytes());
        buf[80..84].copy_from_slice(&self.num_partition_entries.to_le_bytes());
        buf[84..88].copy_from_slice(&self.partition_entry_size.to_le_bytes());
        buf[88..92].copy_from_slice(&self.partition_entries_crc32.to_le_bytes());

        // Compute and fill header CRC32
        let crc = crc32fast::hash(&buf[..92]);
        buf[16..20].copy_from_slice(&crc.to_le_bytes());
        buf
    }

    pub fn deserialize(sector: &[u8; 512]) -> Result<Self> {
        if &sector[0..8] != GPT_SIGNATURE {
            bail!("invalid GPT signature");
        }

        // Verify header CRC32
        let stored_crc = u32::from_le_bytes(sector[16..20].try_into().unwrap());
        let header_size = u32::from_le_bytes(sector[12..16].try_into().unwrap()) as usize;
        let mut check = vec![0u8; header_size];
        check.copy_from_slice(&sector[..header_size]);
        check[16..20].copy_from_slice(&0u32.to_le_bytes());
        let computed_crc = crc32fast::hash(&check);
        if stored_crc != computed_crc {
            bail!(
                "GPT header CRC32 mismatch (stored {stored_crc:#010X}, computed {computed_crc:#010X})"
            );
        }

        let mut disk_guid = [0u8; 16];
        disk_guid.copy_from_slice(&sector[56..72]);

        Ok(Self {
            my_lba: u64::from_le_bytes(sector[24..32].try_into().unwrap()),
            alternate_lba: u64::from_le_bytes(sector[32..40].try_into().unwrap()),
            first_usable_lba: u64::from_le_bytes(sector[40..48].try_into().unwrap()),
            last_usable_lba: u64::from_le_bytes(sector[48..56].try_into().unwrap()),
            disk_guid,
            partition_entry_lba: u64::from_le_bytes(sector[72..80].try_into().unwrap()),
            num_partition_entries: u32::from_le_bytes(sector[80..84].try_into().unwrap()),
            partition_entry_size: u32::from_le_bytes(sector[84..88].try_into().unwrap()),
            partition_entries_crc32: u32::from_le_bytes(sector[88..92].try_into().unwrap()),
        })
    }
}

// ─── GPT Partition Entry ─────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct GptPartitionEntry {
    pub type_guid: [u8; 16],
    pub unique_guid: [u8; 16],
    pub first_lba: u64,
    pub last_lba: u64,
    pub attributes: u64,
    pub name: [u16; 36],
}

impl GptPartitionEntry {
    pub fn empty() -> Self {
        Self {
            type_guid: [0; 16],
            unique_guid: [0; 16],
            first_lba: 0,
            last_lba: 0,
            attributes: 0,
            name: [0; 36],
        }
    }

    pub fn is_empty(&self) -> bool {
        self.type_guid == [0u8; 16]
    }

    pub fn size_in_sectors(&self) -> u64 {
        if self.is_empty() {
            0
        } else {
            self.last_lba - self.first_lba + 1
        }
    }

    pub fn size_in_bytes(&self) -> u64 {
        self.size_in_sectors() * SECTOR_SIZE
    }

    pub fn name_string(&self) -> String {
        let end = self.name.iter().position(|&c| c == 0).unwrap_or(36);
        String::from_utf16_lossy(&self.name[..end])
    }

    pub fn set_name(&mut self, name: &str) {
        self.name = [0u16; 36];
        for (i, ch) in name.encode_utf16().take(36).enumerate() {
            self.name[i] = ch;
        }
    }

    pub fn serialize(&self) -> [u8; 128] {
        let mut buf = [0u8; 128];
        buf[0..16].copy_from_slice(&self.type_guid);
        buf[16..32].copy_from_slice(&self.unique_guid);
        buf[32..40].copy_from_slice(&self.first_lba.to_le_bytes());
        buf[40..48].copy_from_slice(&self.last_lba.to_le_bytes());
        buf[48..56].copy_from_slice(&self.attributes.to_le_bytes());
        for (i, &ch) in self.name.iter().enumerate() {
            let off = 56 + i * 2;
            buf[off..off + 2].copy_from_slice(&ch.to_le_bytes());
        }
        buf
    }

    pub fn deserialize(buf: &[u8; 128]) -> Self {
        let mut type_guid = [0u8; 16];
        type_guid.copy_from_slice(&buf[0..16]);
        let mut unique_guid = [0u8; 16];
        unique_guid.copy_from_slice(&buf[16..32]);
        let mut name = [0u16; 36];
        for i in 0..36 {
            let off = 56 + i * 2;
            name[i] = u16::from_le_bytes([buf[off], buf[off + 1]]);
        }
        Self {
            type_guid,
            unique_guid,
            first_lba: u64::from_le_bytes(buf[32..40].try_into().unwrap()),
            last_lba: u64::from_le_bytes(buf[40..48].try_into().unwrap()),
            attributes: u64::from_le_bytes(buf[48..56].try_into().unwrap()),
            name,
        }
    }
}

// ─── GptDisk — in-memory representation of a full GPT ────────────────────────

pub struct GptDisk {
    pub header: GptHeader,
    pub entries: Vec<GptPartitionEntry>,
}

impl GptDisk {
    /// Create a brand-new, empty GPT for a disk of `total_sectors` sectors.
    pub fn new(total_sectors: u64) -> Self {
        let last_lba = total_sectors - 1;
        let first_usable = 2 + PARTITION_ARRAY_SECTORS; // LBA 34
        let last_usable = last_lba - PARTITION_ARRAY_SECTORS - 1; // leaves room for backup

        let header = GptHeader {
            my_lba: 1,
            alternate_lba: last_lba,
            first_usable_lba: first_usable,
            last_usable_lba: last_usable,
            disk_guid: new_guid(),
            partition_entry_lba: 2,
            num_partition_entries: MAX_PARTITION_ENTRIES,
            partition_entry_size: PARTITION_ENTRY_SIZE,
            partition_entries_crc32: 0,
        };

        let entries = (0..MAX_PARTITION_ENTRIES)
            .map(|_| GptPartitionEntry::empty())
            .collect();

        Self { header, entries }
    }

    /// Read both GPT header and partition entries from an open image file.
    pub fn read_from(file: &mut File) -> Result<Self> {
        // Read primary header at LBA 1
        let mut sector = [0u8; 512];
        file.seek(SeekFrom::Start(SECTOR_SIZE))
            .context("seeking to GPT header")?;
        file.read_exact(&mut sector)
            .context("reading GPT header")?;
        let header = GptHeader::deserialize(&sector)?;

        // Read partition entries
        let entries_start = header.partition_entry_lba * SECTOR_SIZE;
        file.seek(SeekFrom::Start(entries_start))?;
        let mut entries = Vec::with_capacity(header.num_partition_entries as usize);
        for _ in 0..header.num_partition_entries {
            let mut entry_buf = [0u8; 128];
            file.read_exact(&mut entry_buf)?;
            entries.push(GptPartitionEntry::deserialize(&entry_buf));
        }

        // Verify partition entries CRC32
        let entries_crc = Self::compute_entries_crc(&entries);
        if entries_crc != header.partition_entries_crc32 {
            bail!(
                "partition entries CRC32 mismatch (stored {:#010X}, computed {entries_crc:#010X})",
                header.partition_entries_crc32
            );
        }

        Ok(Self { header, entries })
    }

    /// Write both primary and backup GPT header + partition entries.
    pub fn write_to(&self, file: &mut File) -> Result<()> {
        let entries_crc = Self::compute_entries_crc(&self.entries);
        let entries_bytes = Self::serialize_entries(&self.entries);

        // ── Primary (LBA 1, entries at LBA 2) ──
        let mut primary = self.header.clone();
        primary.my_lba = 1;
        primary.alternate_lba = self.header.alternate_lba;
        primary.partition_entry_lba = 2;
        primary.partition_entries_crc32 = entries_crc;

        let mut sector = [0u8; 512];
        sector[..92].copy_from_slice(&primary.serialize());
        file.seek(SeekFrom::Start(SECTOR_SIZE))?;
        file.write_all(&sector)?;

        file.seek(SeekFrom::Start(2 * SECTOR_SIZE))?;
        file.write_all(&entries_bytes)?;

        // ── Backup (entries just after last usable, header at alternate LBA) ──
        let backup_entries_lba = self.header.last_usable_lba + 1;
        file.seek(SeekFrom::Start(backup_entries_lba * SECTOR_SIZE))?;
        file.write_all(&entries_bytes)?;

        let mut backup = self.header.clone();
        backup.my_lba = self.header.alternate_lba;
        backup.alternate_lba = 1;
        backup.partition_entry_lba = backup_entries_lba;
        backup.partition_entries_crc32 = entries_crc;

        let mut backup_sector = [0u8; 512];
        backup_sector[..92].copy_from_slice(&backup.serialize());
        file.seek(SeekFrom::Start(backup.my_lba * SECTOR_SIZE))?;
        file.write_all(&backup_sector)?;

        file.flush()?;
        Ok(())
    }

    /// Find the first gap of at least `sectors_needed` between existing partitions.
    pub fn find_free_space(&self, sectors_needed: u64) -> Option<u64> {
        let mut used: Vec<(u64, u64)> = self
            .entries
            .iter()
            .filter(|e| !e.is_empty())
            .map(|e| (e.first_lba, e.last_lba))
            .collect();
        used.sort_by_key(|&(start, _)| start);

        let mut cursor = self.header.first_usable_lba;

        for &(start, end) in &used {
            if cursor + sectors_needed - 1 < start {
                return Some(cursor);
            }
            cursor = end + 1;
        }

        // Check space after the last partition
        if cursor + sectors_needed - 1 <= self.header.last_usable_lba {
            return Some(cursor);
        }

        None
    }

    /// Return the index of the first empty entry slot.
    pub fn find_empty_entry_index(&self) -> Option<usize> {
        self.entries.iter().position(|e| e.is_empty())
    }

    /// Check whether a partition at `index` can grow to `new_last_lba`.
    pub fn can_extend_partition(&self, index: usize, new_last_lba: u64) -> bool {
        if new_last_lba > self.header.last_usable_lba {
            return false;
        }
        let entry = &self.entries[index];
        for (i, e) in self.entries.iter().enumerate() {
            if i == index || e.is_empty() {
                continue;
            }
            // Check overlap: does [entry.first_lba..new_last_lba] overlap [e.first_lba..e.last_lba]?
            if entry.first_lba <= e.last_lba && new_last_lba >= e.first_lba {
                return false;
            }
        }
        true
    }

    fn compute_entries_crc(entries: &[GptPartitionEntry]) -> u32 {
        let bytes = Self::serialize_entries(entries);
        crc32fast::hash(&bytes)
    }

    fn serialize_entries(entries: &[GptPartitionEntry]) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(entries.len() * 128);
        for entry in entries {
            bytes.extend_from_slice(&entry.serialize());
        }
        bytes
    }
}
