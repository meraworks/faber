#include <BRepFill_Filling.hxx>
#include <BRepBuilderAPI_MakeSolid.hxx>
#include <GeomAPI_ProjectPointOnCurve.hxx>
#include <GeomAPI_ExtremaCurveCurve.hxx>
#include <algorithm>
#include <iomanip>
#include <limits>
#include <tuple>

namespace surface_join {
constexpr double precision = 1.e-7;
struct Range {
    Handle(Geom_Curve) curve;
    double first, last;
    explicit Range(const TopoDS_Edge &edge) {
        curve = BRep_Tool::Curve(edge, first, last);
        if (curve.IsNull())
            throw std::runtime_error("An edge has no spatial curve.");
    }
    gp_Pnt point(double fraction) const { return curve->Value(first + (last - first) * fraction); }
    double distance(const gp_Pnt &p) const {
        double d = std::min(point(0).Distance(p), point(1).Distance(p));
        GeomAPI_ProjectPointOnCurve project(p, curve, first, last);
        if (project.NbPoints() > 0)
            d = std::min(d, project.LowerDistance());
        return d;
    }
};

// Check both ends and stationary separations in both directions.
static double separation(const TopoDS_Edge &a, const TopoDS_Edge &b) {
    if (a.IsSame(b))
        return 0.;
    const Range x(a), y(b);
    double gap = 0.;
    for (int i = 0; i <= 8; ++i) {
        const double t = i / 8.;
        gap = std::max({gap, x.distance(y.point(t)), y.distance(x.point(t))});
    }
    GeomAPI_ExtremaCurveCurve extrema(x.curve, y.curve, x.first, x.last, y.first, y.last);
    if (!extrema.IsParallel()) {
        for (int i = 1; i <= extrema.NbExtrema(); ++i) {
            gp_Pnt p, q;
            extrema.Points(i, p, q);
            gap = std::max({gap, x.distance(q), y.distance(p)});
        }
    }
    return gap;
}

static std::vector<TopoDS_Edge> boundary_edges(const void *const *input, size_t count) {
    TopTools_IndexedMapOfShape keys;
    std::vector<TopoDS_Edge> edges;
    for (size_t i = 0; i < count; ++i) {
        const auto &s = static_cast<const FaberShape *>(input[i])->value;
        if (s.ShapeType() != TopAbs_WIRE && s.ShapeType() != TopAbs_EDGE)
            throw std::runtime_error("A patch boundary must contain curves or selected edges.");
        for (TopExp_Explorer e(s, TopAbs_EDGE); e.More(); e.Next()) {
            if (keys.Contains(e.Current()))
                throw std::runtime_error("A patch boundary repeats an edge.");
            keys.Add(e.Current());
            if (BRep_Tool::Degenerated(TopoDS::Edge(e.Current())))
                throw std::runtime_error("The boundary contains a zero-length edge.");
            edges.push_back(TopoDS::Edge(e.Current()));
        }
    }
    if (edges.empty())
        throw std::runtime_error("The patch needs a closed boundary.");
    if (edges.size() > 4096)
        throw std::runtime_error("The patch boundary has " + std::to_string(edges.size()) +
                                 " edges. The limit is 4096.");
    return edges;
}
static auto order(const gp_Pnt &p) { return std::make_tuple(p.X(), p.Y(), p.Z()); }
static TopoDS_Wire wire(const std::vector<TopoDS_Edge> &edges) {
    std::vector<gp_Pnt> ends;
    TopTools_ListOfShape list;
    for (const auto &edge : edges) {
        TopoDS_Vertex a, b;
        TopExp::Vertices(edge, a, b, true);
        ends.push_back(BRep_Tool::Pnt(a));
        ends.push_back(BRep_Tool::Pnt(b));
        list.Append(edge);
    }
    int axis = 1;
    double largest = 0.;
    for (int k = 1; k <= 3; ++k) {
        const auto bounds =
            std::minmax_element(ends.begin(), ends.end(), [k](const gp_Pnt &a, const gp_Pnt &b) {
                return a.Coord(k) < b.Coord(k);
            });
        const double extent = bounds.second->Coord(k) - bounds.first->Coord(k);
        if (extent > largest) {
            axis = k;
            largest = extent;
        }
    }
    std::sort(ends.begin(), ends.end(),
              [axis](const gp_Pnt &a, const gp_Pnt &b) { return a.Coord(axis) < b.Coord(axis); });
    std::vector<gp_Pnt> open;
    size_t first = 0;
    for (const auto &p : ends) {
        while (p.Coord(axis) - ends[first].Coord(axis) > precision)
            ++first;
        size_t n = 0;
        for (size_t j = first; j < ends.size() && ends[j].Coord(axis) - p.Coord(axis) <= precision;
             ++j) {
            if (p.SquareDistance(ends[j]) <= precision * precision && ++n > 2)
                throw std::runtime_error("The patch boundary branches at a vertex.");
        }
        if (n == 1)
            open.push_back(p);
    }
    if (!open.empty()) {
        std::sort(open.begin(), open.end(),
                  [](const gp_Pnt &a, const gp_Pnt &b) { return order(a) < order(b); });
        const auto &p = open.front();
        double gap = std::numeric_limits<double>::infinity();
        for (size_t j = 1; j < open.size(); ++j)
            gap = std::min(gap, p.Distance(open[j]));
        std::ostringstream msg;
        msg << std::setprecision(9) << "The boundary has a gap of " << gap << " mm at (" << p.X() << ", "
            << p.Y() << ", " << p.Z() << ").";
        throw std::runtime_error(msg.str());
    }
    BRepBuilderAPI_MakeWire make;
    make.Add(list);
    if (!make.IsDone())
        throw std::runtime_error("The boundary must be one closed loop.");
    TopTools_IndexedMapOfShape made;
    TopExp::MapShapes(make.Wire(), TopAbs_EDGE, made);
    if (made.Extent() != static_cast<int>(edges.size()) || !make.Wire().Closed())
        throw std::runtime_error("The boundary must be one closed loop.");
    return make.Wire();
}
struct Constraint {
    TopoDS_Edge edge;
    TopoDS_Face support;
};
static std::vector<Constraint> constraints(const std::vector<TopoDS_Edge> &edges,
                                           const void *const *support, size_t count) {
    std::vector<Constraint> result;
    for (const auto &e : edges)
        result.push_back({e, {}});
    for (size_t i = 0; i < count; ++i) {
        bool used = false;
        const auto &src = static_cast<const FaberShape *>(support[i])->value;
        for (auto &con : result) {
            for (TopExp_Explorer faces(src, TopAbs_FACE); faces.More(); faces.Next()) {
                const auto f = TopoDS::Face(faces.Current());
                for (TopExp_Explorer side(f, TopAbs_EDGE); side.More(); side.Next()) {
                    const auto e = TopoDS::Edge(side.Current());
                    if (BRep_Tool::Degenerated(e))
                        continue;
                    if (!con.edge.IsSame(e)) {
                        Range a(con.edge), b(e);
                        if (b.distance(a.point(0.5)) > precision ||
                            a.distance(b.point(0.5)) > precision || separation(con.edge, e) > precision)
                            continue;
                    }
                    if (!con.support.IsNull() && !con.support.IsSame(f))
                        throw std::runtime_error(
                            "Two support faces meet the same boundary edge. Select one tangent face.");
                    con.edge = e;
                    con.support = f;
                    used = true;
                }
            }
        }
        if (!used)
            throw std::runtime_error("A tangent support does not meet the patch boundary.");
    }
    return result;
}
static TopoDS_Face round_cap(const Constraint &c) {
    const BRepAdaptor_Curve curve(c.edge);
    const BRepAdaptor_Surface surf(c.support);
    if (curve.GetType() != GeomAbs_Circle || surf.GetType() != GeomAbs_Cylinder ||
        !BRep_Tool::IsClosed(c.edge))
        return {};
    const auto rim = curve.Circle();
    const auto cylinder = surf.Cylinder();
    if (!rim.Axis().Direction().IsParallel(cylinder.Axis().Direction(), 1.e-8) ||
        std::abs(rim.Radius() - cylinder.Radius()) > precision)
        return {};
    auto axis = cylinder.Axis().Direction();
    GProp_GProps props;
    BRepGProp::SurfaceProperties(c.support, props);
    if (gp_Vec(props.CentreOfMass(), rim.Location()).Dot(gp_Vec(axis)) < 0.)
        axis.Reverse();
    BRepPrimAPI_MakeSphere dome(gp_Ax2(rim.Location(), axis, rim.XAxis().Direction()), rim.Radius(), 0.,
                                std::acos(-1.) / 2.);
    for (TopExp_Explorer faces(dome.Shape(), TopAbs_FACE); faces.More(); faces.Next()) {
        const auto f = TopoDS::Face(faces.Current());
        if (BRepAdaptor_Surface(f).GetType() == GeomAbs_Sphere)
            return f;
    }
    throw std::runtime_error("Cannot form the tangent cap.");
}
static TopoDS_Face patch(const std::vector<TopoDS_Edge> &edges, const void *const *support,
                         size_t count) {
    const auto boundary = wire(edges);
    if (count == 0) {
        BRepBuilderAPI_MakeFace plane(boundary, true);
        if (plane.IsDone())
            return plane.Face();
    }
    const auto cons = constraints(edges, support, count);
    if (cons.size() == 1 && !cons[0].support.IsNull()) {
        const auto face = round_cap(cons[0]);
        if (!face.IsNull())
            return face;
    }
    BRepFill_Filling fill;
    fill.SetConstrParam(1.e-6, 1.e-5, 0.01, 0.1);
    for (const auto &c : cons) {
        if (c.support.IsNull())
            fill.Add(c.edge, GeomAbs_C0);
        else
            fill.Add(c.edge, c.support, GeomAbs_G1);
    }
    fill.Build();
    if (!fill.IsDone() || fill.G0Error() > 0.001 || (count != 0 && fill.G1Error() > 0.03))
        throw std::runtime_error("Cannot fill the boundary at the specified continuity.");
    return fill.Face();
}
static uint32_t free_edges(const TopoDS_Shape &s) {
    TopTools_IndexedDataMapOfShapeListOfShape faces;
    TopExp::MapShapesAndAncestors(s, TopAbs_EDGE, TopAbs_FACE, faces);
    uint32_t n = 0;
    for (int i = 1; i <= faces.Extent(); ++i) {
        if (!BRep_Tool::Degenerated(TopoDS::Edge(faces.FindKey(i))) && faces(i).Extent() == 1)
            ++n;
    }
    return n;
}
static TopoDS_Shape stitch(const void *const *surfaces, size_t count, double tolerance, bool as_solid,
                           uint32_t &open, double &gap) {
    if (count == 0)
        throw std::runtime_error("Select at least one surface to stitch.");
    BRepBuilderAPI_Sewing sew(tolerance);
    sew.SetMaxTolerance(tolerance);
    TopTools_IndexedMapOfShape faces;
    for (size_t i = 0; i < count; ++i) {
        const auto &s = static_cast<const FaberShape *>(surfaces[i])->value;
        if (TopExp_Explorer(s, TopAbs_SOLID).More() ||
            TopExp_Explorer(s, TopAbs_EDGE, TopAbs_FACE).More() ||
            TopExp_Explorer(s, TopAbs_VERTEX, TopAbs_EDGE).More())
            throw std::runtime_error("A stitch input must be a surface.");
        TopTools_IndexedMapOfShape current;
        TopExp::MapShapes(s, TopAbs_FACE, current);
        if (current.IsEmpty())
            throw std::runtime_error("A stitch input has no faces.");
        for (int j = 1; j <= current.Extent(); ++j) {
            if (faces.Contains(current(j)))
                throw std::runtime_error("A stitch input repeats a face.");
            faces.Add(current(j));
            sew.Add(current(j));
        }
    }
    sew.Perform();
    auto result = sew.SewedShape();
    if (result.IsNull() || sew.NbDeletedFaces() != 0)
        throw std::runtime_error("The stitch would lose a face. Reduce its tolerance.");
    if (sew.NbMultipleEdges() != 0)
        throw std::runtime_error("More than two faces meet at a stitched edge.");
    open = free_edges(result);
    gap = 0.;
    for (int i = 1; i <= sew.NbContigousEdges(); ++i) {
        const auto &pair = sew.ContigousEdgeCouple(i);
        for (TopTools_ListIteratorOfListOfShape a(pair); a.More(); a.Next()) {
            auto b = a;
            for (b.Next(); b.More(); b.Next())
                gap = std::max(gap, separation(TopoDS::Edge(a.Value()), TopoDS::Edge(b.Value())));
        }
    }
    if (!as_solid)
        return result;
    if (open != 0)
        throw std::runtime_error("The stitched shell has " + std::to_string(open) +
                                 " open edges. It is not a solid.");
    TopTools_IndexedMapOfShape shells;
    TopExp::MapShapes(result, TopAbs_SHELL, shells);
    if (shells.Extent() != 1)
        throw std::runtime_error("Select surfaces that enclose one connected shell.");
    auto solid = BRepBuilderAPI_MakeSolid(TopoDS::Shell(shells(1))).Solid();
    if (!BRepLib::OrientClosedSolid(solid))
        throw std::runtime_error("Cannot orient the stitched solid.");
    // Dress operations need a forward solid with the orientation on its shell.
    if (solid.Orientation() == TopAbs_REVERSED) {
        TopoDS_Solid forward;
        BRep_Builder builder;
        builder.MakeSolid(forward);
        for (TopoDS_Iterator shells(solid); shells.More(); shells.Next())
            builder.Add(forward, shells.Value());
        solid = forward;
    }
    GProp_GProps mass;
    BRepGProp::VolumeProperties(solid, mass);
    if (!(mass.Mass() > 0.))
        throw std::runtime_error("The stitched shell encloses no volume.");
    return solid;
}
} // namespace surface_join

extern "C" int faber_join_edge(const void *input, uint32_t index, void **out, char *error,
                               size_t len) noexcept {
    return protect(error, len, [&] {
        TopTools_IndexedMapOfShape edges;
        TopExp::MapShapes(static_cast<const FaberShape *>(input)->value, TopAbs_EDGE, edges);
        if (index == 0 || index > static_cast<uint32_t>(edges.Extent()))
            throw std::runtime_error("Unknown edge index.");
        publish(edges(index), out);
    });
}
extern "C" int faber_patch_boundary(const void *const *edges, size_t count, const void *const *support,
                                    size_t supports, void **out, char *error, size_t len) noexcept {
    return protect(error, len, [&] {
        publish(surface_join::patch(surface_join::boundary_edges(edges, count), support, supports), out);
    });
}
extern "C" int faber_stitch_faces(const void *const *faces, size_t count, double tolerance, int solid,
                                  uint32_t *open, double *gap, void **out, char *error,
                                  size_t len) noexcept {
    return protect(error, len, [&] {
        publish(surface_join::stitch(faces, count, tolerance, solid != 0, *open, *gap), out);
    });
}
