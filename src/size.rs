use anyhow::{bail, Result};

pub const SECTOR_SIZE: u64 = 512;

pub fn parse_size(s: &str) -> Result<u64> {
    let s = s.trim();
    if s.is_empty() {
        bail!("size string is empty");
    }

    let (num_str, multiplier) = match s.as_bytes().last() {
        Some(b'T' | b't') => (&s[..s.len() - 1], 1u64 << 40),
        Some(b'G' | b'g') => (&s[..s.len() - 1], 1u64 << 30),
        Some(b'M' | b'm') => (&s[..s.len() - 1], 1u64 << 20),
        Some(b'K' | b'k') => (&s[..s.len() - 1], 1u64 << 10),
        _ => (s, 1u64),
    };

    let num: f64 = num_str
        .parse()
        .map_err(|_| anyhow::anyhow!("invalid size value: '{}'", s))?;

    if num <= 0.0 {
        bail!("size must be greater than zero");
    }

    let bytes = (num * multiplier as f64) as u64;

    // Round up to nearest sector boundary
    let aligned = bytes.div_ceil(SECTOR_SIZE) * SECTOR_SIZE;
    Ok(aligned)
}

pub fn format_size(bytes: u64) -> String {
    const TIB: u64 = 1u64 << 40;
    const GIB: u64 = 1u64 << 30;
    const MIB: u64 = 1u64 << 20;
    const KIB: u64 = 1u64 << 10;

    if bytes >= TIB {
        format_unit(bytes as f64 / TIB as f64, "T")
    } else if bytes >= GIB {
        format_unit(bytes as f64 / GIB as f64, "G")
    } else if bytes >= MIB {
        format_unit(bytes as f64 / MIB as f64, "M")
    } else if bytes >= KIB {
        format_unit(bytes as f64 / KIB as f64, "K")
    } else {
        format!("{} B", bytes)
    }
}

fn format_unit(val: f64, unit: &str) -> String {
    if (val - val.round()).abs() < 0.01 {
        format!("{}{}", val.round() as u64, unit)
    } else if (val * 10.0 - (val * 10.0).round()).abs() < 0.01 {
        format!("{:.1}{}", val, unit)
    } else {
        format!("{:.2}{}", val, unit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_basic_sizes() {
        assert_eq!(parse_size("512").unwrap(), 512);
        assert_eq!(parse_size("1K").unwrap(), 1024);
        assert_eq!(parse_size("1M").unwrap(), 1 << 20);
        assert_eq!(parse_size("1G").unwrap(), 1 << 30);
        assert_eq!(parse_size("32G").unwrap(), 32 << 30);
        assert_eq!(parse_size("256M").unwrap(), 256 << 20);
    }

    #[test]
    fn parse_rounds_up() {
        // 1000 bytes rounds up to 1024 (2 sectors)
        assert_eq!(parse_size("1000").unwrap(), 1024);
        // 513 rounds up to 1024
        assert_eq!(parse_size("513").unwrap(), 1024);
    }

    #[test]
    fn format_sizes() {
        assert_eq!(format_size(1 << 30), "1G");
        assert_eq!(format_size(32 << 30), "32G");
        assert_eq!(format_size(512 << 20), "512M");
        assert_eq!(format_size(2560 << 20), "2.5G");
    }
}
