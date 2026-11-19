use faber_model::{Scalar, SketchConstraint, SketchEntity, SolvedSketchData};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Pick {
    Point(String),
    Entity(String),
}
impl Pick {
    pub(super) fn id(&self) -> &str {
        match self {
            Self::Point(id) | Self::Entity(id) => id,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Operand {
    Point,
    Line,
    Round,
    Entity,
}

impl Operand {
    pub fn label(self) -> &'static str {
        match self {
            Self::Point => "point",
            Self::Line => "line",
            Self::Round => "circle or arc",
            Self::Entity => "entity",
        }
    }
}
use Operand::{Entity, Line, Point, Round};
pub(super) struct Rule {
    pub label: &'static str,
    pub kind: &'static str,
    pub inputs: &'static [(&'static str, Operand)],
    pub dimension: Option<&'static str>,
}
pub(super) const RULES: &[Rule] = &[
    rule(
        "Coincident",
        "coincident",
        &[("a", Point), ("b", Point)],
        None,
    ),
    rule(
        "Point on line",
        "point_on_line",
        &[("point", Point), ("line", Line)],
        None,
    ),
    rule(
        "Point on circle",
        "point_on_circle",
        &[("point", Point), ("circle", Round)],
        None,
    ),
    rule(
        "Midpoint",
        "midpoint",
        &[("point", Point), ("line", Line)],
        None,
    ),
    rule("Horizontal", "horizontal", &[("line", Line)], None),
    rule("Vertical", "vertical", &[("line", Line)], None),
    rule("Parallel", "parallel", &[("a", Line), ("b", Line)], None),
    rule(
        "Perpendicular",
        "perpendicular",
        &[("a", Line), ("b", Line)],
        None,
    ),
    rule("Collinear", "collinear", &[("a", Line), ("b", Line)], None),
    rule("Tangent", "tangent", &[("a", Entity), ("b", Entity)], None),
    rule("Equal", "equal", &[("a", Entity), ("b", Entity)], None),
    rule(
        "Concentric",
        "concentric",
        &[("a", Round), ("b", Round)],
        None,
    ),
    rule(
        "Symmetric",
        "symmetric",
        &[("a", Point), ("b", Point), ("about", Line)],
        None,
    ),
    rule("Fix", "fix", &[("point", Point)], None),
    rule(
        "Distance",
        "distance",
        &[("a", Point), ("b", Point)],
        Some("value"),
    ),
    rule(
        "Horizontal distance",
        "distance_x",
        &[("a", Point), ("b", Point)],
        Some("value"),
    ),
    rule(
        "Vertical distance",
        "distance_y",
        &[("a", Point), ("b", Point)],
        Some("value"),
    ),
    rule(
        "Point to line distance",
        "point_line_distance",
        &[("point", Point), ("line", Line)],
        Some("value"),
    ),
    rule(
        "Angle",
        "angle",
        &[("a", Line), ("b", Line)],
        Some("degrees"),
    ),
    rule("Radius", "radius", &[("entity", Round)], Some("value")),
    rule("Diameter", "diameter", &[("entity", Round)], Some("value")),
    rule("Length", "length", &[("line", Line)], Some("value")),
];
const fn rule(
    label: &'static str,
    kind: &'static str,
    inputs: &'static [(&'static str, Operand)],
    dimension: Option<&'static str>,
) -> Rule {
    Rule {
        label,
        kind,
        inputs,
        dimension,
    }
}

impl Rule {
    pub fn bind(
        &self,
        picks: &[Pick],
        sketch: &SolvedSketchData<Scalar>,
    ) -> Option<Map<String, Value>> {
        if picks.len() != self.inputs.len() {
            return None;
        }
        let mut used = vec![false; picks.len()];
        let mut obj = Map::from_iter([("kind".into(), json!(self.kind))]);
        for &(key, want) in self.inputs {
            let i = picks.iter().enumerate().position(|(i, p)| {
                !used[i]
                    && match p {
                        Pick::Point(_) => want == Point,
                        Pick::Entity(id) => sketch.entities.get(id).is_some_and(|e| {
                            want == Entity
                                || match e {
                                    SketchEntity::Line { .. } => want == Line,
                                    _ => want == Round,
                                }
                        }),
                    }
            })?;
            used[i] = true;
            obj.insert(key.into(), json!(picks[i].id()));
        }
        if self.kind == "tangent"
            && picks
                .iter()
                .all(|p| matches!(sketch.entities.get(p.id()), Some(SketchEntity::Line { .. })))
        {
            return None;
        }
        Some(obj)
    }
    pub fn make(
        &self,
        mut obj: Map<String, Value>,
        input: &str,
    ) -> Result<SketchConstraint<Scalar>, String> {
        if let Some(key) = self.dimension {
            obj.insert(
                key.into(),
                serde_json::to_value(scalar(input)?).map_err(|e| e.to_string())?,
            );
        }
        serde_json::from_value(Value::Object(obj)).map_err(|e| e.to_string())
    }
    pub fn operands(&self, object: &Value) -> Vec<Pick> {
        self.inputs
            .iter()
            .filter_map(|&(key, k)| {
                object.get(key)?.as_str().map(|id| {
                    if k == Point {
                        Pick::Point(id.into())
                    } else {
                        Pick::Entity(id.into())
                    }
                })
            })
            .collect()
    }
}

pub(super) struct Label {
    pub rule: &'static Rule,
    pub picks: Vec<Pick>,
    pub value: Option<String>,
}
impl Label {
    pub fn new(c: &SketchConstraint<Scalar>) -> Self {
        let obj = json!(c);
        let rule = RULES
            .iter()
            .find(|r| obj["kind"] == r.kind)
            .expect("Every sketch constraint has a control");
        let value = rule.dimension.map(|key| match &obj[key] {
            Value::String(v) => v.clone(),
            n => n.to_string(),
        });
        Self {
            rule,
            picks: rule.operands(&obj),
            value,
        }
    }
}

pub(super) fn scalar(s: &str) -> Result<Scalar, String> {
    let s = s.trim();
    if s.is_empty() {
        return Err("Enter a dimension or a parameter expression.".into());
    }
    Ok(s.parse()
        .map(Scalar::Number)
        .unwrap_or_else(|_| Scalar::Expression(s.into())))
}
pub(super) fn number(s: &Scalar, params: &BTreeMap<String, f64>) -> Result<f64, String> {
    match s {
        Scalar::Number(v) => Ok(*v),
        Scalar::Expression(e) => faber_expr::evaluate(e, |id| {
            params
                .get(id)
                .copied()
                .ok_or_else(|| format!("Unknown parameter: {id}"))
        }),
    }
}
