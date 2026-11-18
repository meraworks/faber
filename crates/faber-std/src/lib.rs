//! Metric standard tables: thread pitch, clearance holes, tap drills and screw heads.
//! Lengths are in millimetres.

pub mod materials;
pub mod printers;
mod table;

/// Clearance hole series from ISO 273.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fit {
    Fine,
    Medium,
    Coarse,
}

/// Extra depth below the socket head, in millimetres. A practice value of this crate.
pub const COUNTERBORE_ALLOWANCE: f64 = 0.2;

/// Included angle of a countersink for ISO 10642 heads, in degrees.
pub const COUNTERSINK_ANGLE: f64 = 90.0;

/// Socket head cap screw head size from ISO 4762, maximum values.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SocketHead {
    /// Head diameter, maximum.
    pub dk: f64,
    /// Head height, maximum.
    pub k: f64,
}

/// Flush head counterbore.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Counterbore {
    pub diameter: f64,
    pub depth: f64,
}

/// Countersink for an ISO 10642 head.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Countersink {
    pub head_diameter: f64,
    pub angle: f64,
}

/// One metric size. The source of each value is in its field doc.
///
/// Sources: ISO 261 (pitch), ISO 273 (clearance), ISO 4762 (socket head),
/// ISO 10642 (countersink). The counterbore diameter is common practice.
#[derive(Debug)]
pub struct MetricSize {
    name: &'static str,
    nominal: f64,
    pitch: f64,
    clearance: [f64; 3],
    socket_head: SocketHead,
    countersink_head: Option<f64>,
    counterbore_diameter: Option<f64>,
}

impl MetricSize {
    /// Size name, for example "M3".
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// Nominal diameter. Source: ISO 261.
    pub fn nominal(&self) -> f64 {
        self.nominal
    }

    /// Coarse thread pitch. Source: ISO 261.
    pub fn pitch(&self) -> f64 {
        self.pitch
    }

    /// Clearance hole diameter for a fit. Source: ISO 273.
    pub fn clearance(&self, fit: Fit) -> f64 {
        match fit {
            Fit::Fine => self.clearance[0],
            Fit::Medium => self.clearance[1],
            Fit::Coarse => self.clearance[2],
        }
    }

    /// Tap drill for a coarse thread: nominal minus pitch.
    pub fn tap_drill(&self) -> f64 {
        self.nominal - self.pitch
    }

    /// Socket head cap screw head. Source: ISO 4762.
    pub fn socket_head(&self) -> SocketHead {
        self.socket_head
    }

    /// Counterbore for a flush socket head, or None when no value is tabled.
    /// The depth is the head height k plus `COUNTERBORE_ALLOWANCE`.
    pub fn counterbore(&self) -> Option<Counterbore> {
        self.counterbore_diameter.map(|diameter| Counterbore {
            diameter,
            depth: self.socket_head.k + COUNTERBORE_ALLOWANCE,
        })
    }

    /// Countersink for an ISO 10642 head, or None when no value is tabled.
    pub fn countersink(&self) -> Option<Countersink> {
        self.countersink_head.map(|head_diameter| Countersink {
            head_diameter,
            angle: COUNTERSINK_ANGLE,
        })
    }
}

/// All metric sizes, in increasing order.
pub fn sizes() -> &'static [MetricSize] {
    &table::SIZES
}

/// Finds a size by name. The match ignores case, so "m3" finds "M3".
pub fn metric(name: &str) -> Option<&'static MetricSize> {
    table::SIZES
        .iter()
        .find(|size| size.name.eq_ignore_ascii_case(name))
}
