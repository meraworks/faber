#include <BndLib_Add2dCurve.hxx>
#include <Bnd_Box2d.hxx>
#include <BRepTools_WireExplorer.hxx>
#include <BRep_Builder.hxx>
#include <Geom2dConvert.hxx>
#include <Geom2d_BSplineCurve.hxx>
#include <Geom2d_TrimmedCurve.hxx>
#include <GeomAPI.hxx>
#include <Geom_CylindricalSurface.hxx>
#include <iomanip>

namespace {
gp_Ax3 emboss_frame(const gp_Pnt& origin, const gp_Dir& axis) {
    gp_Vec x(std::abs(axis.X()) > 0.999999 ? gp::DY() : gp::DX());
    x -= gp_Vec(axis) * x.Dot(gp_Vec(axis));
    return gp_Ax3(origin, axis, gp_Dir(x));
}

TopoDS_Wire wrap_wire(const TopoDS_Wire& wire, const TopoDS_Face& flat,
                      const gp_Pln& plane, const Handle(Geom_CylindricalSurface)& surface,
                      const double* at, Bnd_Box2d& bounds) {
    BRepBuilderAPI_MakeWire result;
    for (BRepTools_WireExplorer it(wire, flat); it.More(); it.Next()) {
        const TopoDS_Edge& edge = it.Current();
        double first, last;
        const Handle(Geom_Curve) curve = BRep_Tool::Curve(edge, first, last);
        if (curve.IsNull()) throw std::runtime_error("The sketch has an empty edge.");
        const auto planar = GeomAPI::To2d(curve, plane);
        if (planar.IsNull()) throw std::runtime_error("The sketch edge is not planar.");
        Handle(Geom2d_TrimmedCurve) segment = new Geom2d_TrimmedCurve(planar, first, last);
        BndLib_Add2dCurve::AddOptimal(segment, segment->FirstParameter(), segment->LastParameter(), 0., bounds);
        // A rational spline keeps the sketch curve under the UV scale.
        const auto uv = Geom2dConvert::CurveToBSplineCurve(segment);
        for (int i = 1; i <= uv->NbPoles(); ++i) {
            const auto p = uv->Pole(i);
            uv->SetPole(i, gp_Pnt2d((p.X() + at[0]) / surface->Radius(), p.Y() + at[1]));
        }
        BRepBuilderAPI_MakeEdge build(uv, surface);
        if (!build.IsDone()) throw std::runtime_error("Cannot wrap the sketch edge.");
        TopoDS_Edge placed = build.Edge();
        if (edge.Orientation() == TopAbs_REVERSED) placed.Reverse();
        result.Add(placed);
        if (!result.IsDone()) throw std::runtime_error("The wrapped sketch edges do not meet.");
    }
    if (!result.IsDone()) throw std::runtime_error("The sketch has an empty loop.");
    return result.Wire();
}
}

extern "C" int faber_wrap_cylinder(const void* flat_ptr, const double* start, const double* axis,
        double radius, double length, int internal, const double* at, void** output,
        char* error, size_t capacity) noexcept {
    return protect(error, capacity, [&] {
        const auto& sketch = static_cast<const FaberShape*>(flat_ptr)->value;
        Handle(Geom_CylindricalSurface) surface = new Geom_CylindricalSurface(
            emboss_frame(gp_Pnt(start[0],start[1],start[2]), gp_Dir(axis[0],axis[1],axis[2])), radius);
        TopoDS_Compound group;
        BRep_Builder builder;
        builder.MakeCompound(group);
        Bnd_Box2d bounds;
        for (TopExp_Explorer it(sketch, TopAbs_FACE); it.More(); it.Next()) {
            const auto flat = TopoDS::Face(it.Current().Oriented(TopAbs_FORWARD));
            BRepAdaptor_Surface basis(flat);
            if (basis.GetType() != GeomAbs_Plane) throw std::runtime_error("The sketch must be flat.");
            auto normal = basis.Plane().Axis().Direction();
            // Choose the same axes for all loops, including text counters.
            if (normal.Z() < -1e-9) normal.Reverse();
            const gp_Vec n(normal);
            const auto origin = gp::Origin().Translated(n * gp_Vec(gp::Origin(), basis.Plane().Location()).Dot(n));
            const gp_Pln plane(emboss_frame(origin, normal));
            const auto outer = BRepTools::OuterWire(flat);
            auto rim = wrap_wire(outer, flat, plane, surface, at, bounds);
            BRepBuilderAPI_MakeFace face(surface, rim, true);
            for (TopExp_Explorer w(flat, TopAbs_WIRE); w.More(); w.Next()) {
                const auto hole = TopoDS::Wire(w.Current());
                if (!hole.IsSame(outer))
                    face.Add(wrap_wire(hole, flat, plane, surface, at, bounds));
            }
            if (!face.IsDone()) throw std::runtime_error("Cannot build the wrapped profile.");
            auto placed = face.Face();
            BRepLib::BuildCurves3d(placed,1e-7);
            BRepLib::SameParameter(placed,1e-7);
            if (internal) placed.Reverse();
            builder.Add(group, placed);
        }
        if (bounds.IsVoid()) throw std::runtime_error("The emboss sketch is empty.");
        double x0,y0,x1,y1;
        bounds.Get(x0,y0,x1,y1);
        const double circumference = 2. * std::acos(-1.) * radius;
        if (x1-x0 >= circumference - 1e-7) {
            std::ostringstream msg;
            msg << "The profile is " << x1-x0 << " mm wide. The circumference is "
                << std::fixed << std::setprecision(2) << circumference << " mm.";
            throw std::runtime_error(msg.str());
        }
        if (y0 + at[1] < -1e-6 || y1 + at[1] > length + 1e-6)
            throw std::runtime_error("The emboss profile extends outside the selected face.");
        publish(group,output);
    });
}
