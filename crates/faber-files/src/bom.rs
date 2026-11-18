//! Text forms of a bill of materials.
use faber_ports::bom::{BillWriter, BomRow, BomSource};

pub struct CsvBill;
pub struct MarkdownBill;
const HEAD: [&str; 10] = [
    "Item",
    "Quantity",
    "Name",
    "Parts",
    "Material",
    "Mass (g)",
    "Designation",
    "Source",
    "Licence",
    "Component",
];
fn fields(r: &BomRow) -> [String; 10] {
    let (designation, source, licence) = match &r.source {
        BomSource::Made => (String::new(), "Made".to_owned(), String::new()),
        BomSource::Purchased {
            designation,
            source,
            licence,
        } => (designation.clone(), source.clone(), licence.clone()),
    };
    [
        r.item.to_string(),
        r.quantity.to_string(),
        r.name.clone(),
        r.parts.join(", "),
        r.material.clone().unwrap_or_default(),
        r.mass_g.map(|n| format!("{n:.3}")).unwrap_or_default(),
        designation,
        source,
        licence,
        r.component.clone().unwrap_or_default(),
    ]
}
impl BillWriter for CsvBill {
    fn write(&self, rows: &[BomRow]) -> String {
        let mut out = HEAD.join(",");
        out.push_str("\r\n");
        for r in rows {
            let fields = fields(r).map(|v| {
                if v.contains([',', '"', '\r', '\n']) {
                    format!("\"{}\"", v.replace('"', "\"\""))
                } else {
                    v
                }
            });
            out.push_str(&fields.join(","));
            out.push_str("\r\n");
        }
        out
    }
}
impl BillWriter for MarkdownBill {
    fn write(&self, rows: &[BomRow]) -> String {
        let mut out = format!(
            "| {} |\n| {} |\n",
            HEAD.join(" | "),
            ["---"; 10].join(" | ")
        );
        for r in rows {
            let fields = fields(r).map(|v| {
                v.replace('\\', "\\\\")
                    .replace('|', "\\|")
                    .replace(['\r', '\n'], " ")
            });
            out.push_str("| ");
            out.push_str(&fields.join(" | "));
            out.push_str(" |\n");
        }
        out
    }
}
