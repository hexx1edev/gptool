use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartitionType {
    EfiSystem,
    MicrosoftBasicData,
    LinuxFilesystem,
    BiosBootPartition,
    LinuxSwap,
}

impl PartitionType {
    pub fn from_name(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "efi" => Some(Self::EfiSystem),
            "microsoft" => Some(Self::MicrosoftBasicData),
            "linux" => Some(Self::LinuxFilesystem),
            "bios" => Some(Self::BiosBootPartition),
            "swap" => Some(Self::LinuxSwap),
            _ => None,
        }
    }

    pub fn guid_bytes(&self) -> [u8; 16] {
        match self {
            Self::EfiSystem => uuid_to_mixed_endian("C12A7328-F81F-11D2-BA4B-00A0C93EC93B"),
            Self::MicrosoftBasicData => {
                uuid_to_mixed_endian("EBD0A0A2-B9E5-4433-87C0-68B6B72699C7")
            }
            Self::LinuxFilesystem => {
                uuid_to_mixed_endian("0FC63DAF-8483-4772-8E79-3D69D8477DE4")
            }
            Self::BiosBootPartition => {
                uuid_to_mixed_endian("21686148-6449-6E6F-744E-656564454649")
            }
            Self::LinuxSwap => uuid_to_mixed_endian("0657FD6D-A4AB-43C4-84E5-0933C84B4F4F"),
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Self::EfiSystem => "EFI System",
            Self::MicrosoftBasicData => "Microsoft Basic Data",
            Self::LinuxFilesystem => "Linux Filesystem",
            Self::BiosBootPartition => "BIOS Boot",
            Self::LinuxSwap => "Linux Swap",
        }
    }

    pub fn from_guid(guid: &[u8; 16]) -> Option<Self> {
        const ALL: &[PartitionType] = &[
            PartitionType::EfiSystem,
            PartitionType::MicrosoftBasicData,
            PartitionType::LinuxFilesystem,
            PartitionType::BiosBootPartition,
            PartitionType::LinuxSwap,
        ];
        ALL.iter().find(|t| t.guid_bytes() == *guid).copied()
    }

    pub fn all_types() -> &'static [(&'static str, &'static str)] {
        &[
            ("efi", "EFI System Partition"),
            ("microsoft", "Microsoft Basic Data"),
            ("linux", "Linux Filesystem"),
            ("bios", "BIOS Boot Partition"),
            ("swap", "Linux Swap"),
        ]
    }
}

/// Generate a new random GUID in mixed-endian format for GPT
pub fn new_guid() -> [u8; 16] {
    let uuid = Uuid::new_v4();
    let b = uuid.as_bytes();
    let mut guid = [0u8; 16];
    // First 3 groups: little-endian
    guid[0] = b[3];
    guid[1] = b[2];
    guid[2] = b[1];
    guid[3] = b[0];
    guid[4] = b[5];
    guid[5] = b[4];
    guid[6] = b[7];
    guid[7] = b[6];
    // Last 2 groups: big-endian (same as RFC 4122)
    guid[8..16].copy_from_slice(&b[8..16]);
    guid
}

/// Format a mixed-endian GUID as a standard UUID string
pub fn guid_to_string(guid: &[u8; 16]) -> String {
    format!(
        "{:02X}{:02X}{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}",
        guid[3], guid[2], guid[1], guid[0],
        guid[5], guid[4],
        guid[7], guid[6],
        guid[8], guid[9],
        guid[10], guid[11], guid[12], guid[13], guid[14], guid[15],
    )
}

/// Convert a UUID string (RFC 4122) to mixed-endian GUID bytes for GPT
fn uuid_to_mixed_endian(s: &str) -> [u8; 16] {
    let uuid = Uuid::parse_str(s).expect("invalid hardcoded UUID");
    let b = uuid.as_bytes();
    let mut guid = [0u8; 16];
    guid[0] = b[3];
    guid[1] = b[2];
    guid[2] = b[1];
    guid[3] = b[0];
    guid[4] = b[5];
    guid[5] = b[4];
    guid[6] = b[7];
    guid[7] = b[6];
    guid[8..16].copy_from_slice(&b[8..16]);
    guid
}
