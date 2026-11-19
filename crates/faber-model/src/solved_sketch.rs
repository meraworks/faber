//! Planar geometry and its dimensions.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, bound(deserialize = "N: Deserialize<'de>"))]
pub struct SolvedSketchData<N = f64> {
    pub points: BTreeMap<String, [N; 2]>,
    pub entities: BTreeMap<String, SketchEntity<N>>,
    #[serde(default)]
    pub constraints: Vec<SketchConstraint<N>>,
    #[serde(default)]
    pub construction: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SketchEntity<N = f64> {
    Line {
        a: String,
        b: String,
    },
    Circle {
        centre: String,
        radius: N,
    },
    Arc {
        centre: String,
        start: String,
        end: String,
    },
}
impl<N> SketchEntity<N> {
    pub fn map_numbers<T>(
        &self,
        map: &mut impl FnMut(&N) -> Result<T, String>,
    ) -> Result<SketchEntity<T>, String> {
        Ok(match self {
            Self::Line { a, b } => SketchEntity::Line {
                a: a.clone(),
                b: b.clone(),
            },
            Self::Circle { centre, radius } => SketchEntity::Circle {
                centre: centre.clone(),
                radius: map(radius)?,
            },
            Self::Arc { centre, start, end } => SketchEntity::Arc {
                centre: centre.clone(),
                start: start.clone(),
                end: end.clone(),
            },
        })
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SketchConstraint<N = f64> {
    Coincident {
        a: String,
        b: String,
    },
    PointOnLine {
        point: String,
        line: String,
    },
    PointOnCircle {
        point: String,
        circle: String,
    },
    Midpoint {
        point: String,
        line: String,
    },
    Horizontal {
        line: String,
    },
    Vertical {
        line: String,
    },
    Parallel {
        a: String,
        b: String,
    },
    Perpendicular {
        a: String,
        b: String,
    },
    Collinear {
        a: String,
        b: String,
    },
    Tangent {
        a: String,
        b: String,
    },
    Equal {
        a: String,
        b: String,
    },
    Concentric {
        a: String,
        b: String,
    },
    Symmetric {
        a: String,
        b: String,
        about: String,
    },
    Fix {
        point: String,
    },
    Distance {
        a: String,
        b: String,
        value: N,
    },
    DistanceX {
        a: String,
        b: String,
        value: N,
    },
    DistanceY {
        a: String,
        b: String,
        value: N,
    },
    PointLineDistance {
        point: String,
        line: String,
        value: N,
    },
    Angle {
        a: String,
        b: String,
        degrees: N,
    },
    Radius {
        entity: String,
        value: N,
    },
    Diameter {
        entity: String,
        value: N,
    },
    Length {
        line: String,
        value: N,
    },
}
impl<N> SketchConstraint<N> {
    pub fn map_numbers<T>(
        &self,
        map: &mut impl FnMut(&N) -> Result<T, String>,
    ) -> Result<SketchConstraint<T>, String> {
        Ok(match self {
            Self::Coincident { a, b } => SketchConstraint::Coincident {
                a: a.clone(),
                b: b.clone(),
            },
            Self::PointOnLine { point, line } => SketchConstraint::PointOnLine {
                point: point.clone(),
                line: line.clone(),
            },
            Self::PointOnCircle { point, circle } => SketchConstraint::PointOnCircle {
                point: point.clone(),
                circle: circle.clone(),
            },
            Self::Midpoint { point, line } => SketchConstraint::Midpoint {
                point: point.clone(),
                line: line.clone(),
            },
            Self::Horizontal { line } => SketchConstraint::Horizontal { line: line.clone() },
            Self::Vertical { line } => SketchConstraint::Vertical { line: line.clone() },
            Self::Parallel { a, b } => SketchConstraint::Parallel {
                a: a.clone(),
                b: b.clone(),
            },
            Self::Perpendicular { a, b } => SketchConstraint::Perpendicular {
                a: a.clone(),
                b: b.clone(),
            },
            Self::Collinear { a, b } => SketchConstraint::Collinear {
                a: a.clone(),
                b: b.clone(),
            },
            Self::Tangent { a, b } => SketchConstraint::Tangent {
                a: a.clone(),
                b: b.clone(),
            },
            Self::Equal { a, b } => SketchConstraint::Equal {
                a: a.clone(),
                b: b.clone(),
            },
            Self::Concentric { a, b } => SketchConstraint::Concentric {
                a: a.clone(),
                b: b.clone(),
            },
            Self::Symmetric { a, b, about } => SketchConstraint::Symmetric {
                a: a.clone(),
                b: b.clone(),
                about: about.clone(),
            },
            Self::Fix { point } => SketchConstraint::Fix {
                point: point.clone(),
            },
            Self::Distance { a, b, value } => SketchConstraint::Distance {
                a: a.clone(),
                b: b.clone(),
                value: map(value)?,
            },
            Self::DistanceX { a, b, value } => SketchConstraint::DistanceX {
                a: a.clone(),
                b: b.clone(),
                value: map(value)?,
            },
            Self::DistanceY { a, b, value } => SketchConstraint::DistanceY {
                a: a.clone(),
                b: b.clone(),
                value: map(value)?,
            },
            Self::PointLineDistance { point, line, value } => SketchConstraint::PointLineDistance {
                point: point.clone(),
                line: line.clone(),
                value: map(value)?,
            },
            Self::Angle { a, b, degrees } => SketchConstraint::Angle {
                a: a.clone(),
                b: b.clone(),
                degrees: map(degrees)?,
            },
            Self::Radius { entity, value } => SketchConstraint::Radius {
                entity: entity.clone(),
                value: map(value)?,
            },
            Self::Diameter { entity, value } => SketchConstraint::Diameter {
                entity: entity.clone(),
                value: map(value)?,
            },
            Self::Length { line, value } => SketchConstraint::Length {
                line: line.clone(),
                value: map(value)?,
            },
        })
    }
}
impl<N> SolvedSketchData<N> {
    pub fn map_numbers<T>(
        &self,
        map: &mut impl FnMut(&N) -> Result<T, String>,
    ) -> Result<SolvedSketchData<T>, String> {
        Ok(SolvedSketchData {
            points: self
                .points
                .iter()
                .map(|(id, [x, y])| Ok((id.clone(), [map(x)?, map(y)?])))
                .collect::<Result<_, String>>()?,
            entities: self
                .entities
                .iter()
                .map(|(id, v)| Ok((id.clone(), v.map_numbers(map)?)))
                .collect::<Result<_, String>>()?,
            constraints: self
                .constraints
                .iter()
                .map(|v| v.map_numbers(map))
                .collect::<Result<_, _>>()?,
            construction: self.construction.clone(),
        })
    }
}
