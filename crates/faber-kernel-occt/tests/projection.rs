use faber_kernel_occt::OcctKernel;
use faber_model::Placement;
use faber_ports::{
    Combine, Kernel, Shape,
    drawing::{Curve2, ViewFrame},
};

fn front() -> ViewFrame {
    ViewFrame {
        origin: [0.; 3],
        x: [1., 0., 0.],
        y: [0., 0., 1.],
    }
}
fn top() -> ViewFrame {
    ViewFrame {
        origin: [0.; 3],
        x: [1., 0., 0.],
        y: [0., 1., 0.],
    }
}
fn plate() -> Shape {
    OcctKernel.box_shape([40., 20., 10.]).unwrap()
}
fn placed(s: &Shape, translation: [f64; 3], rotation: [f64; 3]) -> Shape {
    OcctKernel
        .transform_shape(
            s,
            &Placement {
                translation,
                rotation,
            },
        )
        .unwrap()
}
fn bored_plate() -> Shape {
    let bore = placed(
        &OcctKernel.cylinder_shape(5., 10.).unwrap(),
        [20., 10., 0.],
        [0.; 3],
    );
    OcctKernel
        .combine(&plate(), &bore, Combine::Difference)
        .unwrap()
}
fn size(b: [[f64; 2]; 2]) -> [f64; 2] {
    [b[1][0] - b[0][0], b[1][1] - b[0][1]]
}
fn close(actual: [f64; 2], expected: [f64; 2]) {
    assert!(
        (actual[0] - expected[0]).abs() < 1e-6 && (actual[1] - expected[1]).abs() < 1e-6,
        "{actual:?} instead of {expected:?}"
    );
}

#[test]
fn box_front_view() {
    let p = OcctKernel
        .project_edges(&[plate()], &front(), true)
        .unwrap();
    close(size(p.bounds), [40., 10.]);
    assert_eq!(p.visible.len(), 4, "{:?}", p.visible);
    assert!(p.visible.iter().all(|c| matches!(c, Curve2::Line(_))));
    assert!(p.hidden.is_empty(), "{:?}", p.hidden);
}
#[test]
fn hole_is_a_circle_from_the_top() {
    let p = OcctKernel
        .project_edges(&[bored_plate()], &top(), true)
        .unwrap();
    let circles: Vec<_> = p
        .visible
        .iter()
        .filter_map(|c| {
            if let Curve2::Circle { centre, radius } = c {
                Some((*centre, *radius))
            } else {
                None
            }
        })
        .collect();
    assert_eq!(circles.len(), 1, "{:?}", p.visible);
    close(circles[0].0, [20., 10.]);
    assert!((circles[0].1 - 5.).abs() < 1e-8);
}
#[test]
fn hole_is_two_hidden_lines_from_the_front() {
    let s = bored_plate();
    let p = OcctKernel
        .project_edges(std::slice::from_ref(&s), &front(), true)
        .unwrap();
    for x in [15., 25.] {
        assert!(p.hidden.iter().any(|c|matches!(c,Curve2::Line([a,b]) if (a[0]-x).abs()<1e-6 && (b[0]-x).abs()<1e-6 && ((a[1]-b[1]).abs()-10.).abs()<1e-6)),"{:?}",p.hidden);
    }
    assert!(
        OcctKernel
            .project_edges(&[s], &front(), false)
            .unwrap()
            .hidden
            .is_empty()
    );
}
#[test]
fn view_uses_the_placement() {
    let rotated = placed(&plate(), [31., 0., 4.], [0., 0., 90.]);
    let mut frame = front();
    frame.origin = [1., 0., 2.];
    let p = OcctKernel.project_edges(&[rotated], &frame, false).unwrap();
    close(size(p.bounds), [20., 10.]);
    close(p.bounds[0], [10., 2.]);
}
#[test]
fn two_parts_hide_each_other() {
    let cylinder = placed(
        &OcctKernel.cylinder_shape(3., 4.).unwrap(),
        [20., 25., 3.],
        [0.; 3],
    );
    let p = OcctKernel
        .project_edges(&[plate(), cylinder], &front(), true)
        .unwrap();
    assert_eq!(p.visible.len(), 4, "{:?}", p.visible);
    assert!(p.hidden.len() >= 2, "{:?}", p.hidden);
}
#[test]
fn section_face_of_a_tube() {
    let tube = OcctKernel
        .combine(
            &OcctKernel.cylinder_shape(5., 20.).unwrap(),
            &OcctKernel.cylinder_shape(3., 20.).unwrap(),
            Combine::Difference,
        )
        .unwrap();
    let face = OcctKernel
        .section_face(&tube, [0.; 3], [0., 1., 0.])
        .unwrap();
    let faces = OcctKernel.faces(&face).unwrap();
    assert_eq!(faces.len(), 2);
    assert!(faces.iter().all(|f| (f.area - 40.).abs() < 1e-6));
}
#[test]
fn a_tilted_circle_has_a_bounded_polyline_error() {
    let s = OcctKernel.cylinder_shape(10., 2.).unwrap();
    let k = 0.5_f64.sqrt();
    let frame = ViewFrame {
        origin: [0.; 3],
        x: [1., 0., 0.],
        y: [0., k, k],
    };
    let p = OcctKernel.project_edges(&[s], &frame, false).unwrap();
    let paths: Vec<_> = p
        .visible
        .iter()
        .filter_map(|c| {
            if let Curve2::Polyline(v) = c {
                Some(v)
            } else {
                None
            }
        })
        .collect();
    assert!(!paths.is_empty(), "{:?}", p.visible);
    for points in paths {
        for segment in points.windows(2) {
            let [a, b] = [segment[0], segment[1]];
            let on = |p: [f64; 2], cy: f64| p[0].hypot((p[1] - cy) / k);
            let centre = if (on(a, 2. * k) - 10.).abs() + (on(b, 2. * k) - 10.).abs()
                < (on(a, 0.) - 10.).abs() + (on(b, 0.) - 10.).abs()
            {
                2. * k
            } else {
                0.
            };
            let r = on([(a[0] + b[0]) / 2., (a[1] + b[1]) / 2.], centre);
            assert!((r - 10.).abs() < 0.015, "{r} from {a:?} to {b:?}");
        }
    }
}
