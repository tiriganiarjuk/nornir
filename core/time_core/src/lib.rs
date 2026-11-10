//! Pure time conversion utilities.
//!
//! No I/O, no system clock reads. All functions take epoch seconds as input.

/// Decompose Unix epoch seconds into (year, month, day, hour, minute, second).
/// Month and day are 1-based. Handles 1970–2099 correctly.
fn epoch_to_parts(epoch_secs: u64) -> (i64, u8, u8, u8, u8, u8) {
    let total_secs = epoch_secs;
    let hour = ((total_secs % 86400) / 3600) as u8;
    let minute = ((total_secs % 3600) / 60) as u8;
    let second = (total_secs % 60) as u8;

    let days = total_secs / 86400;
    let mut year = 1970i64;
    let mut remaining = days as i64;

    loop {
        let year_days = if is_leap(year) { 366 } else { 365 };
        if remaining < year_days {
            break;
        }
        remaining -= year_days;
        year += 1;
    }

    let month_days = month_lengths(is_leap(year));
    let mut month = 0u8;
    for md in &month_days {
        if remaining < *md as i64 {
            break;
        }
        remaining -= *md as i64;
        month += 1;
    }

    (year, month + 1, remaining as u8 + 1, hour, minute, second)
}

fn is_leap(year: i64) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

fn month_lengths(leap: bool) -> [u8; 12] {
    [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
}

/// Convert Unix epoch seconds to a `YYYY-MM-DD` civil date string (UTC).
pub fn civil_date(epoch_secs: u64) -> String {
    let (year, month, day, _, _, _) = epoch_to_parts(epoch_secs);
    format!("{year:04}-{month:02}-{day:02}")
}

/// Convert Unix epoch seconds to an ISO 8601 Zulu timestamp: `YYYY-MM-DDTHH:MM:SSZ` (UTC).
pub fn iso_zulu(epoch_secs: u64) -> String {
    let (year, month, day, hour, minute, second) = epoch_to_parts(epoch_secs);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// Convert Unix epoch seconds to the ISO 8601 basic Zulu form: `YYYYMMDDTHHMMSSZ` (UTC).
///
/// No separators, so the result is safe as a filename component and sorts
/// chronologically as a plain string.
pub fn iso_basic(epoch_secs: u64) -> String {
    let (year, month, day, hour, minute, second) = epoch_to_parts(epoch_secs);
    format!("{year:04}{month:02}{day:02}T{hour:02}{minute:02}{second:02}Z")
}

/// Reduce an extended Zulu timestamp (`YYYY-MM-DDTHH:MM:SSZ`) to its basic form
/// (`YYYYMMDDTHHMMSSZ`).
///
/// Pure string transform: the shape is checked, not the calendar. Returns the
/// reason when the input is not exactly the extended Zulu form.
pub fn iso_zulu_to_basic(iso_zulu: &str) -> Result<String, String> {
    const EXTENDED_LEN: usize = "YYYY-MM-DDTHH:MM:SSZ".len();
    if iso_zulu.len() != EXTENDED_LEN {
        return Err(format!(
            "expected YYYY-MM-DDTHH:MM:SSZ ({EXTENDED_LEN} chars), got {} chars: {iso_zulu:?}",
            iso_zulu.len()
        ));
    }
    let bytes = iso_zulu.as_bytes();
    let separators_ok = bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes[10] == b'T'
        && bytes[13] == b':'
        && bytes[16] == b':'
        && bytes[19] == b'Z';
    if !separators_ok {
        return Err(format!("expected YYYY-MM-DDTHH:MM:SSZ separators, got {iso_zulu:?}"));
    }
    let digits_ok = iso_zulu
        .char_indices()
        .filter(|(i, _)| !matches!(i, 4 | 7 | 10 | 13 | 16 | 19))
        .all(|(_, c)| c.is_ascii_digit());
    if !digits_ok {
        return Err(format!("expected digits between separators, got {iso_zulu:?}"));
    }
    Ok(iso_zulu.chars().filter(|c| *c != '-' && *c != ':').collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- civil_date ---

    #[test]
    fn civil_date_epoch_zero() {
        assert_eq!(civil_date(0), "1970-01-01");
    }

    #[test]
    fn civil_date_known_2024_01_01() {
        assert_eq!(civil_date(1704067200), "2024-01-01");
    }

    #[test]
    fn civil_date_leap_day_2024() {
        assert_eq!(civil_date(1709164800), "2024-02-29");
    }

    #[test]
    fn civil_date_year_boundary_2023_12_31() {
        // 2024-01-01 00:00:00 minus 1 second
        assert_eq!(civil_date(1704067199), "2023-12-31");
    }

    #[test]
    fn civil_date_2026_03_20() {
        // 2026-03-20 00:00:00 UTC
        assert_eq!(civil_date(1773964800), "2026-03-20");
    }

    #[test]
    fn civil_date_end_of_feb_non_leap() {
        // 2023-02-28 12:00:00 UTC = 1677585600
        assert_eq!(civil_date(1677585600), "2023-02-28");
    }

    #[test]
    fn civil_date_march_1_non_leap() {
        // 2023-03-01 00:00:00 UTC = 1677628800
        assert_eq!(civil_date(1677628800), "2023-03-01");
    }

    // --- iso_zulu ---

    #[test]
    fn iso_zulu_epoch_zero() {
        assert_eq!(iso_zulu(0), "1970-01-01T00:00:00Z");
    }

    #[test]
    fn iso_zulu_known_timestamp() {
        // 2024-01-01 12:30:45 UTC = 1704067200 + 45045
        assert_eq!(iso_zulu(1704112245), "2024-01-01T12:30:45Z");
    }

    #[test]
    fn iso_zulu_end_of_day() {
        // 2024-01-01 23:59:59 UTC = 1704067200 + 86399
        assert_eq!(iso_zulu(1704153599), "2024-01-01T23:59:59Z");
    }

    #[test]
    fn iso_zulu_midnight_boundary() {
        // 2024-01-02 00:00:00 UTC = 1704067200 + 86400
        assert_eq!(iso_zulu(1704153600), "2024-01-02T00:00:00Z");
    }

    // --- iso_basic ---

    #[test]
    fn iso_basic_known_timestamp() {
        assert_eq!(iso_basic(1704112245), "20240101T123045Z");
    }

    #[test]
    fn iso_basic_sorts_as_string() {
        assert!(iso_basic(1704112245) < iso_basic(1704112246));
        assert!(iso_basic(1704153599) < iso_basic(1704153600));
    }

    // --- iso_zulu_to_basic ---

    #[test]
    fn iso_zulu_to_basic_round_trips_iso_zulu() {
        let extended = iso_zulu(1704112245);
        assert_eq!(iso_zulu_to_basic(&extended).unwrap(), iso_basic(1704112245));
    }

    #[test]
    fn iso_zulu_to_basic_rejects_wrong_length() {
        assert!(iso_zulu_to_basic("2024-01-01T12:30:45").is_err());
        assert!(iso_zulu_to_basic("2024-01-01T12:30:45.123Z").is_err());
    }

    #[test]
    fn iso_zulu_to_basic_rejects_wrong_separators() {
        assert!(iso_zulu_to_basic("2024/01/01T12:30:45Z").is_err());
        assert!(iso_zulu_to_basic("2024-01-01 12:30:45Z").is_err());
    }

    #[test]
    fn iso_zulu_to_basic_rejects_non_digits() {
        assert!(iso_zulu_to_basic("2024-01-0aT12:30:45Z").is_err());
    }

    // --- is_leap ---

    #[test]
    fn leap_year_2000() {
        assert!(is_leap(2000)); // divisible by 400
    }

    #[test]
    fn not_leap_1900() {
        assert!(!is_leap(1900)); // divisible by 100 but not 400
    }

    #[test]
    fn leap_year_2024() {
        assert!(is_leap(2024)); // divisible by 4
    }

    #[test]
    fn not_leap_2023() {
        assert!(!is_leap(2023));
    }
}
