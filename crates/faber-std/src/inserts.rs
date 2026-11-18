//! Dimensions in mm for Series 19 long, straight-hole heat-set inserts.
//! See the catalogue and hole design guide in `docs/material-data.md`.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InsertHole {
    pub designation: &'static str,
    pub hole_diameter: f64,
    pub depth: f64,
    pub least_boss_diameter: f64,
}

/// Depth includes two thread pitches. The boss is twice the insert knurl diameter.
pub fn insert_hole(designation: &str) -> Option<InsertHole> {
    // Series 19 long brass. Columns are pilot hole, insert length, pitch and overknurl diameter.
    let (name, hole, length, pitch, knurl) = match designation {
        "M2-insert" => ("M2-insert", 3.20, 3.99, 0.4, 3.63),
        "M3-insert" => ("M3-insert", 3.99, 5.74, 0.5, 4.75),
        "M4-insert" => ("M4-insert", 5.61, 8.15, 0.7, 6.38),
        "M5-insert" => ("M5-insert", 6.40, 9.53, 0.8, 7.16),
        _ => return None,
    };
    Some(InsertHole {
        designation: name,
        hole_diameter: hole,
        depth: length + 2. * pitch,
        least_boss_diameter: 2. * knurl,
    })
}
