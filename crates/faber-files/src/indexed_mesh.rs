use faber_ports::{Mesh, Result};
use std::io::{BufWriter, Write};

/// Write indexed triangles with full-precision coordinates for an editable asset.
pub fn write_indexed_mesh(mesh: &Mesh, writer: impl Write) -> Result<()> {
    fn write(mesh: &Mesh, writer: impl Write) -> std::io::Result<()> {
        let mut out = BufWriter::new(writer);
        writeln!(
            out,
            "OFF\n{} {} 0",
            mesh.vertices.len(),
            mesh.triangles.len()
        )?;
        for &[x, y, z] in &mesh.vertices {
            writeln!(out, "{x} {y} {z}")?;
        }
        for &[a, b, c] in &mesh.triangles {
            writeln!(out, "3 {a} {b} {c}")?;
        }
        out.flush()
    }
    write(mesh, writer).map_err(|e| e.to_string())
}
