use crate::Move;
use faber_model::machining::{Operation, Tool};
use faber_ports::Result;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct StockBox {
    pub min: [f64; 3],
    pub max: [f64; 3],
}

impl StockBox {
    pub fn validate(&self) -> Result<()> {
        if (0..3).any(|i| {
            !self.min[i].is_finite() || !self.max[i].is_finite() || self.min[i] >= self.max[i]
        }) {
            return Err("The stock must have finite bounds and positive size.".into());
        }
        Ok(())
    }
}
pub fn validate_stock_depth(stock: &StockBox, bottom: f64) -> Result<()> {
    stock.validate()?;
    if !bottom.is_finite() {
        return Err("The bottom height must be finite.".into());
    }
    if bottom < stock.min[2] - 1e-8 {
        return Err(format!(
            "Bottom {bottom} mm is below the stock bottom {} mm.",
            stock.min[2]
        ));
    }
    Ok(())
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Toolpath {
    pub operation: String,
    pub tool: String,
    pub moves: Vec<Move>,
}
impl Toolpath {
    fn totals(&self) -> (f64, f64, f64) {
        let (mut cut, mut rapid, mut minutes) = (0., 0., 0.);
        let mut last = None;
        for m in &self.moves {
            match m {
                Move::Dwell { seconds } => minutes += seconds / 60.,
                Move::Rapid { to } | Move::Feed { to, .. } => {
                    if let Some(prev) = last {
                        let len = glam::DVec3::from(*to).distance(prev);
                        if let Move::Feed { feed, .. } = m {
                            cut += len;
                            minutes += len / feed;
                        } else {
                            rapid += len;
                        }
                    }
                    last = Some((*to).into());
                }
            }
        }
        (cut, rapid, minutes)
    }
    pub fn cut_length(&self) -> f64 {
        self.totals().0
    }
    pub fn rapid_length(&self) -> f64 {
        self.totals().1
    }
    pub fn minutes(&self, rapid_mm_min: f64) -> f64 {
        let (_, rapid, minutes) = self.totals();
        minutes + rapid / rapid_mm_min
    }
    pub fn bounds(&self) -> [[f64; 3]; 2] {
        let mut lo = glam::DVec3::splat(f64::INFINITY);
        let mut hi = -lo;
        for p in self.moves.iter().filter_map(Move::target) {
            lo = lo.min(p.into());
            hi = hi.max(p.into());
        }
        if !lo.is_finite() {
            return [[0.; 3]; 2];
        }
        [lo.to_array(), hi.to_array()]
    }
}

const MAX_MOVES: usize = 1_000_000;
pub(crate) fn passes(top: f64, bottom: f64, stepdown: Option<f64>) -> Result<Vec<f64>> {
    if !top.is_finite() || !bottom.is_finite() || top < bottom {
        return Err("The bottom must be at or below the top.".into());
    }
    let depth = top - bottom;
    let step = stepdown.unwrap_or(if depth > 0. { depth } else { 1. });
    if !step.is_finite() || step <= 0. {
        return Err("Stepdown must be positive and finite.".into());
    }
    let count = (depth / step).ceil().max(1.);
    if count > MAX_MOVES as f64 {
        return Err("The operation exceeds the depth pass limit.".into());
    }
    Ok((1..=count as usize)
        .map(|i| {
            if i == count as usize {
                bottom
            } else {
                (top - step * i as f64).max(bottom)
            }
        })
        .collect())
}

pub(crate) fn stepover(value: f64, diameter: f64) -> Result<()> {
    if !value.is_finite() || value <= 0. || value > diameter {
        return Err("Stepover must be positive and no greater than the tool diameter.".into());
    }
    Ok(())
}
pub(crate) fn allowance(v: f64) -> Result<()> {
    if !v.is_finite() || v < 0. {
        return Err("Stock to leave must be finite and non-negative.".into());
    }
    Ok(())
}
pub(crate) struct Writer<'a> {
    path: Toolpath,
    pub(crate) tool: &'a Tool,
    pub(crate) op: &'a Operation,
    last: Option<[f64; 3]>,
}
impl<'a> Writer<'a> {
    pub(crate) fn new(tool: &'a Tool, op: &'a Operation) -> Result<Self> {
        if [tool.diameter, tool.feed_mm_min, tool.plunge_mm_min]
            .iter()
            .any(|v| !v.is_finite() || *v <= 0.)
        {
            return Err("The tool diameter and feeds must be positive and finite.".into());
        }
        let h = &op.heights;
        if ![h.clearance, h.retract, h.top]
            .iter()
            .all(|v| v.is_finite())
            || h.clearance < h.retract
            || h.retract < h.top
        {
            return Err(
                "The clearance must be at or above retract, and retract at or above the top."
                    .into(),
            );
        }
        Ok(Self {
            tool,
            op,
            path: Toolpath {
                operation: op.id.clone(),
                tool: op.tool.clone(),
                moves: vec![],
            },
            last: None,
        })
    }
    pub(crate) fn add(&mut self, m: Move) -> Result<()> {
        if self.path.moves.len() >= MAX_MOVES {
            return Err(format!("The operation exceeds {MAX_MOVES} moves."));
        }
        if let Some(p) = m.target() {
            if !p.iter().all(|v| v.is_finite()) {
                return Err("The toolpath contains a non-finite point.".into());
            }
            if self.last == Some(p) {
                return Ok(());
            }
            self.last = Some(p);
        }
        self.path.moves.push(m);
        Ok(())
    }
    pub(crate) fn start(&mut self, xy: [f64; 2]) -> Result<()> {
        let h = &self.op.heights;
        // Move up before a change in XY. The first XY assumes the machine is already clear.
        if let Some(a) = self.last {
            self.add(Move::Rapid {
                to: [a[0], a[1], h.retract],
            })?;
            self.add(Move::Rapid {
                to: [xy[0], xy[1], h.retract],
            })
        } else {
            self.add(Move::Rapid {
                to: [xy[0], xy[1], h.clearance],
            })?;
            self.add(Move::Rapid {
                to: [xy[0], xy[1], h.retract],
            })
        }
    }
    pub(crate) fn feed(&mut self, xy: [f64; 2], z: f64, rate: f64) -> Result<()> {
        self.add(Move::Feed {
            to: [xy[0], xy[1], z],
            feed: rate,
        })
    }
    pub(crate) fn ring(&mut self, ring: &[[f64; 2]], z: f64) -> Result<()> {
        let a = *ring.first().ok_or("The path has no points.")?;
        self.start(a)?;
        self.feed(a, z, self.tool.plunge_mm_min)?;
        for &p in &ring[1..] {
            self.feed(p, z, self.tool.feed_mm_min)?;
        }
        self.feed(a, z, self.tool.feed_mm_min)
    }
    pub(crate) fn finish(mut self) -> Result<Toolpath> {
        let a = self.last.ok_or("The operation has no cutting moves.")?;
        self.add(Move::Rapid {
            to: [a[0], a[1], self.op.heights.clearance],
        })?;
        Ok(self.path)
    }
}
