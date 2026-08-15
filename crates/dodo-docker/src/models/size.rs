//! Human-readable byte sizes for the Images and Volumes columns.
//!
//! Docker's own CLI reports image sizes in SI units (base 1000: `kB`, `MB`,
//! `GB`), so [`format_size`] does the same, keeping the numbers a person
//! recognises from `docker images`. Pure, so it is unit tested directly.

/// The SI unit ladder, base 1000. `B` is whole; the rest carry one decimal.
const UNITS: [&str; 6] = ["B", "kB", "MB", "GB", "TB", "PB"];

/// Formats a byte count the way the Docker CLI does: exact bytes below a
/// kilobyte (`512B`), then one decimal place in the largest fitting SI unit
/// (`5.2MB`). A negative count — the engine's "not calculated" sentinel — is
/// clamped to zero rather than printed with a sign.
pub fn format_size(bytes: i64) -> String {
    let bytes = bytes.max(0);
    if bytes < 1000 {
        return format!("{bytes}{}", UNITS[0]);
    }
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1000.0 && unit < UNITS.len() - 1 {
        size /= 1000.0;
        unit += 1;
    }
    format!("{size:.1}{}", UNITS[unit])
}

#[cfg(test)]
mod tests {
    use super::format_size;

    #[test]
    fn bytes_below_a_kilobyte_are_exact() {
        assert_eq!(format_size(0), "0B");
        assert_eq!(format_size(512), "512B");
        assert_eq!(format_size(999), "999B");
    }

    #[test]
    fn larger_sizes_use_one_decimal_si_unit() {
        assert_eq!(format_size(1000), "1.0kB");
        assert_eq!(format_size(1500), "1.5kB");
        assert_eq!(format_size(187_000_000), "187.0MB");
        assert_eq!(format_size(2_500_000_000), "2.5GB");
    }

    #[test]
    fn a_negative_sentinel_is_clamped_to_zero() {
        // The Engine API uses -1 for "size not calculated".
        assert_eq!(format_size(-1), "0B");
    }
}
