use faber_kernel_occt::OcctKernel;
use faber_ports::Kernel;

#[test]
fn box_has_twelve_edge_lines() {
    let shape = OcctKernel.box_shape([4., 5., 6.]).unwrap();
    let lines = OcctKernel.tessellate_edges(&shape, 0.03).unwrap();
    assert_eq!(lines.ranges.len(), 12);
    for edge in OcctKernel.edges(&shape).unwrap() {
        let r = &lines.ranges[(edge.index - 1) as usize];
        let pts = &lines.points[r.start as usize..r.end as usize];
        assert_eq!(pts.len(), 2);
        let [a, b] = [glam::DVec3::from(pts[0]), glam::DVec3::from(pts[1])];
        assert!((a.distance(b) - edge.length).abs() < 1e-10);
        assert!(((a + b) * 0.5 - glam::DVec3::from(edge.centre)).length() < 1e-10);
    }
}

#[test]
fn cylinder_has_three_edge_lines() {
    let shape = OcctKernel.cylinder_shape(7., 12.).unwrap();
    let lines = OcctKernel.tessellate_edges(&shape, 0.03).unwrap();
    assert_eq!(lines.ranges.len(), 3);
    for edge in OcctKernel.edges(&shape).unwrap() {
        let r = &lines.ranges[(edge.index - 1) as usize];
        let pts = &lines.points[r.start as usize..r.end as usize];
        if edge.radius.is_some() {
            assert!(pts.len() >= 8);
            assert!(
                glam::DVec3::from(pts[0]).distance(glam::DVec3::from(*pts.last().unwrap())) < 1e-8
            );
            assert!(pts.windows(2).all(|pair| {
                let mid = (glam::DVec3::from(pair[0]) + glam::DVec3::from(pair[1])) * 0.5;
                (7. - mid.x.hypot(mid.y)).abs() <= 0.03
            }));
        } else {
            assert_eq!(pts.len(), 2);
            assert!(((pts[0][2] - pts[1][2]).abs() - 12.).abs() < 1e-9);
        }
    }
}

#[test]
fn edges_omit_degenerate_poles_without_changing_edge_indices() {
    let shape = OcctKernel.sphere_shape(7.).unwrap();
    let lines = OcctKernel.tessellate_edges(&shape, 0.03).unwrap();
    for e in OcctKernel.edges(&shape).unwrap() {
        assert!(lines.ranges[(e.index - 1) as usize].len() >= 2);
    }
    assert!(lines.ranges.iter().any(|r| r.is_empty()));
}
