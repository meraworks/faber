//! Build volumes in mm. The generic profile and process limits are Faber practice values.
use faber_model::Printer;

// Sources: Bambu Lab A1 and A1 mini specifications, Prusa MK4 FAQ, Creality Ender-3 FAQ.
// Links and the basis of the fit allowances are in the Print check section of the guide.
const PROFILES: &[(&str, &str, [f64; 3])] = &[
    ("generic-220", "Generic 220", [220., 220., 250.]),
    ("bambu-a1", "Bambu Lab A1", [256.; 3]),
    ("bambu-a1-mini", "Bambu Lab A1 mini", [180.; 3]),
    ("prusa-mk4", "Original Prusa MK4", [250., 210., 220.]),
    ("ender-3", "Creality Ender-3", [220., 220., 250.]),
];

pub fn profiles() -> impl Iterator<Item = (&'static str, Printer)> {
    PROFILES.iter().map(|&(id, name, bed_mm)| {
        (
            id,
            Printer {
                name: name.into(),
                bed_mm,
                ..Printer::default()
            },
        )
    })
}
pub fn profile(id: &str) -> Option<Printer> {
    profiles()
        .find(|(key, _)| *key == id)
        .map(|(_, printer)| printer)
}
