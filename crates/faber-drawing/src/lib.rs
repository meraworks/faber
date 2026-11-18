//! Technical drawings in sheet millimetres.
mod auto;
mod clip;
mod dimension;
mod hatch;
mod leader;
mod sheet;
mod view;

pub use auto::auto_drawing;
pub use dimension::format_length;
use faber_model::drawing::{Anchor, Drawing, ViewKind};
use faber_ports::{
    Result,
    drawing::{Projection2, ViewFrame},
    vector::{Path2, VectorDoc},
};
use glam::DVec2;
use std::collections::{BTreeMap, BTreeSet};
pub use view::frame;

#[derive(Clone, Debug, PartialEq)]
pub enum Resolved {
    Point([f64; 3]),
    Line([[f64; 3]; 2]),
    Circle {
        centre: [f64; 3],
        axis: [f64; 3],
        radius: f64,
    },
}
#[derive(Clone, Debug, PartialEq)]
pub struct PartRow {
    pub quantity: usize,
    pub name: String,
    pub material: String,
    pub mass_g: Option<f64>,
}
pub trait ModelSource {
    fn project(&self, parts: &[String], frame: &ViewFrame, hidden: bool) -> Result<Projection2>;
    fn section(
        &self,
        parts: &[String],
        origin: [f64; 3],
        normal: [f64; 3],
    ) -> Result<Vec<Vec<[f64; 2]>>>;
    fn resolve(&self, anchor: &Anchor) -> Result<Resolved>;
    fn bounds(&self, parts: &[String]) -> Result<[[f64; 3]; 2]>;
    /// Return the projection of the material behind the cutting plane.
    fn cut_view(
        &self,
        _parts: &[String],
        _frame: &ViewFrame,
        _hidden: bool,
    ) -> Result<Projection2> {
        Ok(Projection2::default())
    }
    /// Express each closed cut loop in the supplied frame.
    fn section_in_frame(&self, parts: &[String], frame: &ViewFrame) -> Result<Vec<Vec<[f64; 2]>>> {
        self.section(parts, frame.origin, view::normal(frame).to_array())
    }
    fn anchors(&self, _parts: &[String]) -> Result<Vec<(Anchor, Resolved)>> {
        Ok(Vec::new())
    }
    fn parts_list(&self, _parts: &[String]) -> Result<Vec<PartRow>> {
        Ok(Vec::new())
    }
    fn limits(&self, _parts: &[String]) -> Vec<String> {
        Vec::new()
    }
}

#[derive(Debug, Clone)]
pub struct Rendered {
    pub doc: VectorDoc,
    pub problems: Vec<String>,
    pub limits: Vec<String>,
}

pub fn render(drawing: &Drawing, model: &dyn ModelSource) -> Result<Rendered> {
    drawing.validate()?;
    let mut doc = sheet::sheet(drawing, model)?;
    let mut views = BTreeMap::new();
    let mut problems = Vec::new();
    let mut limits = BTreeSet::new();
    let cut_names: BTreeMap<_, _> = drawing
        .views
        .iter()
        .filter(|(_, v)| matches!(v.kind, ViewKind::Section { .. }))
        .enumerate()
        .map(|(i, (id, _))| (id.as_str(), section_letter(i)))
        .collect();
    for id in drawing.views.keys() {
        // Visit parents first. Deep view chains do not use the call stack.
        let mut chain = Vec::new();
        let mut next = id.as_str();
        while !views.contains_key(next) {
            chain.push(next);
            let Some(parent) = drawing.views[next].kind.parent() else {
                break;
            };
            next = parent;
        }
        for name in chain.into_iter().rev() {
            let v = &drawing.views[name];
            let parent = v.kind.parent().and_then(|p| views.get(p));
            let layout = view::build(v, drawing, parent, model)?;
            if let ViewKind::Base { parts, .. } = &v.kind {
                limits.extend(model.limits(parts));
            }
            let [w, h] = layout.size().to_array();
            let [sw, sh] = doc.size_mm;
            if w > sw + 1e-7 || h > sh + 1e-7 {
                problems.push(format!(
                    "View {name} is {} x {} mm at scale {}. The sheet is {} x {} mm.",
                    format_length(w),
                    format_length(h),
                    format_length(layout.scale),
                    format_length(sw),
                    format_length(sh)
                ));
            }
            layout.draw(&mut doc)?;
            if let (Some(parent), ViewKind::Section { line, .. }) = (parent, &v.kind) {
                sheet::cut_line(
                    &mut doc,
                    parent.sheet(line[0].into()),
                    parent.sheet(line[1].into()),
                    &cut_names[name],
                );
            }
            let label = v
                .label
                .clone()
                .or_else(|| cut_names.get(name).map(|l| format!("{l}-{l}")));
            if let Some(label) = label {
                sheet::text(
                    &mut doc,
                    "TEXT",
                    layout.at - DVec2::Y * (h / 2. + 5.),
                    label,
                    3.,
                );
            }
            views.insert(name, layout);
        }
    }
    let mut obstacles: Vec<_> = views.values().map(|v| (v.at, v.size() / 2.)).collect();
    let (a, b) = sheet::title_bounds(drawing);
    obstacles.push(((a + b) / 2., (b - a) / 2.));
    let is_group = |d: &faber_model::drawing::Dimension| {
        matches!(
            d.kind,
            faber_model::drawing::DimensionKind::DiameterGroup { .. }
        )
    };
    let dimensions = drawing.dimensions.iter();
    // Place groups after the fixed dimensions so that their leaders can use free paper.
    for (id, d) in dimensions
        .clone()
        .filter(|(_, d)| !is_group(d))
        .chain(dimensions.filter(|(_, d)| is_group(d)))
    {
        let layout = &views[d.view.as_str()];
        if let Err(e) = dimension::draw(&mut doc, d, layout, model, &obstacles) {
            let at = layout.at + DVec2::splat(d.offset);
            sheet::line(
                &mut doc,
                "BROKEN",
                at - DVec2::splat(2.),
                at + DVec2::splat(2.),
            );
            sheet::text(&mut doc, "BROKEN", at + DVec2::Y * 3., "?".into(), 3.);
            let reason = e
                .strip_prefix("The ")
                .map(|s| format!("the {s}"))
                .unwrap_or(e);
            problems.push(format!("Dimension {id}: {reason}"));
        }
    }
    Ok(Rendered {
        doc,
        problems,
        limits: limits.into_iter().collect(),
    })
}
fn section_letter(mut index: usize) -> String {
    let mut s = Vec::new();
    loop {
        s.push((b'A' + (index % 26) as u8) as char);
        if index < 26 {
            break;
        }
        index = index / 26 - 1;
    }
    s.into_iter().rev().collect()
}
fn path(points: impl IntoIterator<Item = DVec2>, closed: bool) -> Path2 {
    Path2::Polyline {
        points: points.into_iter().map(|p| p.to_array()).collect(),
        closed,
    }
}
