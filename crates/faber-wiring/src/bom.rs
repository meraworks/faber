use faber_core::Project;
pub use faber_core::bom::{BomRow, BomSource};
use faber_ports::{Result, bom::BillWriter};

pub fn report(project: &Project, scope: Option<&str>, flat: bool) -> Result<Vec<BomRow>> {
    if let Some(id) = scope
        && !project.data.components.contains_key(id)
    {
        return Err(format!("Unknown component: {id}."));
    }
    project.cached("bom", &format!("{scope:?}/{flat}"), || {
        Ok(faber_core::bom::bom(
            &project.data,
            &project.bodies,
            scope,
            flat,
        ))
    })
}
pub fn csv(rows: &[BomRow]) -> String {
    faber_files::bom::CsvBill.write(rows)
}
pub fn markdown(rows: &[BomRow]) -> String {
    faber_files::bom::MarkdownBill.write(rows)
}
