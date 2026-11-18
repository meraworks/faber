use faber_files::FileAssets;
use faber_ports::{AssetStore, Mesh};

fn read(bytes: &[u8]) -> Result<Mesh, String> {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("input");
    std::fs::write(&source, bytes).unwrap();
    let store = FileAssets::new(root.path());
    let key = store.put(&source)?;
    store
        .read_mesh(&store.resolve(&key)?)?
        .ok_or("Not a mesh".into())
}

#[test]
fn obj_negative_indices_preserve_a_concave_polygon() {
    let mesh =
        read(b"# part\nv 0 0 0\nv 3 0 0\nv 3 3 0\nv 1 1 0\nv 0 3 0\nf -5 -4 -3 -2 -1\n").unwrap();
    assert_eq!(mesh.vertices.len(), 5);
    assert_eq!(mesh.triangles.len(), 3);
    let area: f64 = mesh
        .triangles
        .iter()
        .map(|f| {
            let [a, b, c] = f.map(|i| glam::DVec3::from(mesh.vertices[i as usize]));
            (b - a).cross(c - a).z / 2.
        })
        .sum();
    assert!((area - 6.).abs() < 1e-9);
    assert!(read(b"v 0 0 0\nf 1 2 3\n").is_err());
    assert!(read(b"v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1/nonsense 2//999999 3////\n").is_err());
}

#[test]
fn obj_vertex_attributes_are_numeric_and_have_supported_counts() {
    let rest = "\nv 1 0 0\nv 0 1 0\nf 1 2 3\n";
    for vertex in ["v 0 0 0 garbage garbage", "v 0 0 0 1 0 NaN", "v 0 0 0 1 2"] {
        assert!(
            read(format!("{vertex}{rest}").as_bytes()).is_err(),
            "Accepted {vertex}"
        );
    }
    assert!(read(format!("v 0 0 0 1 0 0{rest}").as_bytes()).is_ok());
}

#[test]
fn off_reads_polygon_faces_and_refuses_missing_vertices() {
    let mesh = read(b"OFF\n# square\n4 1 0\n0 0 0\n2 0 0\n2 2 0\n0 2 0\n4 0 1 2 3\n").unwrap();
    assert_eq!(mesh.triangles.len(), 2);
    assert_eq!(mesh.vertices[2], [2., 2., 0.]);
    assert!(read(b"OFF\n4 1 0\n0 0 0\n").is_err());
}

#[test]
fn ply_reads_ascii_and_both_binary_byte_orders() {
    let header = "ply\nformat ENCODING 1.0\nelement vertex 3\nproperty float x\nproperty float y\nproperty float z\nelement face 1\nproperty list uchar int vertex_indices\nend_header\n";
    let ascii = header.replace("ENCODING", "ascii") + "0 0 0\n2 0 0\n0 3 0\n3 0 1 2\n";
    assert_eq!(read(ascii.as_bytes()).unwrap().triangles, [[0, 1, 2]]);
    assert!(read((ascii.clone() + "junk").as_bytes()).is_err());
    for big in [false, true] {
        let encoding = if big {
            "binary_big_endian"
        } else {
            "binary_little_endian"
        };
        let mut bytes = header.replace("ENCODING", encoding).into_bytes();
        for v in [0f32, 0., 0., 2., 0., 0., 0., 3., 0.] {
            bytes.extend(if big {
                v.to_be_bytes()
            } else {
                v.to_le_bytes()
            });
        }
        bytes.push(3);
        for v in [0i32, 1, 2] {
            bytes.extend(if big {
                v.to_be_bytes()
            } else {
                v.to_le_bytes()
            });
        }
        let mesh = read(&bytes).unwrap();
        assert_eq!(mesh.vertices[2], [0., 3., 0.]);
        assert_eq!(mesh.triangles, [[0, 1, 2]]);
        let mut trailing = bytes.clone();
        trailing.push(1);
        assert!(read(&trailing).is_err());
        bytes.pop();
        assert!(read(&bytes).is_err());
    }
}

#[test]
fn ascii_stl_and_binary_header_named_solid_are_read() {
    let text = b"solid part\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 2 0 0\nvertex 0 3 0\nendloop\nendfacet\nendsolid part\n";
    assert_eq!(read(text).unwrap().triangles.len(), 1);
    let mut binary = vec![0; 80];
    binary[..5].copy_from_slice(b"solid");
    binary.extend(1u32.to_le_bytes());
    binary.extend([0; 12]);
    for n in [0f32, 0., 0., 2., 0., 0., 0., 3., 0.] {
        binary.extend(n.to_le_bytes());
    }
    binary.extend([0; 2]);
    assert_eq!(read(&binary).unwrap().vertices[2], [0., 3., 0.]);
    binary[..10].copy_from_slice(b"v example\n");
    assert_eq!(read(&binary).unwrap().vertices[2], [0., 3., 0.]);
    binary.pop();
    assert!(read(&binary).is_err());
}

fn package(model: &str) -> Vec<u8> {
    package_files(model, &[])
}

fn package_files(model: &str, files: &[(&str, &str)]) -> Vec<u8> {
    use std::io::{Cursor, Write};
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    zip.start_file("_rels/.rels", options).unwrap();
    zip.write_all(br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="root" Type="http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel" Target="/3D/part.model"/></Relationships>"#).unwrap();
    zip.start_file("3D/part.model", options).unwrap();
    zip.write_all(model.as_bytes()).unwrap();
    for (name, text) in files {
        zip.start_file(*name, options).unwrap();
        zip.write_all(text.as_bytes()).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

#[test]
fn three_mf_reads_production_components_and_refuses_missing_files() {
    let root = r#"<model xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02" xmlns:p="http://schemas.microsoft.com/3dmanufacturing/production/2015/06" requiredextensions="p"><resources><object id="2"><components><component p:path="/3D/Objects/mesh.model" objectid="1" transform="1 0 0 0 1 0 0 0 1 5 0 0"/></components></object></resources><build><item objectid="2"/></build></model>"#;
    let part = r#"<model xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02"><resources><object id="1"><mesh><vertices><vertex x="0" y="0" z="0"/><vertex x="1" y="0" z="0"/><vertex x="0" y="1" z="0"/></vertices><triangles><triangle v1="0" v2="1" v3="2"/></triangles></mesh></object></resources></model>"#;
    let mesh = read(&package_files(root, &[("3D/Objects/mesh.model", part)])).unwrap();
    assert_eq!(mesh.vertices, [[5., 0., 0.], [6., 0., 0.], [5., 1., 0.]]);
    assert!(read(&package(root)).is_err());
    let empty = r#"<model xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02"><resources><object id="1"><components/></object></resources><build><item objectid="1"/></build></model>"#;
    assert!(read(&package(empty)).unwrap_err().contains("empty"));
}

#[test]
fn three_mf_uses_build_items_component_transforms_and_units() {
    let model = r#"<model xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02" unit="centimeter"><resources>
      <object id="1" type="model"><mesh><vertices><vertex x="0" y="0" z="0"/><vertex x="2" y="0" z="0"/><vertex x="0" y="3" z="0"/></vertices><triangles><triangle v1="0" v2="1" v3="2"/></triangles></mesh></object>
      <object id="2" type="model"><components><component objectid="1" transform="1 0 0 0 1 0 0 0 1 1 0 0"/></components></object>
      </resources><build><item objectid="2" transform="1 0 0 0 1 0 0 0 1 0 2 0"/></build></model>"#;
    let mesh = read(&package(model)).unwrap();
    assert_eq!(
        mesh.vertices,
        [[10., 20., 0.], [30., 20., 0.], [10., 50., 0.]]
    );
    assert_eq!(mesh.triangles, [[0, 1, 2]]);
    assert!(read(&package(&model.replace("objectid=\"1\"", "objectid=\"2\""))).is_err());
    assert!(read(&package(&model.replace("v3=\"2\"", "v3=\"99\""))).is_err());
    assert!(
        read(&package(
            &model.replace("unit=\"centimeter\"", "unit=\"unknown\"")
        ))
        .is_err()
    );
    let extended = model.replace(
        "unit=\"centimeter\"",
        "unit=\"centimeter\" xmlns:q=\"urn:unknown\" requiredextensions=\"q\"",
    );
    assert!(read(&package(&extended)).is_err());
}
