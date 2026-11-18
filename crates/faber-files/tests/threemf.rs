use faber_files::{FileAssets, ThreeMf};
use faber_ports::{AssetStore, Mesh, MeshAssemblyExport, MeshItem};
use std::io::Read;
use std::path::Path;

#[test]
fn weld_keeps_first_referenced_point_and_skips_unused_vertices() {
    let mesh = Mesh {
        vertices: vec![
            [1e15; 3],
            [2., 0., 0.],
            [3., 0., 0.],
            [2., 1., 0.],
            [2. + 1e-10, 0., 0.],
        ],
        triangles: vec![[1, 2, 3], [4, 3, 2]],
        normals: vec![],
    };
    let (vertices, indices) = mesh.welded_vertices(1e-6).unwrap();
    assert_eq!(vertices.as_slice(), &mesh.vertices[1..4]);
    assert_eq!(indices, [u32::MAX, 0, 1, 2, 0]);
}

fn fixture(path: &Path) {
    let mesh = Mesh {
        vertices: vec![[0., 0., 0.], [1., 0., 0.], [0., 2., 0.], [0., 0., 1.]],
        triangles: vec![[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]],
        normals: vec![],
    };
    let first = MeshItem {
        name: "One & two",
        colour: [16, 32, 48],
        mesh: &mesh,
        transform: [1., 0., 0., 0., 1., 0., 0., 0., 1., 0., 0., 0.],
    };
    let second = MeshItem {
        name: "Next",
        colour: [64, 80, 96],
        mesh: &mesh,
        transform: [0., 1., 0., -1., 0., 0., 0., 0., 1., 10., 20., 30.],
    };
    ThreeMf.write_assembly(&[first, second], path).unwrap();
}
fn text(path: &Path, entry: &str) -> String {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    let mut result = String::new();
    zip.by_name(entry)
        .unwrap()
        .read_to_string(&mut result)
        .unwrap();
    result
}

#[test]
fn threemf_has_one_object_for_each_part() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("plate.3mf");
    fixture(&path);
    let xml = text(&path, "3D/3dmodel.model");
    let model = roxmltree::Document::parse(&xml).unwrap();
    assert_eq!(model.root_element().attribute("unit"), Some("millimeter"));
    let nodes = |name| model.descendants().filter(move |n| n.has_tag_name(name));
    assert_eq!(nodes("object").count(), 2);
    assert_eq!(nodes("vertex").count(), 8);
    assert_eq!(nodes("triangle").count(), 8);
    assert_eq!(nodes("base").count(), 2);
    assert_eq!(
        nodes("object").next().unwrap().attribute("name"),
        Some("One & two")
    );
    assert_eq!(nodes("item").count(), 2);
    for item in nodes("item") {
        assert_eq!(
            item.attribute("transform")
                .unwrap()
                .split_whitespace()
                .count(),
            12
        );
    }
    let mesh = FileAssets::new(dir.path())
        .read_mesh(&path)
        .unwrap()
        .unwrap();
    assert_eq!(mesh.vertices[5], [10., 21., 30.]);
    assert_eq!(mesh.vertices[6], [8., 20., 30.]);
}

#[test]
fn threemf_has_the_content_types_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("assembly.3mf");
    fixture(&path);
    assert!(
        text(&path, "[Content_Types].xml")
            .contains("application/vnd.ms-package.3dmanufacturing-3dmodel+xml")
    );
    assert!(text(&path, "_rels/.rels").contains("/3D/3dmodel.model"));
}

#[test]
fn triangle_soup_exports_as_one_closed_solid() {
    let mut mesh = Mesh::default();
    let p = [[0., 0., 0.], [1., 0., 0.], [0., 2., 0.], [0., 0., 1.]];
    for tri in [[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]] {
        let i = mesh.vertices.len() as u32;
        mesh.vertices.extend(tri.map(|j| p[j]));
        mesh.triangles.push([i, i + 1, i + 2]);
    }
    let item = MeshItem {
        name: "Sheet",
        colour: [0; 3],
        mesh: &mesh,
        transform: [1., 0., 0., 0., 1., 0., 0., 0., 1., 0., 0., 0.],
    };
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("weld.3mf");
    ThreeMf.write_assembly(&[item], &path).unwrap();
    let xml = text(&path, "3D/3dmodel.model");
    let model = roxmltree::Document::parse(&xml).unwrap();
    assert_eq!(
        model
            .descendants()
            .filter(|n| n.has_tag_name("vertex"))
            .count(),
        4
    );
    assert_eq!(
        model
            .descendants()
            .find(|n| n.has_tag_name("object"))
            .unwrap()
            .attribute("type"),
        Some("model")
    );
}
