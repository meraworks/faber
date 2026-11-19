#include <BRepAlgoAPI_Defeaturing.hxx>
#include <BRepAlgoAPI_Section.hxx>
#include <BRepTools_History.hxx>
#include <IntCurvesFace_ShapeIntersector.hxx>
#include <TopTools_MapOfShape.hxx>
#include <BRepFeat_SplitShape.hxx>
#include <algorithm>

namespace {
constexpr const char* cannot_heal = "The faces cannot be removed: the neighbour faces do not close the solid.";
constexpr const char* missed_face = "The tool does not cross the selected face.";

std::vector<TopoDS_Face> edit_faces(const TopoDS_Shape& body, const uint32_t* ids, size_t count) {
    TopTools_IndexedMapOfShape map, selected;
    TopExp::MapShapes(body, TopAbs_FACE, map);
    if (count == 0 || count > static_cast<size_t>(map.Extent()))
        throw std::runtime_error("Select a non-empty set of faces from the solid.");
    std::vector<TopoDS_Face> result;
    for (size_t i = 0; i < count; ++i) {
        if (!ids[i] || ids[i] > static_cast<uint32_t>(map.Extent()))
            throw std::runtime_error("Unknown face index.");
        const auto face = TopoDS::Face(map(ids[i]));
        if (selected.Contains(face)) throw std::runtime_error("Each face must be distinct.");
        selected.Add(face);
        result.push_back(face);
    }
    return result;
}
double edit_volume(const TopoDS_Shape& shape) {
    GProp_GProps properties;
    BRepGProp::VolumeProperties(shape, properties);
    return properties.Mass();
}
bool valid_edit(const TopoDS_Shape& before, const TopoDS_Shape& after) {
    if (after.IsNull()) return false;
    TopTools_IndexedMapOfShape originals, solids;
    TopExp::MapShapes(before, TopAbs_SOLID, originals);
    TopExp::MapShapes(after, TopAbs_SOLID, solids);
    return !solids.IsEmpty() && solids.Extent() == originals.Extent() &&
           BRepCheck_Analyzer(after).IsValid() && edit_volume(after) > 0.;
}
gp_Dir plane_outward(const TopoDS_Face& face, const BRepAdaptor_Surface& surface) {
    const gp_Dir n = surface.Plane().Axis().Direction();
    return (face.Orientation() == TopAbs_REVERSED) == surface.Plane().Direct() ? n.Reversed() : n;
}
// Measure the part extent behind the selected plane in that plane's coordinate system.
double depth_behind(const TopoDS_Shape& body, const gp_Pnt& p, const gp_Dir& outward) {
    gp_Trsf frame;
    frame.SetTransformation(gp_Ax3(p,outward));
    Bnd_Box box;
    BRepBndLib::AddOptimal(BRepBuilderAPI_Transform(body,frame).Shape(),box,false,false);
    return -box.CornerMin().Z();
}
double wall_thickness(const TopoDS_Shape& body, const TopoDS_Face& face,
                      const BRepAdaptor_Surface& surface, double travel) {
    const gp_Dir outward=plane_outward(face,surface);
    const double extent=depth_behind(body,surface.Plane().Location(),outward);
    gp_Pnt start;gp_Pnt2d uv;
    Handle(IntTools_Context) context=new IntTools_Context;
    if(BOPTools_AlgoTools3D::PointInFace(face,start,uv,context)!=0) return extent;
    IntCurvesFace_ShapeIntersector ray;
    const double tol=Precision::Confusion();
    ray.Load(body,tol);
    ray.Perform(gp_Lin(start,outward.Reversed()),tol,std::min(travel,extent)+tol);
    if (!ray.IsDone()) return extent;
    for(int i=1;i<=ray.NbPnt();++i) {
        const double depth=ray.WParameter(i);
        const auto& opposite=ray.Face(i);
        if(depth<=tol || depth>=extent-tol) continue;
        BRepAdaptor_Surface other(opposite);
        if(other.GetType()!=GeomAbs_Plane ||
            !outward.IsOpposite(plane_outward(opposite,other),Precision::Angular())) continue;
        // A hole or a small recess under one point does not bound the whole selected region.
        gp_Trsf translation;translation.SetTranslation(gp_Vec(outward)*-depth);
        const auto projected=BRepBuilderAPI_Transform(face,translation).Shape();
        BRepAlgoAPI_Common overlap(projected,opposite);
        if(!overlap.IsDone() || overlap.HasErrors()) continue;
        GProp_GProps a,b;
        BRepGProp::SurfaceProperties(face,a); BRepGProp::SurfaceProperties(overlap.Shape(),b);
        if(std::abs(a.Mass()-b.Mass())<=a.Mass()*1e-9+tol*tol) return depth;
    }
    return extent;
}
bool has_unselected_coplanar_neighbour(const TopoDS_Shape& body, const std::vector<TopoDS_Face>& faces) {
    TopTools_IndexedDataMapOfShapeListOfShape neighbours;
    TopExp::MapShapesAndAncestors(body,TopAbs_EDGE,TopAbs_FACE,neighbours);
    TopTools_MapOfShape selected;
    for (const auto& face:faces) selected.Add(face);
    for (const auto& face:faces) {
        BRepAdaptor_Surface surface(face);
        if (surface.GetType()!=GeomAbs_Plane) continue;
        const gp_Pln plane=surface.Plane();
        for (TopExp_Explorer e(face,TopAbs_EDGE); e.More(); e.Next()) {
            for (const auto& other:neighbours.FindFromKey(e.Current())) {
                if (selected.Contains(other)) continue;
                BRepAdaptor_Surface adjacent(TopoDS::Face(other));
                if (adjacent.GetType()!=GeomAbs_Plane) continue;
                const gp_Pln p=adjacent.Plane();
                if (plane.Axis().Direction().IsParallel(p.Axis().Direction(),Precision::Angular())
                    && plane.Distance(p.Location())<=Precision::Confusion()) return true;
            }
        }
    }
    return false;
}
std::vector<TopoDS_Face> tangent_rounds(const TopoDS_Shape& body, const TopoDS_Face& face) {
    BRepAdaptor_Surface selected(face);
    if (selected.GetType()!=GeomAbs_Plane) return {};
    const gp_Dir normal=plane_outward(face,selected);
    TopTools_IndexedDataMapOfShapeListOfShape neighbours;
    TopExp::MapShapesAndAncestors(body,TopAbs_EDGE,TopAbs_FACE,neighbours);
    TopTools_MapOfShape seen;
    std::vector<TopoDS_Face> rounds;
    for(TopExp_Explorer e(face,TopAbs_EDGE);e.More();e.Next()) {
        const auto edge=TopoDS::Edge(e.Current());
        BRepAdaptor_Curve line(edge);
        if(line.GetType()!=GeomAbs_Line) continue;
        const auto middle=line.Value((line.FirstParameter()+line.LastParameter())/2.);
        for(const auto& neighbour:neighbours.FindFromKey(edge)) {
            if(neighbour.IsSame(face) || seen.Contains(neighbour)) continue;
            const auto adjacent=TopoDS::Face(neighbour);
            BRepAdaptor_Surface surface(adjacent);
            if(surface.GetType()!=GeomAbs_Cylinder) continue;
            const gp_Cylinder cylinder=surface.Cylinder();
            const gp_Dir axis=cylinder.Axis().Direction();
            if(!line.Line().Direction().IsParallel(axis,Precision::Angular())) continue;
            const gp_Vec delta(cylinder.Location(),middle);
            const gp_Vec radial=delta-gp_Vec(axis)*delta.Dot(gp_Vec(axis));
            if(radial.SquareMagnitude()<=Precision::SquareConfusion() ||
               !gp_Dir(radial).IsParallel(normal,1e-6)) continue;
            double u0,u1,v0,v1;
            BRepTools::UVBounds(adjacent,u0,u1,v0,v1);
            if(u1-u0>std::acos(-1.)+1e-6) continue;
            seen.Add(adjacent);
            rounds.push_back(adjacent);
        }
    }
    return rounds;
}
template<class Boolean>
void add_or_trim(const TopoDS_Shape& body, const TopoDS_Shape& tool, void** result) {
    Boolean op;
    TopTools_ListOfShape left, right; left.Append(body); right.Append(tool);
    op.SetArguments(left);
    op.SetTools(right);
    op.SetNonDestructive(true);
    op.Build();
    if (!op.IsDone() || op.HasErrors() || !valid_edit(body,op.Shape()))
        throw std::runtime_error("The moved faces do not make a valid solid.");
    op.SimplifyResult();
    publish_tracked(op,{body},result);
}
}

extern "C" int faber_remove_faces(const void* input, const uint32_t* ids, size_t count,
    void** result, char* error, size_t capacity) noexcept {
    return protect(error,capacity,[&]{
        const auto& body = static_cast<const FaberShape*>(input)->value;
        const auto faces = edit_faces(body,ids,count);
        BRepAlgoAPI_Defeaturing op;
        op.SetShape(body);
        for (const auto& face : faces) op.AddFaceToRemove(face);
        try { op.Build(); }
        catch (const Standard_Failure&) { throw std::runtime_error(cannot_heal); }
        if (!op.IsDone() || op.HasErrors() || op.HasWarnings() || !valid_edit(body,op.Shape()))
            throw std::runtime_error(cannot_heal);
        for (const auto& face : faces)
            if (!op.IsDeleted(face)) throw std::runtime_error(cannot_heal);
        publish_tracked(op,{body},result);
    });
}

extern "C" int faber_move_faces(const void* input, const uint32_t* ids, size_t count, double distance,
    void** result, char* error, size_t capacity) noexcept {
    return protect(error,capacity,[&]{
        const auto& body = static_cast<const FaberShape*>(input)->value;
        const auto faces = edit_faces(body,ids,count);
        const double initial_volume = edit_volume(body);
        for (const auto& face : faces) {
            BRepAdaptor_Surface surface(face);
            if (distance < 0. && surface.GetType() == GeomAbs_Plane) {
                const double thickness = wall_thickness(body,face,surface,-distance);
                if (-distance >= thickness - Precision::Confusion()) {
                    std::ostringstream text;
                    text << "Distance " << distance << " mm removes the body: the thickness at the face is "
                         << thickness << " mm.";
                    throw std::runtime_error(text.str());
                }
            }
        }
        const auto rounds=faces.size()==1 ? tangent_rounds(body,faces.front()) : std::vector<TopoDS_Face>{};
        const bool needs_step=!rounds.empty() || has_unselected_coplanar_neighbour(body,faces);
        if (!needs_step) try {
            BRepOffset_MakeOffset op;
            op.Initialize(body,0.,Precision::Confusion(),BRepOffset_Skin,false,false,GeomAbs_Intersection);
            for (const auto& face : faces) op.SetOffsetOnFace(face,distance);
            op.MakeOffsetShape();
            if (op.IsDone() && valid_edit(body,op.Shape()) &&
                (edit_volume(op.Shape())-initial_volume)*distance > 0.) {
                publish_tracked(op,{body},result,true);
                return;
            }
        } catch (const Standard_Failure&) {
            // A split planar region can need a step along its boundary.
        }
        BRepAdaptor_Surface surface(faces.front());
        TopoDS_Shape sweep;
        if (surface.GetType() == GeomAbs_Plane) {
            TopoDS_Compound prisms;
            BRep_Builder builder;
            builder.MakeCompound(prisms);
            const gp_Dir normal = plane_outward(faces.front(),surface);
            for (const auto& face: faces) {
                BRepAdaptor_Surface local(face);
                if (local.GetType()!=GeomAbs_Plane)
                    throw std::runtime_error("The moved faces do not make a valid solid.");
                const gp_Dir n=plane_outward(face,local);
                if (!n.IsParallel(normal,Precision::Angular()))
                    throw std::runtime_error("The moved faces do not make a valid solid.");
                builder.Add(prisms,BRepPrimAPI_MakePrism(face,gp_Vec(n)*distance).Shape());
            }
            if (rounds.empty()) {
                sweep=prisms;
            } else {
                // Move a tangent round with its support. Extend the other support at the tangent line.
                sweep=BRepPrimAPI_MakePrism(faces.front(),gp_Vec(normal)*distance).Shape();
                for(const auto& round:rounds) {
                    const auto segment=BRepPrimAPI_MakePrism(round,gp_Vec(normal)*distance).Shape();
                    BRepAlgoAPI_Fuse joined(sweep,segment);
                    if(!joined.IsDone() || joined.HasErrors()) throw std::runtime_error("Cannot extend the tangent round.");
                    joined.SimplifyResult();
                    sweep=joined.Shape();
                }
            }
        } else {
            if (faces.size()!=1)
                throw std::runtime_error("The moved faces do not make a valid solid.");
            BRepOffsetAPI_MakeThickSolid thick;
            thick.MakeThickSolidBySimple(faces.front(),distance);
            if (!thick.IsDone() || thick.Shape().ShapeType()!=TopAbs_SOLID)
                throw std::runtime_error("The face cannot be offset by this distance.");
            auto solid=TopoDS::Solid(thick.Shape());
            if (!BRepLib::OrientClosedSolid(solid))
                throw std::runtime_error("The face offset does not make a closed region.");
            sweep=solid;
        }
        if (distance>0.) add_or_trim<BRepAlgoAPI_Fuse>(body,sweep,result);
        else add_or_trim<BRepAlgoAPI_Cut>(body,sweep,result);
        std::unique_ptr<FaberShape> edited(static_cast<FaberShape*>(*result));
        *result=nullptr;
        if ((edit_volume(edited->value)-initial_volume)*distance<=0.)
            throw std::runtime_error("The offset does not move the selected face.");
        *result=edited.release();
    });
}

extern "C" int faber_split_face(const void* input, uint32_t id, const void* tool,
    void** result, char* error, size_t capacity) noexcept {
    return protect(error,capacity,[&]{
        const auto& body=static_cast<const FaberShape*>(input)->value;
        const auto face=edit_faces(body,&id,1).front();
        const auto& cutter=static_cast<const FaberShape*>(tool)->value;
        BRepAlgoAPI_Section section(face,cutter,false);
        section.SetNonDestructive(true);
        section.ComputePCurveOn1(true);
        section.Build();
        if(!section.IsDone() || section.HasErrors()) throw std::runtime_error(missed_face);
        TopTools_IndexedMapOfShape edges;
        TopExp::MapShapes(section.Shape(),TopAbs_EDGE,edges);
        if(edges.IsEmpty()) throw std::runtime_error(missed_face);
        BRepFeat_SplitShape op(body);
        for(int i=1;i<=edges.Extent();++i) op.Add(TopoDS::Edge(edges(i)),face);
        op.Build();
        if(!op.IsDone() || op.Modified(face).Extent()<2 || !valid_edit(body,op.Shape()))
            throw std::runtime_error(missed_face);
        publish_tracked(op,{body},result);
        // The split feature creates both regions; unchanged neighbours keep their origins.
        auto& links=static_cast<FaberShape*>(*result)->history;
        links.erase(std::remove_if(links.begin(),links.end(),
            [&](const FaceLink& link){return link.source.IsSame(face);}),links.end());
    });
}

namespace {
struct DraftHistory {
    TopoDS_Shape shape;
    Handle(BRepTools_History) history = new BRepTools_History;
    const TopoDS_Shape& Shape() const { return shape; }
    const TopTools_ListOfShape& Modified(const TopoDS_Shape& s) const { return history->Modified(s); }
    const TopTools_ListOfShape& Generated(const TopoDS_Shape& s) const { return history->Generated(s); }
};
template<class Op> void apply_draft_delta(DraftHistory& output,const TopoDS_Shape& change) {
    Op operation;
    TopTools_ListOfShape left,right; left.Append(output.shape);right.Append(change);
    operation.SetArguments(left);operation.SetTools(right);operation.SetNonDestructive(true);
    operation.Build();
    if (!operation.IsDone() || operation.HasErrors())
        throw std::runtime_error("Cannot draft the selected face region.");
    operation.SimplifyResult();
    output.history->Merge(operation.History());
    output.shape=operation.Shape();
}
TopoDS_Shape clipped_difference(const TopoDS_Shape& a,const TopoDS_Shape& b,const TopoDS_Shape& region) {
    BRepAlgoAPI_Cut difference(a,b);
    if (!difference.IsDone() || difference.HasErrors()) throw std::runtime_error("Cannot trim the draft region.");
    BRepAlgoAPI_Common clipped(difference.Shape(),region);
    if (!clipped.IsDone() || clipped.HasErrors()) throw std::runtime_error("Cannot trim the draft region.");
    return clipped.Shape();
}
}
static bool regional_draft(const TopoDS_Shape& body,const TopoDS_Shape& drafted,
        const uint32_t* ids,size_t count,void** result) {
    const auto faces=edit_faces(body,ids,count);
    if (!has_unselected_coplanar_neighbour(body,faces)) return false;
    // Draft can continue over coplanar faces. Limit added and removed material to each selected region.
    TopoDS_Compound regions;
    BRep_Builder builder;builder.MakeCompound(regions);
    for (const auto& face:faces) {
        BRepAdaptor_Surface surface(face);
        if(surface.GetType()!=GeomAbs_Plane) throw std::runtime_error("A split draft region needs a planar face.");
        const gp_Dir n=plane_outward(face,surface);
        const gp_Pnt origin=surface.Plane().Location();
        const double below=std::max(depth_behind(body,origin,n),depth_behind(drafted,origin,n))+1.;
        const double above=std::max(depth_behind(body,origin,n.Reversed()),depth_behind(drafted,origin,n.Reversed()))+1.;
        gp_Trsf start;start.SetTranslation(gp_Vec(n)*-below);
        const auto base=BRepBuilderAPI_Transform(face,start).Shape();
        builder.Add(regions,BRepPrimAPI_MakePrism(base,gp_Vec(n)*(below+above)).Shape());
    }
    const auto removed=clipped_difference(body,drafted,regions);
    const auto added=clipped_difference(drafted,body,regions);
    DraftHistory edited{body};
    if(edit_volume(removed)>0.) apply_draft_delta<BRepAlgoAPI_Cut>(edited,removed);
    if(edit_volume(added)>0.) apply_draft_delta<BRepAlgoAPI_Fuse>(edited,added);
    if(!valid_edit(body,edited.shape) || edited.shape.IsSame(body))
        throw std::runtime_error("The draft does not change the selected face region.");
    publish_tracked(edited,{body},result);
    return true;
}
