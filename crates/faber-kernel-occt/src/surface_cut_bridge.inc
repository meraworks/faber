#include <GeomLib.hxx>
#include <GeomAPI_ProjectPointOnSurf.hxx>
#include <BRepClass_FaceClassifier.hxx>
#include <Geom_RectangularTrimmedSurface.hxx>
#include <algorithm>
#include <limits>

namespace surface_cut {
const TopoDS_Shape& value(const void* handle) { return static_cast<const FaberShape*>(handle)->value; }

struct IndexedShapes : TopTools_IndexedMapOfShape {
    auto begin() const { return cbegin(); }
    auto end() const { return cend(); }
};

IndexedShapes elements(const TopoDS_Shape& shape, TopAbs_ShapeEnum kind) {
    IndexedShapes items;
    TopExp::MapShapes(shape, kind, items);
    return items;
}

void require_surface(const TopoDS_Shape& item) {
    if (elements(item, TopAbs_FACE).IsEmpty() || !elements(item, TopAbs_SOLID).IsEmpty())
        throw std::runtime_error("Select a surface.");
}

void require_body(const TopoDS_Shape& item) {
    if (elements(item, TopAbs_SOLID).IsEmpty())
        throw std::runtime_error("Select a solid body.");
}

TopoDS_Shape indexed(const TopoDS_Shape& shape, TopAbs_ShapeEnum kind, unsigned index) {
    const auto items = elements(shape, kind);
    if (!index || index > static_cast<unsigned>(items.Extent()))
        throw std::runtime_error("Unknown shape index.");
    return items(index);
}

TopoDS_Shape group(const std::vector<TopoDS_Shape>& items) {
    if (items.empty())
        throw std::runtime_error("The selected side has no pieces.");
    if (items.size() == 1)
        return items.front();
    BRep_Builder builder;
    TopoDS_Compound compound;
    builder.MakeCompound(compound);
    for (const auto& item : items)
        builder.Add(compound, item);
    return compound;
}

TopoDS_Shape joined_faces(const std::vector<TopoDS_Shape>& faces) {
    if (faces.size() == 1)
        return faces.front();
    BRepBuilderAPI_Sewing sewing(Precision::Confusion());
    for (const auto& face : faces)
        sewing.Add(face);
    sewing.Perform();
    return sewing.SewedShape();
}

void split(BRepAlgoAPI_Splitter& op, const TopoDS_Shape& stock, const TopoDS_Shape& tool) {
    TopTools_ListOfShape args, tools;
    args.Append(stock);
    tools.Append(tool);
    op.SetArguments(args);
    op.SetTools(tools);
    op.SetNonDestructive(true);
    op.SetRunParallel(false);
    op.Build();
    if (!op.IsDone() || op.HasErrors() || op.Shape().IsNull())
        throw std::runtime_error("Cannot split the shape with this tool.");
}

double reach(const TopoDS_Shape& a, const TopoDS_Shape& b) {
    Bnd_Box box;
    BRepBndLib::AddOptimal(a, box, false, false);
    BRepBndLib::AddOptimal(b, box, false, false);
    return 1.0 + std::sqrt(box.SquareExtent());
}

TopoDS_Shape translated(const TopoDS_Shape& shape, const gp_Vec& v) {
    gp_Trsf t;
    t.SetTranslation(v);
    return shape.Moved(TopLoc_Location(t));
}

TopoDS_Shape sketch_tool(const TopoDS_Shape& stock, const TopoDS_Shape& sketch, const gp_Dir& dir) {
    require_surface(sketch);
    double span = reach(stock, sketch);
    const gp_Vec shift = gp_Vec(dir) * span;
    const auto wires = elements(sketch, TopAbs_WIRE);
    std::vector<TopoDS_Shape> sides;
    for (const auto& wire : wires)
        sides.push_back(BRepPrimAPI_MakePrism(translated(wire, -shift), 2. * shift).Shape());
    return group(sides);
}

std::vector<std::vector<TopoDS_Shape>> regions(const TopoDS_Shape& shape,
                                               const TopTools_IndexedMapOfShape& barriers) {
    const auto faces = elements(shape, TopAbs_FACE);
    TopTools_IndexedDataMapOfShapeListOfShape adjacent;
    TopExp::MapShapesAndAncestors(shape, TopAbs_EDGE, TopAbs_FACE, adjacent);
    std::vector<int> parent(faces.Extent() + 1);
    for (int i = 1; i <= faces.Extent(); ++i)
        parent[i] = i;
    auto root = [&](int i) {
        while (parent[i] != i) {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        return i;
    };
    for (int e = 1; e <= adjacent.Extent(); ++e) {
        if (barriers.Contains(adjacent.FindKey(e)))
            continue;
        const auto& group = adjacent.FindFromIndex(e);
        if (group.IsEmpty())
            continue;
        int first = faces.FindIndex(group.First());
        for (const auto& f : group)
            parent[root(faces.FindIndex(f))] = root(first);
    }
    std::vector<std::vector<TopoDS_Shape>> parts(faces.Extent() + 1);
    for (int i = 1; i <= faces.Extent(); ++i)
        parts[root(i)].push_back(faces(i));
    parts.erase(std::remove_if(parts.begin(), parts.end(), [](const auto& p) { return p.empty(); }),
                parts.end());
    return parts;
}

gp_Vec outward(const TopoDS_Face& face, const TopoDS_Edge& edge);

gp_Vec seam_outward(const TopoDS_Shape& owner, const TopoDS_Shape& edge) {
    const auto face = TopoDS::Face(owner.Oriented(TopAbs_FORWARD));
    for (TopExp_Explorer e(face, TopAbs_EDGE); e.More(); e.Next())
        if (e.Current().IsSame(edge))
            return outward(face, TopoDS::Edge(e.Current()));
    throw std::runtime_error("Cannot read the faces of the shared edge.");
}

// The splitter can reuse an existing seam. Its two faces must enter opposite sides of the tool.
void include_crossed_seams(BRepAlgoAPI_Splitter& op, const TopoDS_Shape& stock, const TopoDS_Shape& tool,
                           TopTools_IndexedMapOfShape& barriers) {
    if (elements(stock, TopAbs_FACE).Extent() < 2)
        return;
    TopTools_IndexedDataMapOfShapeListOfShape owners;
    TopExp::MapShapesAndUniqueAncestors(op.Shape(), TopAbs_EDGE, TopAbs_FACE, owners);
    const auto tool_faces = elements(tool, TopAbs_FACE);
    BRepAlgoAPI_Section section(stock, tool, *op.DSFiller(), true);
    if (!section.IsDone() || section.HasErrors())
        throw std::runtime_error("Cannot read the cutting edges.");
    for (const auto& item : elements(section.Shape(), TopAbs_EDGE)) {
        if (barriers.Contains(item) || !owners.Contains(item))
            continue;
        const auto& pair = owners.FindFromKey(item);
        if (pair.Extent() != 2)
            continue;
        const auto a = seam_outward(pair.First(), item), b = seam_outward(pair.Last(), item);
        BRepAdaptor_Curve curve(TopoDS::Edge(item));
        const auto p = curve.Value((curve.FirstParameter() + curve.LastParameter()) / 2.);
        for (const auto& candidate : tool_faces) {
            auto geometry = BRep_Tool::Surface(TopoDS::Face(candidate));
            GeomAPI_ProjectPointOnSurf project(p, geometry);
            if (!project.NbPoints() || project.LowerDistance() > Precision::Confusion())
                continue;
            double u, v;
            project.LowerDistanceParameters(u, v);
            const BRepClass_FaceClassifier coverage(TopoDS::Face(candidate), gp_Pnt2d(u, v),
                                                    Precision::Confusion());
            if (coverage.State() != TopAbs_IN && coverage.State() != TopAbs_ON)
                continue;
            gp_Pnt foot;
            gp_Vec du, dv;
            geometry->D1(u, v, foot, du, dv);
            const auto n = du.Crossed(dv);
            if (n.SquareMagnitude() <= Precision::SquareConfusion())
                continue;
            const auto normal = n.Normalized();
            const double x = a.Dot(normal), y = b.Dot(normal);
            if (std::min(x, y) < -Precision::Angular() && std::max(x, y) > Precision::Angular()) {
                barriers.Add(item);
                break;
            }
        }
    }
}

void trim(const TopoDS_Shape& stock, const TopoDS_Shape& tool, const gp_Pnt& keep_point, void** output) {
    require_surface(stock);
    require_surface(tool);
    BRepAlgoAPI_Splitter op;
    split(op, stock, tool);
    TopTools_IndexedMapOfShape inherited, barriers;
    for (const auto& edge : elements(stock, TopAbs_EDGE)) {
        inherited.Add(edge);
        for (const auto& image : op.Modified(edge))
            inherited.Add(image);
    }
    for (const auto& e : elements(op.Shape(), TopAbs_EDGE))
        if (!inherited.Contains(e))
            barriers.Add(e);
    for (const auto& e : op.SectionEdges())
        barriers.Add(e);
    include_crossed_seams(op, stock, tool, barriers);
    const auto parts = regions(op.Shape(), barriers);
    if (parts.size() <= regions(stock, TopTools_IndexedMapOfShape()).size())
        throw std::runtime_error("The tool does not cross the surface.");
    const auto vertex = BRepBuilderAPI_MakeVertex(keep_point).Vertex();
    double best = std::numeric_limits<double>::infinity(), next = best;
    size_t chosen = 0;
    for (size_t i = 0; i < parts.size(); ++i) {
        double d = separation(vertex, group(parts[i]));
        if (d < best) {
            next = best;
            best = d;
            chosen = i;
        } else
            next = std::min(next, d);
    }
    if (next - best <= Precision::Confusion())
        throw std::runtime_error("The keep point is on two pieces. Move the point to one side.");
    auto result = joined_faces(parts[chosen]);
    publish_tracked(op, {stock}, output, false, &result);
}

// Each new interface has an outward face on both solids. Its orientation on the
// tool identifies the side even if the centre of mass is outside the solid.
int side(const TopoDS_Shape& solid, const TopoDS_Shape& tool,
         const TopTools_IndexedDataMapOfShapeListOfShape& owners) {
    const auto tools = elements(tool, TopAbs_FACE);
    for (const auto& s : elements(solid, TopAbs_FACE)) {
        if (!owners.Contains(s) || owners.FindFromKey(s).Extent() < 2)
            continue;
        const auto ff = TopoDS::Face(s);
        TopLoc_Location loc;
        const auto& geom = BRep_Tool::Surface(ff, loc);
        for (const auto& t : tools) {
            TopLoc_Location other;
            const auto& tg = BRep_Tool::Surface(TopoDS::Face(t), other);
            if (geom == tg && loc.IsEqual(other))
                return ff.Orientation() == t.Orientation() ? -1 : 1;
        }
    }
    // A separate solid can be wholly on one side and have no shared interface.
    if (!tools.IsEmpty()) {
        auto first = TopoDS::Face(tools(1));
        BRepAdaptor_Surface adaptor(first);
        if (adaptor.GetType() != GeomAbs_Plane)
            return 0;
        auto plane = adaptor.Plane();
        auto normal = plane_outward(first, adaptor);
        for (const auto& other : tools) {
            auto f = TopoDS::Face(other);
            BRepAdaptor_Surface surf(f);
            if (surf.GetType() != GeomAbs_Plane ||
                !normal.IsEqual(plane_outward(f, surf), Precision::Angular()) ||
                plane.Distance(surf.Plane().Location()) > Precision::Confusion())
                return 0;
        }
        if (depth_behind(solid, plane.Location(), normal) <= Precision::Confusion())
            return 1;
        if (depth_behind(solid, plane.Location(), normal.Reversed()) <= Precision::Confusion())
            return -1;
    }
    return 0;
}

void split_body(const TopoDS_Shape& stock, const TopoDS_Shape& tool, int keep, void** output) {
    require_body(stock);
    require_surface(tool);
    BRepAlgoAPI_Splitter op;
    split(op, stock, tool);
    const auto solids = elements(op.Shape(), TopAbs_SOLID);
    if (solids.Extent() <= elements(stock, TopAbs_SOLID).Extent())
        throw std::runtime_error("The surface does not cross the body.");
    if (!keep) {
        publish_tracked(op, {stock, tool}, output, true);
        return;
    }
    TopTools_IndexedDataMapOfShapeListOfShape owners;
    TopExp::MapShapesAndAncestors(op.Shape(), TopAbs_FACE, TopAbs_SOLID, owners);
    std::vector<TopoDS_Shape> chosen;
    for (const auto& solid : solids) {
        const int which = side(solid, tool, owners);
        if (which == 0)
            throw std::runtime_error("Cannot assign a side to all body pieces.");
        if (which == keep)
            chosen.push_back(solid);
    }
    const auto result = group(chosen);
    publish_tracked(op, {stock, tool}, output, true, &result);
}

TopoDS_Shape simplified(const TopoDS_Shape& shape) {
    ShapeUpgrade_UnifySameDomain unify(shape, true, true, false);
    unify.Build();
    return unify.Shape();
}

gp_Vec outward(const TopoDS_Face& face, const TopoDS_Edge& edge) {
    double a, b;
    auto pc = BRep_Tool::CurveOnSurface(edge, face, a, b);
    gp_Pnt2d uv;
    gp_Vec2d uv_t;
    pc->D1((a + b) * 0.5, uv, uv_t);
    BRepAdaptor_Surface surf(face);
    gp_Pnt p;
    gp_Vec du, dv;
    surf.D1(uv.X(), uv.Y(), p, du, dv);
    auto tangent = du * uv_t.X() + dv * uv_t.Y();
    auto outside = tangent.Crossed(du.Crossed(dv));
    if (edge.Orientation() == TopAbs_REVERSED)
        outside.Reverse();
    return outside.Normalized();
}

TopoDS_Shape parameter_strip(const TopoDS_Face& face, const TopoDS_Edge& edge, double length) {
    double first, last;
    auto pc = BRep_Tool::CurveOnSurface(edge, face, first, last);
    const auto p = pc->Value(first), q = pc->Value(last), mid = pc->Value(0.5 * (first + last));
    double u0, u1, v0, v1;
    BRepTools::UVBounds(face, u0, u1, v0, v1);
    const double eps = Precision::PConfusion() * 10.;
    const bool in_u = std::abs(p.X() - q.X()) < eps && std::abs(mid.X() - p.X()) < eps;
    const bool in_v = std::abs(p.Y() - q.Y()) < eps && std::abs(mid.Y() - p.Y()) < eps;
    if (in_u == in_v)
        throw std::runtime_error("The edge must follow a surface parameter line.");
    const double lo = in_u ? u0 : v0, hi = in_u ? u1 : v1, x = in_u ? mid.X() : mid.Y();
    if (std::min(std::abs(x - lo), std::abs(x - hi)) > eps)
        throw std::runtime_error("The edge must lie on the outer surface boundary.");
    const bool after = std::abs(x - hi) < std::abs(x - lo);
    auto geom = BRep_Tool::Surface(face);
    BRepAdaptor_Surface adaptor(face);
    double end;
    if (adaptor.GetType() == GeomAbs_Cylinder || adaptor.GetType() == GeomAbs_Cone ||
        adaptor.GetType() == GeomAbs_Plane) {
        gp_Pnt point;
        gp_Vec du, dv;
        geom->D1(mid.X(), mid.Y(), point, du, dv);
        const double speed = (in_u ? du : dv).Magnitude();
        if (speed <= Precision::Confusion())
            throw std::runtime_error("The surface ends at a pole.");
        end = x + (after ? 1. : -1.) * length / speed;
    } else {
        Handle(Geom_BoundedSurface) patch = new Geom_RectangularTrimmedSurface(geom, u0, u1, v0, v1);
        GeomLib::ExtendSurfByLength(patch, length, 1, in_u, after);
        double a, b, c, d;
        patch->Bounds(a, b, c, d);
        end = in_u ? (after ? b : a) : (after ? d : c);
        geom = patch;
    }
    const auto interval = std::minmax(x, end);
    const double start = in_u ? p.Y() : p.X(), finish = in_u ? q.Y() : q.X();
    const auto span = std::minmax(start, finish);
    BRepBuilderAPI_MakeFace patch(geom, in_u ? interval.first : span.first,
                                  in_u ? interval.second : span.second, in_u ? span.first : interval.first,
                                  in_u ? span.second : interval.second, Precision::Confusion());
    if (!patch.IsDone())
        throw std::runtime_error("Cannot extend the selected edge.");
    return patch.Face();
}

TopoDS_Shape circular_strip(const TopoDS_Face& face, const TopoDS_Edge& edge, double length) {
    BRepAdaptor_Curve curve(edge);
    const double a = curve.FirstParameter(), b = curve.LastParameter();
    const gp_Circ original = curve.Circle();
    const auto radial = gp_Vec(original.Location(), curve.Value((a + b) / 2.));
    double radius = original.Radius() + (outward(face, edge).Dot(radial) > 0. ? length : -length);
    if (radius <= Precision::Confusion())
        throw std::runtime_error("The extension closes the circular boundary.");
    const gp_Circ outer(original.Position(), std::max(original.Radius(), radius));
    const gp_Circ inner(original.Position(), std::min(original.Radius(), radius));
    if (std::abs(b - a - 2. * std::acos(-1.)) <= Precision::Angular()) {
        auto outside = BRepBuilderAPI_MakeWire(BRepBuilderAPI_MakeEdge(outer).Edge()).Wire();
        auto inside = BRepBuilderAPI_MakeWire(BRepBuilderAPI_MakeEdge(inner).Edge()).Wire();
        BRepBuilderAPI_MakeFace ring(outside, true);
        ring.Add(TopoDS::Wire(inside.Reversed()));
        return ring.Face();
    }
    const auto outer_arc = BRepBuilderAPI_MakeEdge(outer, a, b).Edge(),
               inner_arc = BRepBuilderAPI_MakeEdge(inner, a, b).Edge();
    BRepAdaptor_Curve boundary(outer_arc), bore(inner_arc);
    BRepBuilderAPI_MakeWire ring;
    ring.Add(outer_arc);
    ring.Add(BRepBuilderAPI_MakeEdge(boundary.Value(b), bore.Value(b)).Edge());
    ring.Add(TopoDS::Edge(inner_arc.Reversed()));
    ring.Add(BRepBuilderAPI_MakeEdge(bore.Value(a), boundary.Value(a)).Edge());
    return BRepBuilderAPI_MakeFace(ring.Wire(), true).Face();
}

TopoDS_Shape extend(const TopoDS_Shape& stock, unsigned index, double length) {
    require_surface(stock);
    const auto edge = TopoDS::Edge(indexed(stock, TopAbs_EDGE, index));
    TopTools_IndexedDataMapOfShapeListOfShape owners;
    TopExp::MapShapesAndAncestors(stock, TopAbs_EDGE, TopAbs_FACE, owners);
    if (!owners.Contains(edge) || owners.FindFromKey(edge).Extent() != 1)
        throw std::runtime_error("Select an open boundary edge.");
    const auto face = TopoDS::Face(owners.FindFromKey(edge).First().Oriented(TopAbs_FORWARD));
    if (BRep_Tool::IsClosed(edge, face))
        throw std::runtime_error("A surface seam cannot be extended.");
    TopoDS_Edge directed;
    for (TopExp_Explorer e(face, TopAbs_EDGE); e.More(); e.Next())
        if (e.Current().IsSame(edge)) {
            directed = TopoDS::Edge(e.Current());
            break;
        }
    TopoDS_Shape strip;
    if (BRepAdaptor_Surface(face).GetType() == GeomAbs_Plane &&
        BRepAdaptor_Curve(directed).GetType() == GeomAbs_Line)
        strip = BRepPrimAPI_MakePrism(directed, outward(face, directed) * length).Shape();
    else if (BRepAdaptor_Surface(face).GetType() == GeomAbs_Plane &&
             BRepAdaptor_Curve(directed).GetType() == GeomAbs_Circle)
        strip = circular_strip(face, directed, length);
    else
        strip = parameter_strip(face, directed, length);
    auto sewn = joined_faces({stock, strip});
    const TopTools_IndexedMapOfShape empty;
    if (elements(sewn, TopAbs_FACE).Extent() <= elements(stock, TopAbs_FACE).Extent() ||
        regions(sewn, empty).size() != regions(stock, empty).size())
        throw std::runtime_error("Cannot join the extension to the surface.");
    return simplified(sewn);
}

TopoDS_Shape boolean_result(const TopoDS_Shape& a, const TopoDS_Shape& b, bool add) {
    std::unique_ptr<BRepAlgoAPI_BooleanOperation> op;
    if (add)
        op = std::make_unique<BRepAlgoAPI_Fuse>();
    else
        op = std::make_unique<BRepAlgoAPI_Cut>();
    TopTools_ListOfShape args, tools;
    args.Append(a);
    tools.Append(b);
    op->SetArguments(args);
    op->SetTools(tools);
    op->SetNonDestructive(true);
    op->Build();
    if (!op->IsDone() || op->HasErrors())
        throw std::runtime_error("Cannot build the replacement face.");
    return op->Shape();
}

bool retains(BRepAlgoAPI_Splitter& op, const IndexedShapes& faces, const TopoDS_Shape& original) {
    if (faces.Contains(original))
        return true;
    for (const auto& image : op.Modified(original))
        if (faces.Contains(image))
            return true;
    return false;
}

// The kept piece must lose the complete cap and retain each of its neighbours.
TopoDS_Shape cut_off_cap(const TopoDS_Shape& body, const TopoDS_Shape& cap, const TopoDS_Shape& tool) {
    BRepAlgoAPI_Splitter op;
    split(op, body, tool);
    const auto pieces = elements(op.Shape(), TopAbs_SOLID);
    if (pieces.Extent() != 2)
        return {};
    TopTools_IndexedDataMapOfShapeListOfShape adjacent;
    TopExp::MapShapesAndAncestors(body, TopAbs_EDGE, TopAbs_FACE, adjacent);
    IndexedShapes neighbours;
    for (const auto& edge : elements(cap, TopAbs_EDGE)) {
        if (!adjacent.Contains(edge))
            return {};
        for (const auto& neighbour : adjacent.FindFromKey(edge))
            if (!neighbour.IsSame(cap))
                neighbours.Add(neighbour);
    }
    for (const auto& piece : pieces) {
        const auto faces = elements(piece, TopAbs_FACE);
        if (retains(op, faces, cap))
            continue;
        bool keeps_neighbours = true;
        for (const auto& neighbour : neighbours)
            if (!retains(op, faces, neighbour))
                keeps_neighbours = false;
        if (keeps_neighbours)
            return piece;
    }
    return {};
}

TopoDS_Shape raised_body(const TopoDS_Shape& body, const TopoDS_Face& face, const gp_Dir& normal,
                         const TopoDS_Shape& target) {
    auto plane = BRepAdaptor_Surface(face).Plane();
    // Raise only to the top of the target. A distant height can collapse tapered sides.
    double extent = depth_behind(target, plane.Location(), normal.Reversed());
    if (extent <= Precision::Confusion())
        return {};
    double offset = extent + std::max(1e-4, extent * 1e-4);
    BRepOffset_MakeOffset op;
    op.Initialize(body, 0., Precision::Confusion(), BRepOffset_Skin, false, false, GeomAbs_Intersection);
    op.SetOffsetOnFace(face, offset);
    op.MakeOffsetShape();
    if (!op.IsDone() || !valid_edit(body, op.Shape()))
        return {};
    const gp_Pnt top = plane.Location().Translated(gp_Vec(normal) * offset);
    for (const auto& item : elements(op.Shape(), TopAbs_FACE)) {
        auto cap = TopoDS::Face(item);
        BRepAdaptor_Surface adaptor(cap);
        if (adaptor.GetType() != GeomAbs_Plane || adaptor.Plane().Distance(top) > Precision::Confusion() ||
            !normal.IsEqual(plane_outward(cap, adaptor), Precision::Angular()))
            continue;
        auto result = cut_off_cap(op.Shape(), cap, target);
        if (!result.IsNull())
            return result;
    }
    return {};
}

TopoDS_Shape raise_curved_cap(const TopoDS_Shape& body, const TopoDS_Face& face, const TopoDS_Shape& target) {
    const double limit = reach(body, target);
    double step = std::max(separation(face, target), limit / 64.);
    for (unsigned iteration = 0; iteration < 8 && step <= 2. * limit; ++iteration, step *= 2.) {
        BRepOffset_MakeOffset offset;
        offset.Initialize(body, 0., Precision::Confusion(), BRepOffset_Skin, false, false,
                          GeomAbs_Intersection);
        offset.SetOffsetOnFace(face, step);
        offset.MakeOffsetShape();
        if (!offset.IsDone() || !valid_edit(body, offset.Shape()))
            continue;
        TopTools_ListOfShape caps = offset.Modified(face);
        for (const auto& f : offset.Generated(face))
            caps.Append(f);
        for (const auto& cap : caps) {
            if (cap.ShapeType() != TopAbs_FACE)
                continue;
            auto cut = cut_off_cap(offset.Shape(), cap, target);
            if (!cut.IsNull())
                return cut;
        }
    }
    return {};
}

bool allows_prism(const TopoDS_Shape& body, const TopoDS_Face& cap, const gp_Dir& dir) {
    TopTools_IndexedDataMapOfShapeListOfShape neighbours;
    TopExp::MapShapesAndAncestors(body, TopAbs_EDGE, TopAbs_FACE, neighbours);
    for (const auto& edge : elements(cap, TopAbs_EDGE)) {
        for (const auto& face : neighbours.FindFromKey(edge)) {
            if (face.IsSame(cap))
                continue;
            BRepAdaptor_Surface surface(TopoDS::Face(face));
            switch (surface.GetType()) {
            case GeomAbs_Plane: {
                auto normal = surface.Plane().Axis().Direction();
                // A coplanar step can grow from the perimeter of the selected face.
                if (!normal.IsNormal(dir, Precision::Angular()) &&
                    !normal.IsParallel(dir, Precision::Angular()))
                    return false;
                break;
            }
            case GeomAbs_Cylinder:
                if (!surface.Cylinder().Axis().Direction().IsParallel(dir, Precision::Angular()))
                    return false;
                break;
            case GeomAbs_SurfaceOfExtrusion:
                if (!surface.Direction().IsParallel(dir, Precision::Angular()))
                    return false;
                break;
            default:
                return false;
            }
        }
    }
    return true;
}

TopoDS_Shape replace(const TopoDS_Shape& stock, unsigned index, const TopoDS_Shape& surface) {
    require_body(stock);
    require_surface(surface);
    const auto face = TopoDS::Face(indexed(stock, TopAbs_FACE, index));
    BRepAdaptor_Surface adaptor(face);
    const auto failure = "The surface does not cover face " + std::to_string(index) + ". Extend the surface.";
    if (adaptor.GetType() != GeomAbs_Plane) {
        auto cut = cut_off_cap(stock, face, surface);
        if (!cut.IsNull())
            return simplified(cut);
        try {
            auto raised = raise_curved_cap(stock, face, surface);
            if (!raised.IsNull())
                return simplified(raised);
        } catch (const Standard_Failure&) {
            // A short curve can collapse before it reaches the target.
        }
        throw std::runtime_error(failure);
    }
    auto dir = plane_outward(face, adaptor);
    const auto shift = gp_Vec(dir) * reach(stock, surface);
    auto bottom = translated(face, -shift);
    BRepPrimAPI_MakePrism column(bottom, shift * 2.);
    BRepAlgoAPI_Splitter op;
    split(op, column.Shape(), surface);
    const auto pieces = elements(op.Shape(), TopAbs_SOLID);
    if (pieces.Extent() != 2)
        throw std::runtime_error(failure);
    auto lower = cut_off_cap(stock, face, surface);
    if (!lower.IsNull())
        return simplified(lower);
    try {
        auto raised = raised_body(stock, face, dir, surface);
        if (!raised.IsNull())
            return simplified(raised);
    } catch (const Standard_Failure&) {
        // A coplanar step can prevent an offset. Keep the selected footprint.
    }
    if (!allows_prism(stock, face, dir))
        throw std::runtime_error("The neighbour faces cannot extend to the replacement surface.");
    const auto cap = column.LastShape();
    TopoDS_Shape upper;
    for (const auto& piece : pieces)
        if (elements(piece, TopAbs_FACE).Contains(cap))
            upper = piece;
    if (upper.IsNull())
        throw std::runtime_error(failure);
    const auto beyond = BRepPrimAPI_MakePrism(face, shift).Shape();
    auto result = simplified(boolean_result(boolean_result(stock, beyond, true), upper, false));
    if (elements(result, TopAbs_SOLID).Extent() != elements(stock, TopAbs_SOLID).Extent())
        throw std::runtime_error("The replacement surface removes or divides the body.");
    return result;
}
} // namespace surface_cut

extern "C" int faber_trim_surface(const void* input, const void* tool, int mode, const double* origin,
                                  const double* normal, const double* point, void** output, char* error,
                                  size_t capacity) noexcept {
    return protect(error, capacity, [&] {
        const auto& stock = surface_cut::value(input);
        TopoDS_Shape cutter;
        if (mode == 0)
            cutter = surface_cut::value(tool);
        else {
            const gp_Dir direction(normal[0], normal[1], normal[2]);
            if (mode == 1)
                cutter = surface_cut::sketch_tool(stock, surface_cut::value(tool), direction);
            else
                cutter =
                    BRepBuilderAPI_MakeFace(gp_Pln(gp_Pnt(origin[0], origin[1], origin[2]), direction)).Face();
        }
        surface_cut::trim(stock, cutter, gp_Pnt(point[0], point[1], point[2]), output);
    });
}
extern "C" int faber_extend_surface(const void* input, unsigned edge, double distance, void** output,
                                    char* error, size_t capacity) noexcept {
    return protect(error, capacity,
                   [&] { publish(surface_cut::extend(surface_cut::value(input), edge, distance), output); });
}
extern "C" int faber_split_surface(const void* input, const void* tool, int keep, void** output, char* error,
                                   size_t capacity) noexcept {
    return protect(error, capacity, [&] {
        surface_cut::split_body(surface_cut::value(input), surface_cut::value(tool), keep, output);
    });
}
extern "C" int faber_surface_piece(const void* input, unsigned index, void** output, char* error,
                                   size_t capacity) noexcept {
    return protect(error, capacity, [&] {
        publish(surface_cut::indexed(surface_cut::value(input), TopAbs_SOLID, index), output);
    });
}
extern "C" int faber_replace_surface(const void* solid, unsigned index, const void* surface, void** output,
                                     char* error, size_t capacity) noexcept {
    return protect(error, capacity, [&] {
        publish(surface_cut::replace(surface_cut::value(solid), index, surface_cut::value(surface)), output);
    });
}
