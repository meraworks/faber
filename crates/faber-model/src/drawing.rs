//! Drawing references and sheet coordinates in millimetres.
use crate::{EdgeSelector, FaceSelector};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Drawing {
    pub title: String,
    #[serde(default)]
    pub sheet: Sheet,
    #[serde(default)]
    pub projection: Projection,
    #[serde(default = "one")]
    pub scale: f64,
    #[serde(default)]
    pub views: BTreeMap<String, View>,
    #[serde(default)]
    pub dimensions: BTreeMap<String, Dimension>,
    #[serde(default)]
    pub notes: Vec<Note>,
    #[serde(default)]
    pub title_block: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parts_list: Option<PartsList>,
}
fn one() -> f64 {
    1.
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Sheet {
    pub size: SheetSize,
    pub orientation: Orientation,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum SheetSize {
    A1,
    A2,
    A3,
    #[default]
    A4,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Orientation {
    #[default]
    Landscape,
    Portrait,
}
impl Sheet {
    pub fn size_mm(self) -> [f64; 2] {
        let [w, h] = match self.size {
            SheetSize::A1 => [841., 594.],
            SheetSize::A2 => [594., 420.],
            SheetSize::A3 => [420., 297.],
            SheetSize::A4 => [297., 210.],
        };
        if self.orientation == Orientation::Landscape {
            [w, h]
        } else {
            [h, w]
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Projection {
    First,
    #[default]
    Third,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Left,
    Right,
    Top,
    Bottom,
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Direction {
    Front,
    Back,
    Left,
    Right,
    Top,
    Bottom,
    Iso,
    Custom { view: [f64; 3], up: [f64; 3] },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct View {
    pub at: [f64; 2],
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scale: Option<f64>,
    #[serde(default)]
    pub hidden_lines: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(flatten)]
    pub kind: ViewKind,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ViewKind {
    Base {
        #[serde(default)]
        parts: Vec<String>,
        direction: Direction,
    },
    Projected {
        from: String,
        side: Side,
    },
    Section {
        from: String,
        line: [[f64; 2]; 2],
    },
    Detail {
        from: String,
        centre: [f64; 2],
        radius: f64,
    },
}
impl ViewKind {
    pub fn parent(&self) -> Option<&str> {
        match self {
            Self::Base { .. } => None,
            Self::Projected { from, .. }
            | Self::Section { from, .. }
            | Self::Detail { from, .. } => Some(from),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Dimension {
    pub view: String,
    #[serde(default)]
    pub offset: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(flatten)]
    pub kind: DimensionKind,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Along {
    Horizontal,
    Vertical,
    Aligned,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DimensionKind {
    Linear { a: Anchor, b: Anchor, along: Along },
    Diameter { edge: Anchor },
    Radius { edge: Anchor },
    Angle { a: Anchor, b: Anchor },
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Anchor {
    Vertex {
        part: String,
        near: [f64; 3],
    },
    Edge {
        part: String,
        edge: Box<EdgeSelector>,
    },
    CylinderAxis {
        part: String,
        face: Box<FaceSelector>,
    },
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Note {
    pub at: [f64; 2],
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height_mm: Option<f64>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PartsList {
    pub at: [f64; 2],
    #[serde(default)]
    pub parts: Vec<String>,
}

impl Drawing {
    pub fn validate(&self) -> Result<(), String> {
        let positive = |v: f64| v.is_finite() && v > 0.;
        let point = |p: [f64; 2]| p.into_iter().all(f64::is_finite);
        if !positive(self.scale) {
            return Err("The drawing scale must be positive and finite.".into());
        }
        let mut checked = BTreeSet::new();
        for (id, v) in &self.views {
            if !point(v.at) || v.scale.is_some_and(|s| !positive(s)) {
                return Err(format!("Check the position and scale of view {id}."));
            }
            match &v.kind {
                ViewKind::Section { line, .. }
                    if line[0] == line[1] || !line.iter().copied().all(point) =>
                {
                    return Err(format!("View {id} needs two different section points."));
                }
                ViewKind::Detail { radius, centre, .. }
                    if !positive(*radius) || !point(*centre) =>
                {
                    return Err(format!("Check the detail circle of view {id}."));
                }
                ViewKind::Base {
                    direction: Direction::Custom { view, up },
                    ..
                } => {
                    let cross = [
                        view[1] * up[2] - view[2] * up[1],
                        view[2] * up[0] - view[0] * up[2],
                        view[0] * up[1] - view[1] * up[0],
                    ];
                    if cross.iter().any(|v| !v.is_finite()) || cross == [0.; 3] {
                        return Err(format!("View {id} needs two different axis directions."));
                    }
                }
                _ => {}
            }
            let mut path = BTreeSet::new();
            let mut next = id.as_str();
            while !checked.contains(next) {
                if !path.insert(next) {
                    return Err(format!("Drawing view cycle at {next}."));
                }
                let parent = self
                    .views
                    .get(next)
                    .ok_or_else(|| format!("Unknown drawing view: {next}"))?
                    .kind
                    .parent();
                let Some(parent) = parent else {
                    break;
                };
                next = parent;
            }
            checked.extend(path);
        }
        for (id, d) in &self.dimensions {
            if !self.views.contains_key(&d.view) || !d.offset.is_finite() {
                return Err(format!("Check the view and offset of dimension {id}."));
            }
        }
        if self
            .notes
            .iter()
            .any(|n| !point(n.at) || n.height_mm.is_some_and(|h| !positive(h)))
            || self.parts_list.as_ref().is_some_and(|p| !point(p.at))
        {
            return Err(
                "Check the position of the notes and parts list and the text height.".into(),
            );
        }
        Ok(())
    }
}
