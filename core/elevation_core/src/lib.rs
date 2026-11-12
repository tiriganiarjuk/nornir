//! Elevation grids: decoding heights from image samples, and repairing bad pixels.
//!
//! Elevation tiles are not solid. A lake can hold one pixel far below its
//! neighbours; a tile can hold values no place on Earth has. `repair_heights`
//! finds three kinds of bad pixel and replaces each from its good neighbours:
//!
//! - **outside the height range** — below the lowest or above the highest
//!   height believable for the area;
//! - **below sea level** — in an area the caller says has no land below zero;
//! - **lone pixel** — far from every neighbour while those neighbours agree
//!   with one another. A cliff, where the neighbours themselves differ, is
//!   left alone.
//!
//! Every repair is counted by kind, so a bad tile shows up as a number.
//!
//! Pure: samples and rules in, heights and counts out.

/// Fewest good neighbours a pixel needs before it can be judged a lone pixel.
const LONE_PIXEL_MIN_NEIGHBOURS: usize = 3;

/// What counts as a believable height, and how far a lone pixel may stand out.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HeightRules {
    pub minimum_m: f32,
    pub maximum_m: f32,
    /// True for an area with no land below sea level.
    pub sea_level_floor: bool,
    pub lone_pixel_threshold_m: f32,
}

/// How many pixels were replaced, by reason.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RepairCounts {
    pub outside_height_range: usize,
    pub below_sea_level: usize,
    pub lone_pixel: usize,
}

/// A repaired grid of heights in metres, row by row from the top.
#[derive(Debug, Clone, PartialEq)]
pub struct HeightGrid {
    pub width: usize,
    pub height: usize,
    pub heights: Vec<f32>,
    pub repaired: RepairCounts,
    pub lowest_m: f32,
    pub highest_m: f32,
}

#[derive(Debug, PartialEq, thiserror::Error)]
pub enum ElevationError {
    #[error("height rules are invalid: {0}")]
    RulesInvalid(String),

    #[error("the grid is {width} by {height} but holds {found} heights, not {expected}")]
    SizeMismatch {
        width: usize,
        height: usize,
        expected: usize,
        found: usize,
    },

    #[error(
        "{unrepaired} pixels could not be repaired: no good pixel is connected to them. \
         The tile holds no usable heights under these rules."
    )]
    NothingToRepairFrom { unrepaired: usize },
}

impl ElevationError {
    /// The failure class a caller branches on — the `error.type` of a result.
    /// `rules_error`: the rules or grid size the caller stated cannot be used.
    /// `repair_error`: the heights hold nothing to repair from.
    pub fn error_type(&self) -> &'static str {
        match self {
            ElevationError::RulesInvalid(_) | ElevationError::SizeMismatch { .. } => "rules_error",
            ElevationError::NothingToRepairFrom { .. } => "repair_error",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Flaw {
    Good,
    OutsideHeightRange,
    BelowSeaLevel,
    LonePixel,
}

/// Decode Terrarium-coded RGB samples (three bytes per pixel) to heights in metres:
/// red × 256 + green + blue ÷ 256 − 32768.
pub fn decode_terrarium(rgb_samples: &[u8]) -> Vec<f32> {
    rgb_samples
        .chunks_exact(3)
        .map(|pixel| f32::from(pixel[0]) * 256.0 + f32::from(pixel[1]) + f32::from(pixel[2]) / 256.0 - 32768.0)
        .collect()
}

fn check_rules(rules: &HeightRules) -> Result<(), ElevationError> {
    let numbers = [rules.minimum_m, rules.maximum_m, rules.lone_pixel_threshold_m];
    if numbers.iter().any(|number| !number.is_finite()) {
        return Err(ElevationError::RulesInvalid("every number must be finite".to_string()));
    }
    if rules.minimum_m > rules.maximum_m {
        return Err(ElevationError::RulesInvalid(format!(
            "minimum {} m is above maximum {} m",
            rules.minimum_m, rules.maximum_m
        )));
    }
    if rules.lone_pixel_threshold_m <= 0.0 {
        return Err(ElevationError::RulesInvalid("the lone-pixel threshold must be above zero".to_string()));
    }
    Ok(())
}

fn range_flaw(height_m: f32, rules: &HeightRules) -> Flaw {
    if !height_m.is_finite() || height_m < rules.minimum_m || height_m > rules.maximum_m {
        Flaw::OutsideHeightRange
    } else if rules.sea_level_floor && height_m < 0.0 {
        Flaw::BelowSeaLevel
    } else {
        Flaw::Good
    }
}

/// Indices of the up-to-eight pixels around `index`.
fn neighbour_indices(width: usize, height: usize, index: usize) -> Vec<usize> {
    let (column, row) = (index % width, index / width);
    let (first_column, last_column) = (column.saturating_sub(1), (column + 1).min(width - 1));
    let (first_row, last_row) = (row.saturating_sub(1), (row + 1).min(height - 1));
    (first_row..=last_row)
        .flat_map(|near_row| (first_column..=last_column).map(move |near_column| near_row * width + near_column))
        .filter(|near| *near != index)
        .collect()
}

/// Heights of the good pixels around `index`.
fn good_neighbour_heights(width: usize, height: usize, index: usize, heights: &[f32], good: &[bool]) -> Vec<f32> {
    neighbour_indices(width, height, index)
        .into_iter()
        .filter(|near| good[*near])
        .map(|near| heights[near])
        .collect()
}

/// A lone pixel is far from every good neighbour while those neighbours agree with one another.
fn is_lone_pixel(height_m: f32, around: &[f32], threshold_m: f32) -> bool {
    if around.len() < LONE_PIXEL_MIN_NEIGHBOURS {
        return false;
    }
    let lowest = around.iter().copied().fold(f32::INFINITY, f32::min);
    let highest = around.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    highest - lowest <= threshold_m && around.iter().all(|near| (height_m - near).abs() > threshold_m)
}

fn median(values: &mut [f32]) -> f32 {
    values.sort_by(f32::total_cmp);
    let middle = values.len() / 2;
    if values.len() % 2 == 0 {
        (values[middle - 1] + values[middle]) / 2.0
    } else {
        values[middle]
    }
}

/// Judge every pixel on the heights as decoded, before anything is replaced.
fn find_flaws(width: usize, height: usize, heights: &[f32], rules: &HeightRules) -> Vec<Flaw> {
    let by_range: Vec<Flaw> = heights.iter().map(|height_m| range_flaw(*height_m, rules)).collect();
    let in_range: Vec<bool> = by_range.iter().map(|flaw| *flaw == Flaw::Good).collect();
    by_range
        .iter()
        .enumerate()
        .map(|(index, flaw)| {
            let around = good_neighbour_heights(width, height, index, heights, &in_range);
            if *flaw == Flaw::Good && is_lone_pixel(heights[index], &around, rules.lone_pixel_threshold_m) {
                Flaw::LonePixel
            } else {
                *flaw
            }
        })
        .collect()
}

fn count_flaws(flaws: &[Flaw]) -> RepairCounts {
    let count_of = |kind: Flaw| flaws.iter().filter(|flaw| **flaw == kind).count();
    RepairCounts {
        outside_height_range: count_of(Flaw::OutsideHeightRange),
        below_sea_level: count_of(Flaw::BelowSeaLevel),
        lone_pixel: count_of(Flaw::LonePixel),
    }
}

/// Replace every flawed pixel with the median of its good neighbours, working
/// inward from the good pixels until none is left or none can be reached.
fn fill_flawed(width: usize, height: usize, heights: &mut [f32], good: &mut [bool]) -> Result<(), ElevationError> {
    loop {
        let fills: Vec<(usize, f32)> = (0..heights.len())
            .filter(|index| !good[*index])
            .filter_map(|index| {
                let mut around = good_neighbour_heights(width, height, index, heights, good);
                (!around.is_empty()).then(|| (index, median(&mut around)))
            })
            .collect();
        if fills.is_empty() {
            break;
        }
        for (index, height_m) in fills {
            heights[index] = height_m;
            good[index] = true;
        }
    }
    match good.iter().filter(|is_good| !**is_good).count() {
        0 => Ok(()),
        unrepaired => Err(ElevationError::NothingToRepairFrom { unrepaired }),
    }
}

/// Find and replace the bad pixels of a `width` by `height` grid of heights.
pub fn repair_heights(
    width: usize,
    height: usize,
    mut heights: Vec<f32>,
    rules: &HeightRules,
) -> Result<HeightGrid, ElevationError> {
    check_rules(rules)?;
    let expected = width.saturating_mul(height);
    if heights.len() != expected || expected == 0 {
        return Err(ElevationError::SizeMismatch { width, height, expected, found: heights.len() });
    }
    let flaws = find_flaws(width, height, &heights, rules);
    let mut good: Vec<bool> = flaws.iter().map(|flaw| *flaw == Flaw::Good).collect();
    fill_flawed(width, height, &mut heights, &mut good)?;
    let lowest_m = heights.iter().copied().fold(f32::INFINITY, f32::min);
    let highest_m = heights.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    Ok(HeightGrid { width, height, heights, repaired: count_flaws(&flaws), lowest_m, highest_m })
}

#[cfg(test)]
mod tests {
    use super::*;

    const LAND: HeightRules =
        HeightRules { minimum_m: -430.0, maximum_m: 8849.0, sea_level_floor: true, lone_pixel_threshold_m: 20.0 };

    fn flat(width: usize, height: usize, height_m: f32) -> Vec<f32> {
        vec![height_m; width * height]
    }

    // -- decoding --

    #[test]
    fn terrarium_zero_metres() {
        assert_eq!(decode_terrarium(&[128, 0, 0]), vec![0.0]);
    }

    #[test]
    fn terrarium_fraction_and_negative() {
        assert_eq!(decode_terrarium(&[128, 10, 128, 127, 255, 0]), vec![10.5, -1.0]);
    }

    // -- left alone --

    #[test]
    fn clean_grid_is_unchanged() {
        let grid = repair_heights(3, 3, flat(3, 3, 26.0), &LAND).unwrap();
        assert_eq!(grid.heights, flat(3, 3, 26.0));
        assert_eq!(grid.repaired, RepairCounts::default());
        assert_eq!((grid.lowest_m, grid.highest_m), (26.0, 26.0));
    }

    #[test]
    fn cliff_is_left_alone() {
        // Left two columns at 0 m, right two at 300 m: every pixel has neighbours that disagree.
        let heights: Vec<f32> = (0..16).map(|index| if index % 4 < 2 { 0.0 } else { 300.0 }).collect();
        let grid = repair_heights(4, 4, heights.clone(), &LAND).unwrap();
        assert_eq!(grid.heights, heights);
        assert_eq!(grid.repaired, RepairCounts::default());
    }

    #[test]
    fn below_sea_level_is_kept_where_land_may_be_below_zero() {
        let rules = HeightRules { sea_level_floor: false, ..LAND };
        let grid = repair_heights(3, 3, flat(3, 3, -5.0), &rules).unwrap();
        assert_eq!(grid.heights, flat(3, 3, -5.0));
        assert_eq!(grid.repaired, RepairCounts::default());
    }

    // -- repaired --

    #[test]
    fn lone_pixel_in_a_lake_is_replaced_from_its_neighbours() {
        let mut heights = flat(3, 3, 26.0);
        heights[4] = 0.0;
        let grid = repair_heights(3, 3, heights, &LAND).unwrap();
        assert_eq!(grid.heights, flat(3, 3, 26.0));
        assert_eq!(grid.repaired, RepairCounts { lone_pixel: 1, ..RepairCounts::default() });
    }

    #[test]
    fn pixel_outside_the_height_range_is_replaced() {
        let mut heights = flat(3, 3, 100.0);
        heights[0] = 20000.0;
        let grid = repair_heights(3, 3, heights, &LAND).unwrap();
        assert_eq!(grid.heights, flat(3, 3, 100.0));
        assert_eq!(grid.repaired, RepairCounts { outside_height_range: 1, ..RepairCounts::default() });
    }

    #[test]
    fn pixel_below_sea_level_is_replaced_where_the_floor_applies() {
        let mut heights = flat(3, 3, 2.0);
        heights[8] = -3.0;
        let grid = repair_heights(3, 3, heights, &LAND).unwrap();
        assert_eq!(grid.heights, flat(3, 3, 2.0));
        assert_eq!(grid.repaired, RepairCounts { below_sea_level: 1, ..RepairCounts::default() });
    }

    #[test]
    fn block_of_bad_pixels_is_filled_inward() {
        // 5x5 at 50 m with the whole 3x3 centre out of range: the centre pixel has no good
        // neighbour until the ring around it has been filled.
        let mut heights = flat(5, 5, 50.0);
        for index in [6, 7, 8, 11, 12, 13, 16, 17, 18] {
            heights[index] = 30000.0;
        }
        let grid = repair_heights(5, 5, heights, &LAND).unwrap();
        assert_eq!(grid.heights, flat(5, 5, 50.0));
        assert_eq!(grid.repaired.outside_height_range, 9);
    }

    // -- refused --

    #[test]
    fn grid_with_no_good_pixel_is_refused() {
        let result = repair_heights(2, 2, flat(2, 2, 30000.0), &LAND);
        assert_eq!(result, Err(ElevationError::NothingToRepairFrom { unrepaired: 4 }));
    }

    #[test]
    fn wrong_number_of_heights_is_refused() {
        let result = repair_heights(3, 3, flat(2, 2, 1.0), &LAND);
        assert_eq!(result, Err(ElevationError::SizeMismatch { width: 3, height: 3, expected: 9, found: 4 }));
    }

    #[test]
    fn inverted_range_is_refused() {
        let rules = HeightRules { minimum_m: 100.0, maximum_m: 0.0, ..LAND };
        assert!(matches!(repair_heights(1, 1, vec![1.0], &rules), Err(ElevationError::RulesInvalid(_))));
    }
}
