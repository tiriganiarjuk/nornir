//! Angles written in degrees and minutes, with or without seconds.
//!
//! A value names its notation; nothing is guessed. The parts are separated by their
//! symbols (`°`, `'`, `"`) or by spaces. South and west are negative, given either by
//! a hemisphere letter at the end or by a leading minus sign, never both.

/// A way of writing an angle in parts.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AngleNotation {
    /// Whole degrees, whole minutes, seconds: `56°32'24.5"N`
    DegreesMinutesSeconds,
    /// Whole degrees, minutes: `61°42.654'W`
    DegreesMinutes,
}

/// What each part of an angle is called, in the order written.
const PART_NAMES: [&str; 3] = ["degrees", "minutes", "seconds"];

/// The symbols that may follow each part, in the order written.
const PART_SYMBOLS: [&[char]; 3] = [&['°'], &['\'', '′'], &['"', '″']];

impl AngleNotation {
    /// The names a field's unit annotation gives these notations.
    pub const NAMES: [&'static str; 2] = ["dms", "dm"];

    /// The notation a unit annotation names.
    pub fn from_name(name: &str) -> Option<Self> {
        match name.trim().to_lowercase().as_str() {
            "dms" => Some(AngleNotation::DegreesMinutesSeconds),
            "dm" => Some(AngleNotation::DegreesMinutes),
            _ => None,
        }
    }

    /// The name a unit annotation gives this notation.
    pub fn name(&self) -> &'static str {
        match self {
            AngleNotation::DegreesMinutesSeconds => "dms",
            AngleNotation::DegreesMinutes => "dm",
        }
    }

    /// A value written in this notation.
    pub fn example(&self) -> &'static str {
        match self {
            AngleNotation::DegreesMinutesSeconds => "56°32'24.5\"N",
            AngleNotation::DegreesMinutes => "61°42.654'W",
        }
    }

    fn part_count(&self) -> usize {
        match self {
            AngleNotation::DegreesMinutesSeconds => 3,
            AngleNotation::DegreesMinutes => 2,
        }
    }
}

/// The angle in decimal degrees, or why the text is not an angle in this notation.
pub fn parse_angle(text: &str, notation: AngleNotation) -> Result<f64, String> {
    let (without_letter, hemisphere_sign) = split_hemisphere(text.trim())?;
    let (parts_text, written_sign) = split_sign(without_letter);
    if hemisphere_sign.is_some() && written_sign.is_some() {
        return Err("it has both a sign and a hemisphere letter; use one".to_string());
    }

    let parts = read_parts(parts_text, notation.part_count())?;
    for (position, part) in parts.iter().enumerate().skip(1) {
        if *part >= 60.0 {
            return Err(format!("{} must be below 60, found {}", PART_NAMES[position], part));
        }
    }

    // Summed in the smallest part written, so that part is never rescaled.
    let smallest_per_degree = 60f64.powi(parts.len() as i32 - 1);
    let total_in_smallest = parts
        .iter()
        .enumerate()
        .map(|(position, part)| part * (smallest_per_degree / 60f64.powi(position as i32)))
        .sum::<f64>();
    let sign = hemisphere_sign.or(written_sign).unwrap_or(1.0);
    Ok(sign * total_in_smallest / smallest_per_degree)
}

/// The text without a closing hemisphere letter, and the sign that letter gives.
fn split_hemisphere(text: &str) -> Result<(&str, Option<f64>), String> {
    let last = match text.chars().last() {
        Some(character) if character.is_alphabetic() => character,
        _ => return Ok((text, None)),
    };
    let sign = match last.to_ascii_uppercase() {
        'N' | 'E' => 1.0,
        'S' | 'W' => -1.0,
        _ => return Err(format!("'{}' is not a hemisphere letter; use N, S, E or W", last)),
    };
    let without_letter = &text[..text.len() - last.len_utf8()];
    Ok((without_letter.trim_end(), Some(sign)))
}

/// The text without a leading sign, and the sign written.
fn split_sign(text: &str) -> (&str, Option<f64>) {
    if let Some(rest) = text.strip_prefix('-') {
        return (rest, Some(-1.0));
    }
    if let Some(rest) = text.strip_prefix('+') {
        return (rest, Some(1.0));
    }
    (text, None)
}

/// The angle's parts as numbers: every part but the last a whole number.
fn read_parts(text: &str, part_count: usize) -> Result<Vec<f64>, String> {
    let mut rest = text.trim_start();
    let mut parts = Vec::with_capacity(part_count);

    for position in 0..part_count {
        let name = PART_NAMES[position];
        let end = rest
            .find(|character: char| !(character.is_ascii_digit() || character == '.'))
            .unwrap_or(rest.len());
        if end == 0 {
            return Err(if rest.is_empty() {
                format!("it has no {}", name)
            } else {
                format!("expected {} but found '{}'", name, rest)
            });
        }

        let number_text = &rest[..end];
        if position + 1 < part_count && number_text.contains('.') {
            return Err(format!("{} must be a whole number, found {}", name, number_text));
        }
        let number: f64 = number_text
            .parse()
            .map_err(|_| format!("{} is not a number: '{}'", name, number_text))?;
        parts.push(number);

        rest = rest[end..].trim_start();
        if let Some(after_symbol) = rest.strip_prefix(PART_SYMBOLS[position]) {
            rest = after_symbol.trim_start();
        }
    }

    if !rest.is_empty() {
        return Err(format!("unexpected '{}' after the {}", rest, PART_NAMES[part_count - 1]));
    }
    Ok(parts)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DMS: AngleNotation = AngleNotation::DegreesMinutesSeconds;
    const DM: AngleNotation = AngleNotation::DegreesMinutes;

    #[test]
    fn test_degrees_minutes_seconds_north() {
        let expected = (56.0 * 3600.0 + 32.0 * 60.0 + 24.5) / 3600.0;
        assert_eq!(parse_angle("56°32'24.5\"N", DMS).unwrap(), expected);
    }

    #[test]
    fn test_degrees_minutes_west_is_negative() {
        let expected = -(61.0 * 60.0 + 42.654) / 60.0;
        assert_eq!(parse_angle("61°42.654'W", DM).unwrap(), expected);
    }

    #[test]
    fn test_parts_separated_by_spaces() {
        assert_eq!(parse_angle("56 32 24.5 N", DMS).unwrap(), parse_angle("56°32'24.5\"N", DMS).unwrap());
        assert_eq!(parse_angle("61 42.654 W", DM).unwrap(), parse_angle("61°42.654'W", DM).unwrap());
    }

    #[test]
    fn test_prime_symbols() {
        assert_eq!(parse_angle("56°32′24.5″N", DMS).unwrap(), parse_angle("56°32'24.5\"N", DMS).unwrap());
    }

    #[test]
    fn test_minus_sign_in_place_of_a_letter() {
        assert_eq!(parse_angle("-61°42.654'", DM).unwrap(), parse_angle("61°42.654'W", DM).unwrap());
    }

    #[test]
    fn test_whole_degrees() {
        assert_eq!(parse_angle("45°30'N", DM).unwrap(), 45.5);
        assert_eq!(parse_angle("10°15'0\"S", DMS).unwrap(), -10.25);
    }

    #[test]
    fn test_sign_and_letter_together_are_refused() {
        assert!(parse_angle("-61°42.654'W", DM).unwrap_err().contains("both"));
    }

    #[test]
    fn test_minutes_of_sixty_are_refused() {
        assert!(parse_angle("56°60'N", DM).unwrap_err().contains("minutes must be below 60"));
        assert!(parse_angle("56°10'60\"N", DMS).unwrap_err().contains("seconds must be below 60"));
    }

    #[test]
    fn test_missing_part_is_refused() {
        assert!(parse_angle("56°32'N", DMS).unwrap_err().contains("no seconds"));
    }

    #[test]
    fn test_extra_part_is_refused() {
        assert!(parse_angle("56°32'24.5\"N", DM).unwrap_err().contains("unexpected"));
    }

    #[test]
    fn test_fractional_degrees_are_refused() {
        assert!(parse_angle("56.5°30'N", DM).unwrap_err().contains("degrees must be a whole number"));
    }

    #[test]
    fn test_other_letters_are_refused() {
        assert!(parse_angle("56°32'24\"X", DMS).unwrap_err().contains("not a hemisphere letter"));
    }

    #[test]
    fn test_notation_names() {
        assert_eq!(AngleNotation::from_name("dms"), Some(DMS));
        assert_eq!(AngleNotation::from_name("DM"), Some(DM));
        assert_eq!(AngleNotation::from_name("deg"), None);
    }
}
