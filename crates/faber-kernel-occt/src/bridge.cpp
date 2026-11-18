#include "bridge.h"
#include <BRepAlgoAPI_Cut.hxx>
#include <BRepAlgoAPI_Fuse.hxx>
#include <BRepAlgoAPI_Common.hxx>
#include <BRepAlgoAPI_Splitter.hxx>
#include <BOPAlgo_BuilderSolid.hxx>
#include <BRepAlgoAPI_Check.hxx>
#include <BRepBuilderAPI_Sewing.hxx>
#include <ShapeUpgrade_UnifySameDomain.hxx>
#include <BRep_Builder.hxx>
#include <TopoDS_Compound.hxx>
#include <BRepBndLib.hxx>
#include <BRepBuilderAPI_Transform.hxx>
#include <BRepBuilderAPI_GTransform.hxx>
#include <BRepAdaptor_CompCurve.hxx>
#include <GCPnts_AbscissaPoint.hxx>
#include <GCPnts_TangentialDeflection.hxx>
#include <gp_GTrsf.hxx>
#include <BRepBuilderAPI_MakePolygon.hxx>
#include <BRepBuilderAPI_MakeFace.hxx>
#include <BRepBuilderAPI_MakeEdge.hxx>
#include <BRepBuilderAPI_MakeWire.hxx>
#include <BRepBuilderAPI_MakeVertex.hxx>
#include <BRepOffsetAPI_MakeOffset.hxx>
#include <BOPTools_AlgoTools3D.hxx>
#include <IntTools_Context.hxx>
#include <HLRTopoBRep_OutLiner.hxx>
#include <HLRAlgo_Projector.hxx>
#include <BRepTopAdaptor_MapOfShapeTool.hxx>
#include <gp_Circ.hxx>
#include <gp_Mat.hxx>
#include <gp_Elips.hxx>
#include <GC_MakeArcOfCircle.hxx>
#include <Geom_BezierCurve.hxx>
#include <Geom_BSplineCurve.hxx>
#include <GeomAPI_Interpolate.hxx>
#include <Geom_CylindricalSurface.hxx>
#include <Geom2d_Line.hxx>
#include <gp_Ax3.hxx>
#include <TColgp_HArray1OfPnt.hxx>
#include <TColgp_Array1OfPnt.hxx>
#include <ShapeAnalysis.hxx>
#include <ShapeExtend_WireData.hxx>
#include <BRepPrimAPI_MakePrism.hxx>
#include <TopoDS_Wire.hxx>
#include <BRepCheck_Analyzer.hxx>
#include <BRepGProp.hxx>
#include <BRepLib.hxx>
#include <BRepExtrema_DistShapeShape.hxx>
#include <BRepMesh_IncrementalMesh.hxx>
#include <BRepLib_ToolTriangulatedShape.hxx>
#include <BRepPrimAPI_MakeBox.hxx>
#include <BRepPrimAPI_MakeCylinder.hxx>
#include <BRepPrimAPI_MakeSphere.hxx>
#include <BRepPrimAPI_MakeCone.hxx>
#include <BRepPrimAPI_MakeTorus.hxx>
#include <BRepPrimAPI_MakeRevol.hxx>
#include <BRepOffsetAPI_ThruSections.hxx>
#include <BRepOffsetAPI_MakePipeShell.hxx>
#include <BRepFilletAPI_MakeFillet.hxx>
#include <BRepFilletAPI_MakeChamfer.hxx>
#include <BRepAdaptor_Curve.hxx>
#include <BRepAdaptor_Surface.hxx>
#include <BRepOffsetAPI_MakeThickSolid.hxx>
#include <BRepOffsetAPI_MakeOffsetShape.hxx>
#include <BRepOffset_MakeOffset.hxx>
#include <BRepOffset_Mode.hxx>
#include <BRepOffsetAPI_DraftAngle.hxx>
#include <gp_Pln.hxx>
#include <gp_Ax1.hxx>
#include <BRep_Tool.hxx>
#include <BRepTools.hxx>
#include <BRepTools_WireExplorer.hxx>
#include <Bnd_Box.hxx>
#include <GProp_GProps.hxx>
#include <Message.hxx>
#include <Message_Messenger.hxx>
#include <Message_PrinterOStream.hxx>
#include <Poly_Triangulation.hxx>
#include <STEPControl_Reader.hxx>
#include <STEPControl_Writer.hxx>
#include <Standard_Failure.hxx>
#include <TopExp.hxx>
#include <TopExp_Explorer.hxx>
#include <TopoDS.hxx>
#include <TopoDS_Shape.hxx>
#include <TopTools_IndexedMapOfShape.hxx>
#include <TopTools_ListOfShape.hxx>
#include <TopTools_ListIteratorOfListOfShape.hxx>
#include <TopTools_IndexedDataMapOfShapeListOfShape.hxx>
#include <initializer_list>
#include <gp_Trsf.hxx>
#include <OSD_ThreadPool.hxx>
#include <Standard_Version.hxx>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <sstream>
#include <BinTools.hxx>
#include <cmath>
#include <cstdint>
#include <exception>
#include <memory>
#include <stdexcept>
#include <vector>

struct FaceLink { TopoDS_Shape source,output; };
struct FaberShape { TopoDS_Shape value; std::vector<FaceLink> history; bool tracked=false; };
struct FaberFaceSource { uint32_t face,input,source_face; };
struct FaberDistance { double distance; double point_a[3]; double point_b[3]; };
struct FaberSegment { int32_t kind; double data[6]; const double* points; size_t count; };
struct FaberCurveSegment { int32_t kind; double data[9]; const double* points; size_t count; };
struct FaberEdge { uint32_t index; double length; double centre[3]; double direction[3]; double radius; };
struct FaberFace { uint32_t index; double area; double centre[3]; double normal[3]; double start[3],axis[3],radius,length; int internal; };
struct FaberEdgeFace { uint32_t edge,face; };
struct FaberMesh {
    std::vector<double> vertices, normals;
    std::vector<uint32_t> triangles, face_of_triangle;
};
struct FaberProperties {
    double volume;
    double centre[3];
    double bounds[6];
    uint32_t faces, edges, solids, valid;
};
struct FaberMassProperties { double volume; double area; double centre[3]; double inertia[9]; };

template<class F> int protect(char* error,size_t capacity,F action) noexcept {
    try {
        Message::DefaultMessenger()->RemovePrinters(STANDARD_TYPE(Message_PrinterOStream));
        action();
        return 0;
    } catch (const Standard_Failure& failure) {
        std::snprintf(error,capacity,"%s",failure.GetMessageString() ? failure.GetMessageString() : "Geometry operation failed");
    } catch (const std::exception& failure) {
        std::snprintf(error,capacity,"%s",failure.what());
    } catch (...) {
        std::snprintf(error,capacity,"Geometry operation failed");
    }
    return 1;
}

static void publish(const TopoDS_Shape& shape,void** result) {
    if(shape.IsNull() || !BRepCheck_Analyzer(shape).IsValid()) throw std::runtime_error("Operation did not produce a valid shape");
    *result=new FaberShape{shape};
}

#include "drawing_bridge.inc"

template<class Operation> static void publish_tracked(Operation& operation,std::initializer_list<TopoDS_Shape> inputs,void** result,bool retain_generated=false,const TopoDS_Shape* final_shape=nullptr) {
    void* raw=nullptr;
    publish(final_shape ? *final_shape : operation.Shape(),&raw);
    std::unique_ptr<FaberShape> output(static_cast<FaberShape*>(raw));
    TopTools_IndexedMapOfShape final_faces;
    TopExp::MapShapes(output->value,TopAbs_FACE,final_faces);
    for(const auto& input:inputs) {
        TopTools_IndexedMapOfShape faces;
        TopExp::MapShapes(input,TopAbs_FACE,faces);
        for(int f=1;f<=faces.Extent();++f) {
            if(final_faces.Contains(faces(f))) output->history.push_back({faces(f),faces(f)});
            TopTools_ListOfShape modified=operation.Modified(faces(f));
            if(retain_generated) {
                const auto& generated=operation.Generated(faces(f));
                for(TopTools_ListIteratorOfListOfShape it(generated);it.More();it.Next()) modified.Append(it.Value());
            }
            for(TopTools_ListIteratorOfListOfShape it(modified);it.More();it.Next()) {
                TopTools_IndexedMapOfShape changed;
                TopExp::MapShapes(it.Value(),TopAbs_FACE,changed);
                for(int m=1;m<=changed.Extent();++m) {
                    if(final_faces.Contains(changed(m))) output->history.push_back({faces(f),changed(m)});
                }
            }
        }
    }
    output->tracked=true;
    *result=output.release();
}

extern "C" int faber_face_sources(const void* input,const void* const* sources,size_t source_count,FaberFaceSource* result,size_t capacity_links,size_t* count,uint32_t* known,char* error,size_t capacity) noexcept {
    return protect(error,capacity,[&] {
        const auto& output=*static_cast<const FaberShape*>(input);
        *known=output.tracked || source_count==0;
        *count=0;
        if(!*known) return;
        TopTools_IndexedMapOfShape final_faces;
        TopExp::MapShapes(output.value,TopAbs_FACE,final_faces);
        std::vector<FaberFaceSource> links;
        for(size_t s=0;s<source_count;++s) {
            TopTools_IndexedMapOfShape faces;
            TopExp::MapShapes(static_cast<const FaberShape*>(sources[s])->value,TopAbs_FACE,faces);
            for(const auto& link:output.history) {
                const int source=faces.FindIndex(link.source);
                const int face=final_faces.FindIndex(link.output);
                if(source && face) links.push_back({static_cast<uint32_t>(face),static_cast<uint32_t>(s),static_cast<uint32_t>(source)});
            }
        }
        *count=links.size();
        if(!result) return;
        if(capacity_links<links.size()) throw std::runtime_error("Face history buffer is too small");
        for(size_t i=0;i<links.size();++i) result[i]=links[i];
    });
}

extern "C" int faber_box(const double* size,void** result,char* error,size_t capacity) noexcept {
    return protect(error,capacity,[&]{publish(BRepPrimAPI_MakeBox(size[0],size[1],size[2]).Shape(),result);});
}

extern "C" int faber_solid_from_mesh(const double* vertices,size_t vertex_count,
    const uint32_t* triangles,size_t triangle_count,int merge_planar,void** result,
    char* error,size_t capacity) noexcept {
    return protect(error,capacity,[&] {
        BRepBuilderAPI_Sewing stitch(1e-7);
        for(size_t i=0;i<triangle_count;++i) {
            BRepBuilderAPI_MakePolygon outline;
            for(size_t corner=0;corner<3;++corner) {
                const auto id=triangles[3*i+corner];
                if(id>=vertex_count) throw std::runtime_error("Invalid triangle vertex");
                const double* p=vertices+3*id;
                outline.Add(gp_Pnt(p[0],p[1],p[2]));
            }
            outline.Close();
            if(!outline.IsDone()) throw std::runtime_error("Cannot build a triangle boundary");
            BRepBuilderAPI_MakeFace facet(outline.Wire(),true);
            if(!facet.IsDone()) throw std::runtime_error("Cannot build a triangle face");
            stitch.Add(facet.Face());
        }
        stitch.Perform();
        const auto sewn=stitch.SewedShape();
        if(sewn.IsNull() || stitch.NbFreeEdges()!=0 || stitch.NbMultipleEdges()!=0)
            throw std::runtime_error("Cannot join the triangle faces into closed shells");
        TopTools_ListOfShape faces;
        for(TopExp_Explorer e(sewn,TopAbs_FACE);e.More();e.Next()) faces.Append(e.Current());
        if(static_cast<size_t>(faces.Extent())!=triangle_count)
            throw std::runtime_error("Joining the mesh lost a triangle face");
        // Closed shells with inward winding define cavities inside their enclosing solid.
        BOPAlgo_BuilderSolid builder;
        builder.SetShapes(faces);
        builder.SetAvoidInternalShapes(true);
        builder.Perform();
        if(builder.HasErrors() || builder.HasWarnings() || builder.Areas().IsEmpty())
            throw std::runtime_error("Cannot make solids from the closed shells. Check the mesh winding");
        TopoDS_Shape shape;
        if(builder.Areas().Extent()==1) {
            shape=builder.Areas().First();
        } else {
            BRep_Builder b;
            TopoDS_Compound compound;
            b.MakeCompound(compound);
            for(TopTools_ListIteratorOfListOfShape it(builder.Areas());it.More();it.Next())
                b.Add(compound,it.Value());
            shape=compound;
        }
        TopTools_IndexedMapOfShape used;
        TopExp::MapShapes(shape,TopAbs_FACE,used);
        if(static_cast<size_t>(used.Extent())!=triangle_count)
            throw std::runtime_error("Some mesh faces do not bound a solid. Check the mesh winding");
        if(merge_planar) {
            ShapeUpgrade_UnifySameDomain merge(shape,true,true,false);
            merge.SetLinearTolerance(1e-7);
            merge.SetAngularTolerance(1e-9);
            merge.Build();
            shape=merge.Shape();
        }
        BRepAlgoAPI_Check check(shape,false,true);
        if(!check.IsValid()) throw std::runtime_error("Mesh faces intersect. Repair the mesh before conversion");
        GProp_GProps properties;
        BRepGProp::VolumeProperties(shape,properties);
        if(!(properties.Mass()>0.)) throw std::runtime_error("The mesh does not enclose positive volume");
        publish(shape,result);
    });
}

extern "C" int faber_offset(const void* input,double distance,void** result,char* error,size_t capacity) noexcept {
    return protect(error,capacity,[&] {
        BRepOffsetAPI_MakeOffsetShape operation;
        operation.PerformByJoin(static_cast<const FaberShape*>(input)->value,distance,1e-7);
        if(!operation.IsDone()) throw std::runtime_error("Offset failed");
        publish_tracked(operation,{static_cast<const FaberShape*>(input)->value},result,true);
    });
}

extern "C" int faber_thicken(const void* input,double thickness,void** result,char* error,size_t capacity) noexcept {
    return protect(error,capacity,[&] {
        BRepOffsetAPI_MakeThickSolid operation;
        operation.MakeThickSolidBySimple(static_cast<const FaberShape*>(input)->value,thickness);
        if(!operation.IsDone()) throw std::runtime_error("Thicken failed");
        if(operation.Shape().ShapeType()!=TopAbs_SOLID) throw std::runtime_error("Thicken needs one closed solid result");
        TopoDS_Solid solid=TopoDS::Solid(operation.Shape());
        if(!BRepLib::OrientClosedSolid(solid)) throw std::runtime_error("Cannot orient thickened solid");
        publish_tracked(operation,{static_cast<const FaberShape*>(input)->value},result,false,&solid);
    });
}

extern "C" int faber_face_shape(const void* input,uint32_t index,void** result,char* error,size_t capacity) noexcept {
    return protect(error,capacity,[&] {
        TopTools_IndexedMapOfShape faces;
        TopExp::MapShapes(static_cast<const FaberShape*>(input)->value,TopAbs_FACE,faces);
        if(index==0 || index>static_cast<uint32_t>(faces.Extent())) throw std::runtime_error("Unknown face index");
        publish(faces(index),result);
    });
}

static double surface_area(const TopoDS_Shape& shape) {
    GProp_GProps p;
    BRepGProp::SurfaceProperties(shape,p);
    return std::abs(p.Mass());
}

extern "C" int faber_inset_face(const void* input,uint32_t index,double distance,void** result,char* error,size_t capacity) noexcept {
    return protect(error,capacity,[&] {
        const TopoDS_Face face=TopoDS::Face(static_cast<const FaberShape*>(input)->value);
        const BRepAdaptor_Surface surface(face);
        if(surface.GetType()!=GeomAbs_Plane) throw std::runtime_error("Select a planar face for the rim");
        TopoDS_Wire outer=BRepTools::OuterWire(face), selected=outer;
        if(index>0) {
            uint32_t count=0;
            for(TopExp_Explorer it(face,TopAbs_WIRE);it.More();it.Next()) {
                if(it.Current().IsSame(outer)) continue;
                if(++count==index) selected=TopoDS::Wire(it.Current());
            }
            if(count<index) throw std::runtime_error("The selected face has no such inner loop");
            if(count>1 && index==1) throw std::runtime_error("The face has more than one inner loop");
        }
        const gp_Pln plane=surface.Plane();
        BRepBuilderAPI_MakeFace disk(plane,selected,true);
        if(!disk.IsDone()) throw std::runtime_error("Cannot make a face from the selected loop");
        TopoDS_Face output=disk.Face();
        if(distance!=0.) {
            BRepOffsetAPI_MakeOffset offset(output,GeomAbs_Intersection);
            offset.Perform(distance);
            if(!offset.IsDone()) throw std::runtime_error("The rim width collapses the selected loop");
            TopTools_IndexedMapOfShape wires; TopExp::MapShapes(offset.Shape(),TopAbs_WIRE,wires);
            if(wires.Extent()!=1) throw std::runtime_error("The rim width splits or collapses the selected loop");
            BRepBuilderAPI_MakeFace inset(plane,TopoDS::Wire(wires(1)),true);
            if(!inset.IsDone()) throw std::runtime_error("Cannot build the offset loop");
            output=inset.Face();
            if((surface_area(output)-surface_area(disk.Face()))*distance<=0.) {
                throw std::runtime_error("The rim width does not give a valid offset loop");
            }
        }
        publish(output,result);
    });
}

static double separation(const TopoDS_Shape& a,const TopoDS_Shape& b) {
    BRepExtrema_DistShapeShape distance(a,b);
    if(!distance.IsDone()) throw std::runtime_error("Cannot measure the wall limits");
    return distance.Value();
}

// Divide a curved face at its silhouettes. Each patch then faces one way
// over its full projection in the growth direction.
static TopoDS_Shape wall_patches(const TopoDS_Face& face,const gp_Vec& grow) {
    if(BRepAdaptor_Surface(face).GetType()==GeomAbs_Plane) return face;
    HLRTopoBRep_OutLiner outline(face);
    BRepTopAdaptor_MapOfShapeTool tools;
    outline.Fill(HLRAlgo_Projector(gp_Ax2(gp_Pnt(0,0,0),gp_Dir(grow))),tools,0);
    const auto& data=outline.DataStructure();
    if(!data.FaceHasIntL(face)) return face;
    TopTools_ListOfShape edges,arguments;
    for(TopTools_ListIteratorOfListOfShape it(data.FaceIntL(face));it.More();it.Next()) {
        const auto edge=TopoDS::Edge(it.Value().Oriented(TopAbs_FORWARD));
        if(!BRepLib::BuildCurve3d(edge)) throw std::runtime_error("Cannot resolve a curved wall limit");
        edges.Append(edge);
    }
    if(edges.IsEmpty()) return face;
    arguments.Append(face);
    BRepAlgoAPI_Splitter parts; parts.SetArguments(arguments); parts.SetTools(edges);
    parts.SetNonDestructive(true); parts.Build();
    if(!parts.IsDone() || parts.HasErrors()) throw std::runtime_error("Cannot split a curved wall limit");
    return parts.Shape();
}

// A void can wrap around a shelf. Remove the material downstream of each
// back-facing patch so each line of growth stops at its first contact.
static TopoDS_Shape first_wall_contact(const TopoDS_Shape& cell,const gp_Pnt& start,const gp_Vec& grow,double reach) {
    TopTools_ListOfShape shadows;
    Handle(IntTools_Context) context=new IntTools_Context();
    for(TopExp_Explorer faces(cell,TopAbs_FACE);faces.More();faces.Next()) {
        const auto original=TopoDS::Face(faces.Current());
        const auto patches=wall_patches(original,grow);
        for(TopExp_Explorer parts(patches,TopAbs_FACE);parts.More();parts.Next()) {
            const auto face=TopoDS::Face(parts.Current().Oriented(original.Orientation()));
            const BRepAdaptor_Surface surface(face);
            gp_Pnt point; gp_Vec normal;
            if(surface.GetType()==GeomAbs_Plane) {
                const auto plane=surface.Plane();
                point=plane.Location();
                normal=gp_Vec(plane.Axis().Direction());
                if(std::abs(normal.Dot(grow))>1.-1e-10 && std::abs(plane.Distance(start))<1e-7) continue;
            } else {
                gp_Pnt2d uv;
                if(BOPTools_AlgoTools3D::PointInFace(face,point,uv,context)!=0) {
                    throw std::runtime_error("Cannot identify a curved wall limit");
                }
                gp_Vec du,dv; surface.D1(uv.X(),uv.Y(),point,du,dv);
                normal=du.Crossed(dv).Normalized();
            }
            if(face.Orientation()==TopAbs_REVERSED) normal.Reverse();
            if(normal.Dot(grow)>=-1e-9) continue;
            BRepPrimAPI_MakePrism swept(face,grow*reach,true);
            if(!swept.IsDone() || swept.Shape().ShapeType()!=TopAbs_SOLID) throw std::runtime_error("Cannot trim a wall behind its first contact");
            auto solid=TopoDS::Solid(swept.Shape());
            if(!BRepLib::OrientClosedSolid(solid)) throw std::runtime_error("Cannot orient a wall limit");
            shadows.Append(solid);
        }
    }
    if(shadows.IsEmpty()) return cell;
    TopTools_ListOfShape arguments;arguments.Append(cell);
    BRepAlgoAPI_Cut trim;trim.SetArguments(arguments);trim.SetTools(shadows);trim.SetNonDestructive(true);trim.Build();
    if(!trim.IsDone() || trim.HasErrors()) throw std::runtime_error("Cannot trim the wall behind its first contact");
    return trim.Shape();
}

extern "C" int faber_linear_form(const void* raw_body,const void* raw_profile,const double* direction,
        double thickness,double offset,void** result,char* error,size_t capacity) noexcept {
    return protect(error,capacity,[&] {
        const auto& body=static_cast<const FaberShape*>(raw_body)->value;
        const auto& profile=static_cast<const FaberShape*>(raw_profile)->value;
        TopTools_IndexedMapOfShape edges;
        TopExp::MapShapes(profile,TopAbs_EDGE,edges);
        if(edges.Extent()!=1) throw std::runtime_error("Wall growth needs one line");
        const BRepAdaptor_Curve curve(TopoDS::Edge(edges(1)));
        if(curve.GetType()!=GeomAbs_Line) throw std::runtime_error("Wall growth needs a straight line");
        const gp_Pnt a=curve.Value(curve.FirstParameter()), b=curve.Value(curve.LastParameter());
        constexpr double tol=1e-6;
        if(separation(BRepBuilderAPI_MakeVertex(a).Vertex(),body)>tol) throw std::runtime_error("open start");
        if(separation(BRepBuilderAPI_MakeVertex(b).Vertex(),body)>tol) throw std::runtime_error("open end");
        const gp_Vec along=gp_Vec(a,b).Normalized();
        const gp_Vec grow=gp_Vec(direction[0],direction[1],direction[2]).Normalized();
        if(std::abs(along.Dot(grow))>1e-8) throw std::runtime_error("Wall growth must be perpendicular to the line");
        const gp_Vec side=along.Crossed(grow).Normalized();
        const gp_Vec low=side*(thickness*(offset-1.)/2.), high=side*(thickness*(offset+1.)/2.);
        BRepBuilderAPI_MakePolygon outline;
        outline.Add(a.Translated(low)); outline.Add(b.Translated(low));
        outline.Add(b.Translated(high)); outline.Add(a.Translated(high)); outline.Close();
        const auto seed=BRepBuilderAPI_MakeFace(outline.Wire()).Face();
        Bnd_Box bounds; BRepBndLib::Add(body,bounds);
        const double reach=std::sqrt(bounds.SquareExtent())*2.+thickness+1.;
        BRepPrimAPI_MakePrism prism(seed,grow*reach,true);
        BRepAlgoAPI_Cut split(prism.Shape(),body);
        if(!split.IsDone() || split.HasErrors()) throw std::runtime_error("Cannot trim the wall to the next faces");
        // Keep only cells that start at the profile and stop on the body.
        BRep_Builder builder; TopoDS_Compound walls; builder.MakeCompound(walls);
        double covered=0.;
        for(TopExp_Explorer it(split.Shape(),TopAbs_SOLID);it.More();it.Next()) {
            const auto& cell=it.Current();
            BRepAlgoAPI_Common contact(cell,seed);
            if(!contact.IsDone() || contact.HasErrors()) throw std::runtime_error("Cannot find the wall contact");
            const double area=surface_area(contact.Shape());
            if(area<=tol*tol) continue;
            const TopoDS_Shape visible=first_wall_contact(cell,a,grow,reach);
            if(separation(visible,prism.LastShape())<=tol) throw std::runtime_error("The wall does not reach a limiting face");
            covered+=area;
            builder.Add(walls,visible);
        }
        // Endpoints can extend into existing walls. Measure the free part of the centre line.
        BRepAlgoAPI_Cut free_line(profile,body);
        if(!free_line.IsDone() || free_line.HasErrors()) throw std::runtime_error("Cannot measure the open profile");
        GProp_GProps length; BRepGProp::LinearProperties(free_line.Shape(),length);
        const double expected=length.Mass()*thickness;
        if(covered<=tol*tol || covered+tol*(a.Distance(b)+thickness)<expected) {
            throw std::runtime_error("The wall thickness exceeds the free space at the profile");
        }
        BRepAlgoAPI_Fuse joined(body,walls);
        if(!joined.IsDone() || joined.HasErrors()) throw std::runtime_error("Cannot join the wall to the body");
        joined.SimplifyResult();
        TopTools_IndexedMapOfShape before,after;
        TopExp::MapShapes(body,TopAbs_SOLID,before); TopExp::MapShapes(joined.Shape(),TopAbs_SOLID,after);
        if(after.Extent()!=1 || before.Extent()!=1) throw std::runtime_error("The wall must join to one solid");
        publish_tracked(joined,{body},result,false,&after(1));
    });
}

extern "C" int faber_split(const void* input,const double* origin,const double* normal,int32_t keep,void** result,char* error,size_t capacity) noexcept {
    return protect(error,capacity,[&] {
        const auto& shape=static_cast<const FaberShape*>(input)->value;
        const gp_Pnt point(origin[0],origin[1],origin[2]);
        const gp_Dir direction(normal[0],normal[1],normal[2]);
        TopTools_ListOfShape arguments,tools;
        arguments.Append(shape);
        tools.Append(BRepBuilderAPI_MakeFace(gp_Pln(point,direction)).Face());
        BRepAlgoAPI_Splitter operation;
        operation.SetArguments(arguments);
        operation.SetTools(tools);
        operation.SetNonDestructive(true);
        operation.SetRunParallel(false);
        operation.SetToFillHistory(true);
        operation.Build();
        if(!operation.IsDone() || operation.HasErrors()) throw std::runtime_error("Split failed");
        TopTools_IndexedMapOfShape before,after;
        TopExp::MapShapes(shape,TopAbs_SOLID,before);
        TopExp::MapShapes(operation.Shape(),TopAbs_SOLID,after);
        if(after.Extent()<=before.Extent()) throw std::runtime_error("Plane does not split the source");
        BRep_Builder builder;
        TopoDS_Compound output;
        builder.MakeCompound(output);
        int count=0;
        for(int i=1;i<=after.Extent();++i) {
            GProp_GProps properties;
            BRepGProp::VolumeProperties(after(i),properties);
            if(properties.Mass()<=0.) throw std::runtime_error("Split produced a solid without positive volume");
            const double side=gp_Vec(point,properties.CentreOfMass()).Dot(gp_Vec(direction));
            if(keep==0 || (keep>0 && side>0.) || (keep<0 && side<0.)) {
                builder.Add(output,after(i));
                ++count;
            }
        }
        if(count==0) throw std::runtime_error("Split has no solid on the selected side");
        publish_tracked(operation,{shape},result,false,&output);
    });
}

extern "C" int faber_draft(const void* input,const uint32_t* indices,size_t count,const double* direction,const double* origin,const double* normal,double angle,void** result,char* error,size_t capacity) noexcept {
    return protect(error,capacity,[&] {
        const auto& shape=static_cast<const FaberShape*>(input)->value;
        TopTools_IndexedMapOfShape faces; TopExp::MapShapes(shape,TopAbs_FACE,faces);
        BRepOffsetAPI_DraftAngle operation(shape);
        const gp_Dir pull(direction[0],direction[1],direction[2]);
        const gp_Pln plane(gp_Pnt(origin[0],origin[1],origin[2]),gp_Dir(normal[0],normal[1],normal[2]));
        for(size_t i=0;i<count;++i) {
            if(indices[i]==0 || indices[i]>static_cast<uint32_t>(faces.Extent())) throw std::runtime_error("Unknown face index");
            operation.Add(TopoDS::Face(faces(indices[i])),pull,angle,plane);
            if(!operation.AddDone()) throw std::runtime_error("Cannot draft selected face");
        }
        operation.Build();
        if(!operation.IsDone()) throw std::runtime_error("Draft failed");
        publish_tracked(operation,{shape},result,true);
    });
}

extern "C" int faber_cylinder(double radius,double height,void** result,char* error,size_t capacity) noexcept {
    return protect(error,capacity,[&]{publish(BRepPrimAPI_MakeCylinder(radius,height).Shape(),result);});
}

static TopoDS_Wire ring_wire(const double* points,size_t count,double z,bool positive) {
    double area=0;
    for(size_t i=0;i<count;++i) {const size_t j=(i+1)%count;area+=points[2*i]*points[2*j+1]-points[2*j]*points[2*i+1];}
    BRepBuilderAPI_MakePolygon polygon;
    for(size_t i=0;i<count;++i) {
        const size_t j=((area>0)==positive) ? i : count-1-i;
        polygon.Add(gp_Pnt(points[2*j],points[2*j+1],z));
    }
    polygon.Close();
    if(!polygon.IsDone()) throw std::runtime_error("Cannot make profile boundary");
    return polygon.Wire();
}

extern "C" int faber_sketch_loop(int kind,const double* data,size_t count,void** result,char* error,size_t capacity) noexcept {
    return protect(error,capacity,[&]{
        if(kind==0) {
            const gp_Circ circle(gp_Ax2(gp_Pnt(data[0],data[1],0),gp_Dir(0,0,1)),data[2]);
            BRepBuilderAPI_MakeEdge edge(circle);BRepBuilderAPI_MakeWire wire(edge.Edge());
            publish(wire.Wire(),result);
        } else if(kind==2) {
            const gp_Elips ellipse(gp_Ax2(gp_Pnt(data[0],data[1],0),gp_Dir(0,0,1),gp_Dir(std::cos(data[4]),std::sin(data[4]),0)),data[2],data[3]);
            BRepBuilderAPI_MakeEdge edge(ellipse);BRepBuilderAPI_MakeWire wire(edge.Edge());
            publish(wire.Wire(),result);
        } else {
            publish(ring_wire(data,count,0,true),result);
        }
    });
}

static TopoDS_Edge spline_edge(gp_Pnt& current,const double* values,size_t count,size_t dimensions,const double* tangents,bool periodic) {
    Handle(TColgp_HArray1OfPnt) points=new TColgp_HArray1OfPnt(1,static_cast<int>(count)+1);
    points->SetValue(1,current);
    for(size_t i=0;i<count;++i) {
        const double* p=values+i*dimensions;
        points->SetValue(static_cast<int>(i)+2,gp_Pnt(p[0],p[1],dimensions==3 ? p[2] : 0.));
    }
    GeomAPI_Interpolate operation(points,periodic,1e-7);
    if(tangents) {
        const double* end=tangents+dimensions;
        operation.Load(gp_Vec(tangents[0],tangents[1],dimensions==3 ? tangents[2] : 0.),
                       gp_Vec(end[0],end[1],dimensions==3 ? end[2] : 0.),true);
    }
    operation.Perform();
    if(!operation.IsDone()) throw std::runtime_error("Cannot interpolate spline points");
    if(!periodic) current=points->Value(points->Upper());
    return BRepBuilderAPI_MakeEdge(operation.Curve()).Edge();
}

extern "C" int faber_sketch_path(const double* start,const FaberSegment* segments,size_t count,void** result,char* error,size_t capacity) noexcept {
    return protect(error,capacity,[&]{
        gp_Pnt current(start[0],start[1],0);BRepBuilderAPI_MakeWire wire;
        for(size_t i=0;i<count;++i) {
            const auto& segment=segments[i];const double* p=segment.data;
            if(segment.kind>=3 && segment.kind<=5) {
                wire.Add(spline_edge(current,segment.points,segment.count,2,segment.kind==4 ? p : nullptr,segment.kind==5));
            } else if(segment.kind==0) {
                const gp_Pnt end(p[0],p[1],0);wire.Add(BRepBuilderAPI_MakeEdge(current,end).Edge());current=end;
            } else if(segment.kind==1) {
                const gp_Pnt end(p[2],p[3],0);GC_MakeArcOfCircle arc(current,gp_Pnt(p[0],p[1],0),end);
                if(!arc.IsDone()) throw std::runtime_error("Cannot make sketch arc");
                wire.Add(BRepBuilderAPI_MakeEdge(arc.Value()).Edge());current=end;
            } else {
                const gp_Pnt end(p[4],p[5],0);TColgp_Array1OfPnt poles(1,4);
                poles.SetValue(1,current);poles.SetValue(2,gp_Pnt(p[0],p[1],0));poles.SetValue(3,gp_Pnt(p[2],p[3],0));poles.SetValue(4,end);
                Handle(Geom_BezierCurve) curve=new Geom_BezierCurve(poles);
                wire.Add(BRepBuilderAPI_MakeEdge(curve).Edge());current=end;
            }
            if(!wire.IsDone()) throw std::runtime_error("Cannot join sketch segments");
        }
        if(!wire.Wire().Closed()) throw std::runtime_error("Sketch path must be closed");
        TopoDS_Wire boundary=wire.Wire();
        BRepBuilderAPI_MakeFace plane(gp_Pln(gp_Pnt(0,0,0),gp_Dir(0,0,1)),boundary,true);
        Handle(ShapeExtend_WireData) data=new ShapeExtend_WireData(boundary);
        if(ShapeAnalysis::TotCross2D(data,plane.Face())<0) boundary.Reverse();
        publish(boundary,result);
    });
}

extern "C" int faber_helix(double radius,double pitch,double turns,void** result,char* error,size_t capacity) noexcept {
    return protect(error,capacity,[&] {
        const double tau=2.*std::acos(-1.);
        Handle(Geom_CylindricalSurface) surface=new Geom_CylindricalSurface(gp_Ax3(gp_Pnt(0,0,0),gp_Dir(0,0,1)),radius);
        Handle(Geom2d_Line) line=new Geom2d_Line(gp_Pnt2d(0,0),gp_Dir2d(tau,pitch));
        BRepBuilderAPI_MakeEdge edge(line,surface,0.,turns*std::hypot(tau,pitch));
        if(!edge.IsDone()) throw std::runtime_error("Cannot make helix edge");
        BRepBuilderAPI_MakeWire wire(edge.Edge());
        if(!wire.IsDone() || !BRepLib::BuildCurves3d(wire.Wire(),1e-7,GeomAbs_C1,14,static_cast<int>(std::ceil(turns))*32+30)) {
            throw std::runtime_error("Cannot build helix curve");
        }
        publish(wire.Wire(),result);
    });
}

extern "C" int faber_curve(const double* start,const FaberCurveSegment* segments,size_t count,void** result,char* error,size_t capacity) noexcept {
    return protect(error,capacity,[&]{
        gp_Pnt current(start[0],start[1],start[2]);BRepBuilderAPI_MakeWire wire;
        for(size_t i=0;i<count;++i) {
            const auto& segment=segments[i];const double* p=segment.data;
            if(segment.kind>=3 && segment.kind<=5) {
                wire.Add(spline_edge(current,segment.points,segment.count,3,segment.kind==4 ? p : nullptr,segment.kind==5));
            } else if(segment.kind==0) {
                const gp_Pnt end(p[0],p[1],p[2]);wire.Add(BRepBuilderAPI_MakeEdge(current,end).Edge());current=end;
            } else if(segment.kind==1) {
                const gp_Pnt end(p[3],p[4],p[5]);GC_MakeArcOfCircle arc(current,gp_Pnt(p[0],p[1],p[2]),end);
                if(!arc.IsDone()) throw std::runtime_error("Cannot make curve arc");
                wire.Add(BRepBuilderAPI_MakeEdge(arc.Value()).Edge());current=end;
            } else {
                const gp_Pnt end(p[6],p[7],p[8]);TColgp_Array1OfPnt poles(1,4);
                poles.SetValue(1,current);poles.SetValue(2,gp_Pnt(p[0],p[1],p[2]));poles.SetValue(3,gp_Pnt(p[3],p[4],p[5]));poles.SetValue(4,end);
                Handle(Geom_BezierCurve) curve=new Geom_BezierCurve(poles);wire.Add(BRepBuilderAPI_MakeEdge(curve).Edge());current=end;
            }
            if(!wire.IsDone()) throw std::runtime_error("Cannot join curve segments");
        }
        publish(wire.Wire(),result);
    });
}

extern "C" int faber_sweep_shape(const void* section_input,const void* path_input,int frenet,void** result,char* error,size_t capacity) noexcept {
    return protect(error,capacity,[&]{
        const auto& section=static_cast<const FaberShape*>(section_input)->value;
        const auto& path=static_cast<const FaberShape*>(path_input)->value;
        if(section.ShapeType()!=TopAbs_FACE || path.ShapeType()!=TopAbs_WIRE) throw std::runtime_error("Sweep needs a sketch and a curve path");
        const TopoDS_Face face=TopoDS::Face(section);const TopoDS_Wire spine=TopoDS::Wire(path);
        BRepTools_WireExplorer first(spine);if(!first.More()) throw std::runtime_error("Sweep path has no edges");
        BRepAdaptor_Curve curve(first.Current());gp_Pnt start;gp_Vec tangent;
        curve.D1(first.Current().Orientation()==TopAbs_REVERSED ? curve.LastParameter() : curve.FirstParameter(),start,tangent);
        BRepAdaptor_Surface surface(face);
        if(surface.GetType()!=GeomAbs_Plane || surface.Plane().Distance(start)>1e-6 || std::abs(surface.Plane().Axis().Direction().Dot(gp_Dir(tangent)))<1-1e-8) throw std::runtime_error("Sweep sketch must start normal to the path");
        auto make=[&](const TopoDS_Wire& wire) {
            BRepOffsetAPI_MakePipeShell operation(spine);operation.SetMode(frenet != 0);operation.SetTransitionMode(BRepBuilderAPI_RightCorner);
            operation.Add(wire,false,false);operation.Build();
            if(!operation.IsDone() || !operation.MakeSolid()) throw std::runtime_error("Cannot sweep sketch");
            return operation.Shape();
        };
        const TopoDS_Wire outer=BRepTools::OuterWire(face);TopoDS_Shape shape=make(outer);
        for(TopExp_Explorer it(face,TopAbs_WIRE);it.More();it.Next()) {
            if(it.Current().IsSame(outer)) continue;
            BRepAlgoAPI_Cut cut(shape,make(TopoDS::Wire(it.Current().Reversed())));
            if(!cut.IsDone() || cut.HasErrors()) throw std::runtime_error("Cannot make sweep hole");
            shape=cut.Shape();
        }
        publish(shape,result);
        static_cast<FaberShape*>(*result)->tracked=true;
    });
}

extern "C" int faber_sketch_face(const void* const* wires,size_t count,void** result,char* error,size_t capacity) noexcept {
    return protect(error,capacity,[&]{
        BRepBuilderAPI_MakeFace face(TopoDS::Wire(static_cast<const FaberShape*>(wires[0])->value));
        for(size_t i=1;i<count;++i) face.Add(TopoDS::Wire(static_cast<const FaberShape*>(wires[i])->value.Reversed()));
        if(!face.IsDone()) throw std::runtime_error("Cannot make sketch face");
        publish(face.Face(),result);
    });
}

extern "C" int faber_extrude_shape(const void* input,const double* vector,void** result,char* error,size_t capacity) noexcept {
    return protect(error,capacity,[&]{
        const auto& shape=static_cast<const FaberShape*>(input)->value;
        if(shape.ShapeType()!=TopAbs_FACE) throw std::runtime_error("Extrusion source must be a sketch face");
        BRepPrimAPI_MakePrism operation(shape,gp_Vec(vector[0],vector[1],vector[2]),true);
        if(!operation.IsDone()) throw std::runtime_error("Cannot extrude sketch");
        publish(operation.Shape(),result);
        static_cast<FaberShape*>(*result)->tracked=true;
    });
}

extern "C" int faber_curved(int kind,const double* dimensions,void** result,char* error,size_t capacity) noexcept {
    return protect(error,capacity,[&]{
        switch(kind) {
            case 0:publish(BRepPrimAPI_MakeSphere(dimensions[0]).Shape(),result);break;
            case 1:publish(BRepPrimAPI_MakeCone(dimensions[0],dimensions[1],dimensions[2]).Shape(),result);break;
            case 2:publish(BRepPrimAPI_MakeTorus(dimensions[0],dimensions[1]).Shape(),result);break;
            default:throw std::runtime_error("Unknown primitive");
        }
    });
}

extern "C" int faber_revolve_shape(const void* input,const double* origin,const double* direction,double angle,void** result,char* error,size_t capacity) noexcept {
    return protect(error,capacity,[&]{
        const auto& shape=static_cast<const FaberShape*>(input)->value;
        if(shape.ShapeType()!=TopAbs_FACE) throw std::runtime_error("Revolve source must be a sketch face");
        const gp_Ax1 axis(gp_Pnt(origin[0],origin[1],origin[2]),gp_Dir(direction[0],direction[1],direction[2]));
        BRepPrimAPI_MakeRevol operation(shape,axis,angle,true);
        if(!operation.IsDone()) throw std::runtime_error("Cannot revolve sketch");
        publish(operation.Shape(),result);
        static_cast<FaberShape*>(*result)->tracked=true;
    });
}

extern "C" int faber_loft_shapes(const void* const* inputs,size_t count,int ruled,void** result,char* error,size_t capacity) noexcept {
    return protect(error,capacity,[&]{
        std::vector<std::vector<TopoDS_Wire>> sections;
        for(size_t i=0;i<count;++i) {
            const auto& shape=static_cast<const FaberShape*>(inputs[i])->value;
            if(shape.ShapeType()!=TopAbs_FACE) throw std::runtime_error("Loft source must be a sketch face");
            const TopoDS_Wire outer=BRepTools::OuterWire(TopoDS::Face(shape));
            std::vector<TopoDS_Wire> wires{outer};
            for(TopExp_Explorer explorer(shape,TopAbs_WIRE);explorer.More();explorer.Next()) {
                if(!explorer.Current().IsSame(outer)) wires.push_back(TopoDS::Wire(explorer.Current().Reversed()));
            }
            if(!sections.empty() && wires.size()!=sections[0].size()) throw std::runtime_error("Loft sketches must have matching holes");
            sections.push_back(wires);
        }
        auto make=[&](size_t ring) {
            BRepOffsetAPI_ThruSections operation(true,ruled!=0,1e-6);operation.CheckCompatibility(true);
            for(const auto& section:sections) operation.AddWire(section[ring]);
            operation.Build();if(!operation.IsDone()) throw std::runtime_error("Cannot loft sketches");
            return operation.Shape();
        };
        TopoDS_Shape shape=make(0);
        for(size_t ring=1;ring<sections[0].size();++ring) {
            BRepAlgoAPI_Cut cut(shape,make(ring));
            if(!cut.IsDone() || cut.HasErrors()) throw std::runtime_error("Cannot make loft hole");
            shape=cut.Shape();
        }
        publish(shape,result);
        static_cast<FaberShape*>(*result)->tracked=true;
    });
}

extern "C" int faber_loft(const double* points,const size_t* counts,size_t sections,int ruled,void** result,char* error,size_t capacity) noexcept {
    return protect(error,capacity,[&]{
        BRepOffsetAPI_ThruSections operation(true,ruled!=0,1e-6);
        operation.CheckCompatibility(true);
        size_t offset=0;
        for(size_t s=0;s<sections;++s) {
            BRepBuilderAPI_MakePolygon polygon;
            for(size_t i=0;i<counts[s];++i) {
                polygon.Add(gp_Pnt(points[offset],points[offset+1],points[offset+2]));offset+=3;
            }
            polygon.Close();
            if(!polygon.IsDone()) throw std::runtime_error("Cannot make loft section");
            operation.AddWire(polygon.Wire());
        }
        operation.Build();
        if(!operation.IsDone()) throw std::runtime_error("Cannot loft sections");
        publish(operation.Shape(),result);
    });
}

extern "C" int faber_sweep(const double* profile,size_t profile_count,const double* path,size_t path_count,void** result,char* error,size_t capacity) noexcept {
    return protect(error,capacity,[&]{
        BRepBuilderAPI_MakePolygon spine,section;
        for(size_t i=0;i<path_count;++i) spine.Add(gp_Pnt(path[3*i],path[3*i+1],path[3*i+2]));
        for(size_t i=0;i<profile_count;++i) section.Add(gp_Pnt(profile[3*i],profile[3*i+1],profile[3*i+2]));
        section.Close();
        if(!spine.IsDone() || !section.IsDone()) throw std::runtime_error("Cannot make sweep wires");
        BRepOffsetAPI_MakePipeShell operation(spine.Wire());
        operation.SetMode(false);
        operation.SetTransitionMode(BRepBuilderAPI_RightCorner);
        operation.Add(section.Wire(),false,false);
        operation.Build();
        if(!operation.IsDone() || !operation.MakeSolid()) throw std::runtime_error("Cannot sweep profile");
        publish(operation.Shape(),result);
    });
}

extern "C" int faber_revolve(const double* points,const size_t* counts,size_t rings,const double* origin,const double* direction,double angle,void** result,char* error,size_t capacity) noexcept {
    return protect(error,capacity,[&]{
        BRepBuilderAPI_MakeFace face(ring_wire(points,counts[0],0,true));
        size_t offset=counts[0]*2;
        for(size_t i=1;i<rings;++i) {face.Add(ring_wire(points+offset,counts[i],0,false));offset+=counts[i]*2;}
        if(!face.IsDone()) throw std::runtime_error("Cannot make profile face");
        const gp_Ax1 axis(gp_Pnt(origin[0],origin[1],origin[2]),gp_Dir(direction[0],direction[1],direction[2]));
        BRepPrimAPI_MakeRevol operation(face.Face(),axis,angle,true);
        if(!operation.IsDone()) throw std::runtime_error("Cannot revolve profile");
        publish(operation.Shape(),result);
    });
}

extern "C" int faber_profile(const double* points,const size_t* counts,size_t rings,double height,double floor,void** result,char* error,size_t capacity) noexcept {
    return protect(error,capacity,[&]{
        BRepBuilderAPI_MakeFace face(ring_wire(points,counts[0],0,true));
        size_t offset=counts[0]*2;
        if(floor==0) {
            for(size_t i=1;i<rings;++i) {face.Add(ring_wire(points+offset,counts[i],0,false));offset+=counts[i]*2;}
        }
        if(!face.IsDone()) throw std::runtime_error("Cannot make profile face");
        TopoDS_Shape shape=BRepPrimAPI_MakePrism(face.Face(),gp_Vec(0,0,height)).Shape();
        if(floor>0) {
            for(size_t i=1;i<rings;++i) {
                BRepBuilderAPI_MakeFace hole(ring_wire(points+offset,counts[i],floor,true));offset+=counts[i]*2;
                const TopoDS_Shape tool=BRepPrimAPI_MakePrism(hole.Face(),gp_Vec(0,0,height-floor+1)).Shape();
                BRepAlgoAPI_Cut cut(shape,tool);
                if(!cut.IsDone() || cut.HasErrors()) throw std::runtime_error("Cannot make pocket");
                shape=cut.Shape();
            }
        }
        publish(shape,result);
    });
}

extern "C" int faber_transform(const void* input,const double* matrix,void** result,char* error,size_t capacity) noexcept {
    return protect(error,capacity,[&]{
        gp_Trsf transform;
        transform.SetValues(matrix[0],matrix[1],matrix[2],matrix[3],matrix[4],matrix[5],matrix[6],matrix[7],matrix[8],matrix[9],matrix[10],matrix[11]);
        BRepBuilderAPI_Transform operation(static_cast<const FaberShape*>(input)->value,transform,true);
        if(!operation.IsDone()) throw std::runtime_error("Cannot place shape");
          publish_tracked(operation,{static_cast<const FaberShape*>(input)->value},result);
    });
}

// Uniform factors use SetScale. Other factors use a general transform.
extern "C" int faber_scale(const void* input,const double* centre,const double* factor,void** result,char* error,size_t capacity) noexcept {
    return protect(error,capacity,[&]{
        const auto& shape=static_cast<const FaberShape*>(input)->value;
        if(factor[0]==factor[1] && factor[1]==factor[2]) {
            gp_Trsf transform;transform.SetScale(gp_Pnt(centre[0],centre[1],centre[2]),factor[0]);
            BRepBuilderAPI_Transform operation(shape,transform,true);
            if(!operation.IsDone()) throw std::runtime_error("Cannot scale shape");
            publish_tracked(operation,{shape},result);
        } else {
            gp_GTrsf transform;
            for(int i=1;i<=3;++i) {
                transform.SetValue(i,i,factor[i-1]);
                transform.SetValue(i,4,(1.-factor[i-1])*centre[i-1]);
            }
            BRepBuilderAPI_GTransform operation(shape,transform,true);
            if(!operation.IsDone()) throw std::runtime_error("Cannot scale shape");
            publish_tracked(operation,{shape},result);
        }
    });
}

// Writes point and tangent for each distance, six values per sample. Distances are arc lengths from the start.
extern "C" int faber_path_samples(const void* input,const double* distances,size_t count,double* result,double* length,char* error,size_t capacity) noexcept {
    return protect(error,capacity,[&]{
        const auto& shape=static_cast<const FaberShape*>(input)->value;
        if(shape.ShapeType()!=TopAbs_WIRE) throw std::runtime_error("Path must be a curve");
        BRepAdaptor_CompCurve curve(TopoDS::Wire(shape));
        const double first=curve.FirstParameter();
        *length=GCPnts_AbscissaPoint::Length(curve);
        for(size_t i=0;i<count;++i) {
            double u=first;
            if(distances[i]!=0.) {
                GCPnts_AbscissaPoint abscissa(curve,distances[i],first);
                if(!abscissa.IsDone()) throw std::runtime_error("Path distance is outside the curve");
                u=abscissa.Parameter();
            }
            gp_Pnt point;gp_Vec tangent;curve.D1(u,point,tangent);
            if(tangent.Magnitude()<=1e-12) throw std::runtime_error("Path has no tangent");
            result[6*i]=point.X();result[6*i+1]=point.Y();result[6*i+2]=point.Z();
            result[6*i+3]=tangent.X();result[6*i+4]=tangent.Y();result[6*i+5]=tangent.Z();
        }
    });
}

template<class Operation> static void combine(const TopoDS_Shape& a,const TopoDS_Shape& b,void** result) {
    TopTools_ListOfShape left,right;left.Append(a);right.Append(b);
    Operation operation;
    // Parallel mode: about 2.5 times faster on the swept jar cap. No loss on small solids.
    operation.SetArguments(left);operation.SetTools(right);operation.SetRunParallel(true);operation.SetToFillHistory(true);operation.Build();
    if(!operation.IsDone() || operation.HasErrors()) throw std::runtime_error("Boolean operation failed");
    publish_tracked(operation,{a,b},result);
}

extern "C" int faber_combine(const void* left,const void* right,int operation,void** result,char* error,size_t capacity) noexcept {
    return protect(error,capacity,[&]{
        const auto& a=static_cast<const FaberShape*>(left)->value;
        const auto& b=static_cast<const FaberShape*>(right)->value;
        switch(operation) {
            case 0:combine<BRepAlgoAPI_Fuse>(a,b,result);break;
            case 1:combine<BRepAlgoAPI_Cut>(a,b,result);break;
            case 2:combine<BRepAlgoAPI_Common>(a,b,result);break;
            default:throw std::runtime_error("Unknown boolean operation");
        }
    });
}

extern "C" int faber_distance(const void* a,const void* b,FaberDistance* result,char* error,size_t capacity) noexcept {
    return protect(error,capacity,[&] {
        // Multi-thread search: about five times faster on swept B-spline faces.
        BRepExtrema_DistShapeShape query;
        query.LoadS1(static_cast<const FaberShape*>(a)->value);
        query.LoadS2(static_cast<const FaberShape*>(b)->value);
        query.SetMultiThread(true);
        query.Perform();
        if(!query.IsDone() || query.NbSolution()<1) throw std::runtime_error("Distance measurement failed");
        result->distance=query.Value();
        const gp_Pnt& pa=query.PointOnShape1(1);
        const gp_Pnt& pb=query.PointOnShape2(1);
        result->point_a[0]=pa.X(); result->point_a[1]=pa.Y(); result->point_a[2]=pa.Z();
        result->point_b[0]=pb.X(); result->point_b[1]=pb.Y(); result->point_b[2]=pb.Z();
    });
}

static bool has_adaptive_geometry(const TopoDS_Shape& shape) {
    bool adaptive=false;
    for(TopExp_Explorer it(shape,TopAbs_FACE);it.More() && !adaptive;it.Next()) {
        const auto type=BRepAdaptor_Surface(TopoDS::Face(it.Current())).GetType();
        adaptive=type==GeomAbs_BSplineSurface || type==GeomAbs_BezierSurface;
    }
    for(TopExp_Explorer it(shape,TopAbs_EDGE);it.More() && !adaptive;it.Next()) {
        const auto type=BRepAdaptor_Curve(TopoDS::Edge(it.Current())).GetType();
        adaptive=type==GeomAbs_BSplineCurve || type==GeomAbs_BezierCurve;
    }
    return adaptive;
}

static void fill_properties(const TopoDS_Shape& shape,FaberProperties* result,bool with_bounds) {
    GProp_GProps mass;
    if(has_adaptive_geometry(shape)) {
        const double accuracy=BRepGProp::VolumeProperties(shape,mass,1e-9);
        if(accuracy<0.) throw std::runtime_error("Cannot integrate shape volume");
    } else {
        BRepGProp::VolumeProperties(shape,mass);
    }
    result->volume=mass.Mass();const gp_Pnt centre=mass.CentreOfMass();
    result->centre[0]=centre.X();result->centre[1]=centre.Y();result->centre[2]=centre.Z();
    if(with_bounds) {
        // The optimal box is slow. Feature checks do not need it.
        Bnd_Box box;BRepBndLib::AddOptimal(shape,box,false,false);
        if(box.IsVoid()) throw std::runtime_error("Shape has no bounds");
        box.Get(result->bounds[0],result->bounds[1],result->bounds[2],result->bounds[3],result->bounds[4],result->bounds[5]);
    }
    TopTools_IndexedMapOfShape faces,edges,solids;
    TopExp::MapShapes(shape,TopAbs_FACE,faces);TopExp::MapShapes(shape,TopAbs_EDGE,edges);TopExp::MapShapes(shape,TopAbs_SOLID,solids);
    result->faces=faces.Extent();result->edges=edges.Extent();result->solids=solids.Extent();result->valid=BRepCheck_Analyzer(shape).IsValid();
}

extern "C" int faber_properties(const void* input,FaberProperties* result,char* error,size_t capacity) noexcept {
    return protect(error,capacity,[&]{
        fill_properties(static_cast<const FaberShape*>(input)->value,result,true);
    });
}

extern "C" int faber_summary_properties(const void* input,FaberProperties* result,char* error,size_t capacity) noexcept {
    return protect(error,capacity,[&]{
        fill_properties(static_cast<const FaberShape*>(input)->value,result,false);
    });
}

extern "C" int faber_mass_properties(const void* input,FaberMassProperties* result,char* error,size_t capacity) noexcept {
    return protect(error,capacity,[&]{
        const auto& shape=static_cast<const FaberShape*>(input)->value;
        GProp_GProps volume,surface;
        if(has_adaptive_geometry(shape)) {
            if(BRepGProp::VolumeProperties(shape,volume,1e-9)<0.) throw std::runtime_error("Cannot integrate shape volume");
            if(BRepGProp::SurfaceProperties(shape,surface,1e-9)<0.) throw std::runtime_error("Cannot integrate shape area");
        } else {
            BRepGProp::VolumeProperties(shape,volume);
            BRepGProp::SurfaceProperties(shape,surface);
        }
        result->volume=volume.Mass();
        result->area=surface.Mass();
        const gp_Pnt centre=volume.CentreOfMass();
        result->centre[0]=centre.X();result->centre[1]=centre.Y();result->centre[2]=centre.Z();
        const gp_Mat inertia=volume.MatrixOfInertia();
        for(int row=0;row<3;++row) for(int column=0;column<3;++column) result->inertia[row*3+column]=inertia.Value(row+1,column+1);
    });
}

// True for a face on a non-plane surface whose samples lie within 1e-7 mm of one plane.
// GeomLib_IsPlanarSurface reports these B-spline faces as not planar, so it is not used.
static bool planar_face_normal(const TopoDS_Face& face,const BRepAdaptor_Surface& surface,gp_Dir& normal) {
    double u0,u1,v0,v1;BRepTools::UVBounds(face,u0,u1,v0,v1);
    gp_Pnt point;gp_Vec du,dv;surface.D1((u0+u1)*0.5,(v0+v1)*0.5,point,du,dv);
    const gp_Vec cross=du.Crossed(dv);
    if(cross.Magnitude()<1e-12) return false;
    const gp_Dir n(cross);
    for(int i=0;i<=16;++i) for(int j=0;j<=16;++j) {
        gp_Pnt sample;surface.D0(u0+(u1-u0)*i/16.,v0+(v1-v0)*j/16.,sample);
        if(std::abs(gp_Vec(point,sample).Dot(gp_Vec(n)))>1e-7) return false;
    }
    normal=n;
    return true;
}

// True for a straight edge within 1e-7 mm. The direction runs from start to end.
static bool straight_edge_direction(const BRepAdaptor_Curve& curve,gp_Dir& direction) {
    const double first=curve.FirstParameter(),last=curve.LastParameter();
    const gp_Pnt start=curve.Value(first),end=curve.Value(last);
    const gp_Vec chord(start,end);
    if(chord.Magnitude()<1e-9) return false;
    direction=gp_Dir(chord);
    for(int i=1;i<16;++i) {
        const gp_Vec offset(start,curve.Value(first+(last-first)*i/16.));
        if(offset.Crossed(gp_Vec(direction)).Magnitude()>1e-7) return false;
    }
    return true;
}

extern "C" int faber_edges(const void* input,FaberEdge* result,size_t capacity_edges,size_t* count,char* error,size_t capacity) noexcept {
    return protect(error,capacity,[&]{
        TopTools_IndexedMapOfShape edges;TopExp::MapShapes(static_cast<const FaberShape*>(input)->value,TopAbs_EDGE,edges);
        if(capacity_edges < static_cast<size_t>(edges.Extent())) throw std::runtime_error("Edge buffer is too small");
        *count=0;
        for(int i=1;i<=edges.Extent();++i) {
            const TopoDS_Edge edge=TopoDS::Edge(edges(i));
            if(BRep_Tool::Degenerated(edge)) continue;
            FaberEdge& out=result[(*count)++];out={};out.index=i;out.radius=-1;
            GProp_GProps mass;BRepGProp::LinearProperties(edge,mass);out.length=mass.Mass();
            const gp_Pnt centre=mass.CentreOfMass();out.centre[0]=centre.X();out.centre[1]=centre.Y();out.centre[2]=centre.Z();
            BRepAdaptor_Curve curve(edge);
            if(curve.GetType()==GeomAbs_Line) {const gp_Dir d=curve.Line().Direction();out.direction[0]=d.X();out.direction[1]=d.Y();out.direction[2]=d.Z();}
            else if(curve.GetType()!=GeomAbs_Circle) {gp_Dir d;if(straight_edge_direction(curve,d)) {out.direction[0]=d.X();out.direction[1]=d.Y();out.direction[2]=d.Z();}}
            if(curve.GetType()==GeomAbs_Circle) out.radius=curve.Circle().Radius();
        }
    });
}

extern "C" int faber_edge_faces(const void* input,FaberEdgeFace* result,size_t capacity_links,size_t* count,char* error,size_t capacity) noexcept {
    return protect(error,capacity,[&] {
        const auto& shape=static_cast<const FaberShape*>(input)->value;
        TopTools_IndexedMapOfShape faces,edges;
        TopExp::MapShapes(shape,TopAbs_FACE,faces);
        TopExp::MapShapes(shape,TopAbs_EDGE,edges);
        std::vector<FaberEdgeFace> links;
        for(int f=1;f<=faces.Extent();++f) {
            TopTools_IndexedMapOfShape boundary;
            TopExp::MapShapes(faces(f),TopAbs_EDGE,boundary);
            for(int e=1;e<=boundary.Extent();++e) {
                const int index=edges.FindIndex(boundary(e));
                if(index==0) throw std::runtime_error("Face edge is missing from the shape");
                links.push_back({static_cast<uint32_t>(index),static_cast<uint32_t>(f)});
            }
        }
        *count=links.size();
        if(!result) return;
        if(capacity_links<links.size()) throw std::runtime_error("Adjacency buffer is too small");
        for(size_t i=0;i<links.size();++i) result[i]=links[i];
    });
}

extern "C" int faber_faces(const void* input,FaberFace* result,size_t capacity_faces,size_t* count,char* error,size_t capacity) noexcept {
    return protect(error,capacity,[&]{
        TopTools_IndexedMapOfShape faces;TopExp::MapShapes(static_cast<const FaberShape*>(input)->value,TopAbs_FACE,faces);
        if(capacity_faces<static_cast<size_t>(faces.Extent())) throw std::runtime_error("Face buffer is too small");
        *count=faces.Extent();
        for(int i=1;i<=faces.Extent();++i) {
            const TopoDS_Face face=TopoDS::Face(faces(i));FaberFace& out=result[i-1];out={};out.index=i;
            GProp_GProps mass;BRepGProp::SurfaceProperties(face,mass);out.area=mass.Mass();
            const gp_Pnt centre=mass.CentreOfMass();out.centre[0]=centre.X();out.centre[1]=centre.Y();out.centre[2]=centre.Z();
            BRepAdaptor_Surface surface(face);
            if(surface.GetType()==GeomAbs_Cylinder) {
                const auto cylinder=surface.Cylinder();
                const auto axis=cylinder.Axis().Direction();
                double u0,u1,v0,v1;BRepTools::UVBounds(face,u0,u1,v0,v1);
                const auto start=cylinder.Location().Translated(gp_Vec(axis)*v0);
                start.Coord(out.start[0],out.start[1],out.start[2]);
                axis.Coord(out.axis[0],out.axis[1],out.axis[2]);
                out.radius=cylinder.Radius();out.length=v1-v0;
                out.internal=(face.Orientation()==TopAbs_REVERSED)==cylinder.Direct();
            }
            gp_Dir normal;bool planar=false;
            if(surface.GetType()==GeomAbs_Plane) {
                double u0,u1,v0,v1;BRepTools::UVBounds(face,u0,u1,v0,v1);
                gp_Pnt point;gp_Vec du,dv;surface.D1((u0+u1)*0.5,(v0+v1)*0.5,point,du,dv);
                normal=gp_Dir(du.Crossed(dv));planar=true;
            } else planar=planar_face_normal(face,surface,normal);
            if(planar) {
                if(face.Orientation()==TopAbs_REVERSED) normal.Reverse();
                out.normal[0]=normal.X();out.normal[1]=normal.Y();out.normal[2]=normal.Z();
            }
        }
    });
}

extern "C" int faber_shell(const void* input,const uint32_t* indices,size_t count,double thickness,const uint32_t* offset_faces,const double* offset_values,size_t offset_count,void** result,char* error,size_t capacity) noexcept {
    return protect(error,capacity,[&]{
        const auto& shape=static_cast<const FaberShape*>(input)->value;
        TopTools_IndexedMapOfShape faces;TopExp::MapShapes(shape,TopAbs_FACE,faces);
        auto face_at=[&](uint32_t index) {
            if(index==0 || index>static_cast<uint32_t>(faces.Extent())) throw std::runtime_error("Unknown face index");
            return TopoDS::Face(faces(index));
        };
        BRepOffset_MakeOffset operation;
        operation.Initialize(shape,-thickness,1e-6,BRepOffset_Skin,false,false,GeomAbs_Intersection,false,true);
        for(size_t i=0;i<count;++i) operation.AddFace(face_at(indices[i]));
        for(size_t i=0;i<offset_count;++i) operation.SetOffsetOnFace(face_at(offset_faces[i]),-offset_values[i]);
        operation.MakeThickSolid();
        if(!operation.IsDone()) throw std::runtime_error("Cannot shell selected solid");
        publish_tracked(operation,{shape},result);
    });
}

extern "C" int faber_dress(const void* input,const uint32_t* indices,size_t count,double size,int chamfer,void** result,char* error,size_t capacity) noexcept {
    return protect(error,capacity,[&]{
        const auto& shape=static_cast<const FaberShape*>(input)->value;
        TopTools_IndexedMapOfShape edges;TopExp::MapShapes(shape,TopAbs_EDGE,edges);
        for(size_t i=0;i<count;++i) if(indices[i]==0 || indices[i]>static_cast<uint32_t>(edges.Extent())) throw std::runtime_error("Unknown edge index");
        if(chamfer) {
            BRepFilletAPI_MakeChamfer operation(shape);
            for(size_t i=0;i<count;++i) operation.Add(size,TopoDS::Edge(edges(indices[i])));
            operation.Build();
            if(!operation.IsDone()) throw std::runtime_error("Cannot chamfer selected edges");
            publish_tracked(operation,{shape},result);
        } else {
            BRepFilletAPI_MakeFillet operation(shape);
            for(size_t i=0;i<count;++i) operation.Add(size,TopoDS::Edge(edges(indices[i])));
            operation.Build();
            if(!operation.IsDone()) throw std::runtime_error("Cannot fillet selected edges");
            publish_tracked(operation,{shape},result);
        }
    });
}

// Returns the face for one chamfer edge. Index 0 takes the first face of the edge.
static TopoDS_Face dress_face(const TopTools_IndexedDataMapOfShapeListOfShape& edge_faces,const TopoDS_Edge& edge,const TopTools_IndexedMapOfShape& faces,uint32_t index) {
    if(!edge_faces.Contains(edge)) throw std::runtime_error("Edge has no face");
    const TopTools_ListOfShape& owners=edge_faces.FindFromKey(edge);
    if(index==0) return TopoDS::Face(owners.First());
    if(index>static_cast<uint32_t>(faces.Extent())) throw std::runtime_error("Unknown face index");
    const TopoDS_Shape& face=faces(index);
    for(TopTools_ListIteratorOfListOfShape it(owners);it.More();it.Next()) {
        if(it.Value().IsSame(face)) return TopoDS::Face(face);
    }
    throw std::runtime_error("Chamfer face does not touch the edge");
}

extern "C" int faber_chamfer_two(const void* input,const uint32_t* indices,size_t count,double first,double second,uint32_t face,void** result,char* error,size_t capacity) noexcept {
    return protect(error,capacity,[&]{
        const auto& shape=static_cast<const FaberShape*>(input)->value;
        TopTools_IndexedMapOfShape edges;TopExp::MapShapes(shape,TopAbs_EDGE,edges);
        TopTools_IndexedMapOfShape faces;TopExp::MapShapes(shape,TopAbs_FACE,faces);
        TopTools_IndexedDataMapOfShapeListOfShape edge_faces;TopExp::MapShapesAndAncestors(shape,TopAbs_EDGE,TopAbs_FACE,edge_faces);
        for(size_t i=0;i<count;++i) if(indices[i]==0 || indices[i]>static_cast<uint32_t>(edges.Extent())) throw std::runtime_error("Unknown edge index");
        BRepFilletAPI_MakeChamfer operation(shape);
        for(size_t i=0;i<count;++i) {
            const TopoDS_Edge edge=TopoDS::Edge(edges(indices[i]));
            operation.Add(first,second,edge,dress_face(edge_faces,edge,faces,face));
        }
        operation.Build();
        if(!operation.IsDone()) throw std::runtime_error("Cannot chamfer selected edges");
        publish_tracked(operation,{shape},result);
    });
}

// The angle is in radians.
extern "C" int faber_chamfer_angle(const void* input,const uint32_t* indices,size_t count,double distance,double angle,uint32_t face,void** result,char* error,size_t capacity) noexcept {
    return protect(error,capacity,[&]{
        const auto& shape=static_cast<const FaberShape*>(input)->value;
        TopTools_IndexedMapOfShape edges;TopExp::MapShapes(shape,TopAbs_EDGE,edges);
        TopTools_IndexedMapOfShape faces;TopExp::MapShapes(shape,TopAbs_FACE,faces);
        TopTools_IndexedDataMapOfShapeListOfShape edge_faces;TopExp::MapShapesAndAncestors(shape,TopAbs_EDGE,TopAbs_FACE,edge_faces);
        for(size_t i=0;i<count;++i) if(indices[i]==0 || indices[i]>static_cast<uint32_t>(edges.Extent())) throw std::runtime_error("Unknown edge index");
        BRepFilletAPI_MakeChamfer operation(shape);
        for(size_t i=0;i<count;++i) {
            const TopoDS_Edge edge=TopoDS::Edge(edges(indices[i]));
            operation.AddDA(distance,angle,edge,dress_face(edge_faces,edge,faces,face));
        }
        operation.Build();
        if(!operation.IsDone()) throw std::runtime_error("Cannot chamfer selected edges");
        publish_tracked(operation,{shape},result);
    });
}

extern "C" int faber_fillet_variable(const void* input,const uint32_t* indices,size_t count,double start,double finish,void** result,char* error,size_t capacity) noexcept {
    return protect(error,capacity,[&]{
        const auto& shape=static_cast<const FaberShape*>(input)->value;
        TopTools_IndexedMapOfShape edges;TopExp::MapShapes(shape,TopAbs_EDGE,edges);
        for(size_t i=0;i<count;++i) if(indices[i]==0 || indices[i]>static_cast<uint32_t>(edges.Extent())) throw std::runtime_error("Unknown edge index");
        BRepFilletAPI_MakeFillet operation(shape);
        for(size_t i=0;i<count;++i) operation.Add(start,finish,TopoDS::Edge(edges(indices[i])));
        operation.Build();
        if(!operation.IsDone()) throw std::runtime_error("Cannot fillet selected edges");
        publish_tracked(operation,{shape},result);
    });
}

extern "C" int faber_mesh(const void* input,double tolerance,void** result,char* error,size_t capacity) noexcept {
    return protect(error,capacity,[&]{
        const auto& shape=static_cast<const FaberShape*>(input)->value;
        BRepMesh_IncrementalMesh operation(shape,tolerance,false,0.1,false);
        if(!operation.IsDone()) throw std::runtime_error("Cannot make mesh");
        auto mesh=std::make_unique<FaberMesh>();
        TopTools_IndexedMapOfShape faces;
        TopExp::MapShapes(shape,TopAbs_FACE,faces);
        for(int f=1;f<=faces.Extent();++f) {
            const TopoDS_Face face=TopoDS::Face(faces(f));TopLoc_Location location;
            const Handle(Poly_Triangulation) triangles=BRep_Tool::Triangulation(face,location);
            if(triangles.IsNull()) throw std::runtime_error("Face has no mesh");
            BRepLib_ToolTriangulatedShape::ComputeNormals(face,triangles);
            const uint32_t base=static_cast<uint32_t>(mesh->vertices.size()/3);
            for(int i=1;i<=triangles->NbNodes();++i) {
                const gp_Pnt p=triangles->Node(i).Transformed(location.Transformation());
                mesh->vertices.insert(mesh->vertices.end(),{p.X(),p.Y(),p.Z()});
                gp_Dir normal=triangles->Normal(i).Transformed(location.Transformation());
                if(face.Orientation()==TopAbs_REVERSED) normal.Reverse();
                mesh->normals.insert(mesh->normals.end(),{normal.X(),normal.Y(),normal.Z()});
            }
            for(int i=1;i<=triangles->NbTriangles();++i) {
                int a,b,c;triangles->Triangle(i).Get(a,b,c);
                if(face.Orientation()==TopAbs_REVERSED) std::swap(b,c);
                mesh->triangles.insert(mesh->triangles.end(),{base+a-1,base+b-1,base+c-1});
            }
            mesh->face_of_triangle.insert(mesh->face_of_triangle.end(),triangles->NbTriangles(),static_cast<uint32_t>(f));
        }
        *result=mesh.release();
    });
}

extern "C" void faber_mesh_data(const void* input,const double** vertices,size_t* vertex_count,const uint32_t** triangles,size_t* triangle_count,const double** normals) noexcept {
    const auto* mesh=static_cast<const FaberMesh*>(input);
    *vertices=mesh->vertices.data();*vertex_count=mesh->vertices.size()/3;
    *triangles=mesh->triangles.data();*triangle_count=mesh->triangles.size()/3;
    *normals=mesh->normals.data();
}

extern "C" const uint32_t* faber_mesh_face_indices(const void* input) noexcept {
    return static_cast<const FaberMesh*>(input)->face_of_triangle.data();
}

struct FaberEdgeLines {
    std::vector<double> points;
    std::vector<uint32_t> ends;
};

extern "C" int faber_edge_lines(const void* shape,double tolerance,FaberEdgeLineData* result,char* error,size_t capacity) noexcept {
    return protect(error,capacity,[&] {
        TopTools_IndexedMapOfShape edges;
        TopExp::MapShapes(static_cast<const FaberShape*>(shape)->value,TopAbs_EDGE,edges);
        auto lines=std::make_unique<FaberEdgeLines>();
        lines->ends.reserve(edges.Extent());
        for(int index=1;index<=edges.Extent();++index) {
            const auto edge=TopoDS::Edge(edges(index));
            if(!BRep_Tool::Degenerated(edge)) {
                BRepAdaptor_Curve curve(edge);
                GCPnts_TangentialDeflection samples(curve,0.1,tolerance);
                if(samples.NbPoints()<2) throw std::runtime_error("Edge has too few points");
                if(lines->points.size()/3+samples.NbPoints()>UINT32_MAX)
                    throw std::runtime_error("Edge lines exceed the point index limit");
                for(int n=1;n<=samples.NbPoints();++n) {
                    const auto point=samples.Value(n);
                    lines->points.insert(lines->points.end(),{point.X(),point.Y(),point.Z()});
                }
            }
            lines->ends.push_back(static_cast<uint32_t>(lines->points.size()/3));
        }
        *result={lines.get(),lines->points.data(),lines->points.size()/3,lines->ends.data(),lines->ends.size()};
        lines.release();
    });
}

extern "C" void faber_free_edge_lines(void* lines) noexcept {
    delete static_cast<FaberEdgeLines*>(lines);
}

extern "C" int faber_write(const void* input,const char* path,char* error,size_t capacity) noexcept {
    return protect(error,capacity,[&]{
        STEPControl_Writer writer;
        if(writer.Transfer(static_cast<const FaberShape*>(input)->value,STEPControl_AsIs)!=IFSelect_RetDone || writer.Write(path)!=IFSelect_RetDone) throw std::runtime_error("Cannot write exact shape");
    });
}

extern "C" int faber_read(const char* path,void** result,char* error,size_t capacity) noexcept {
    return protect(error,capacity,[&]{
        STEPControl_Reader reader;
        if(reader.ReadFile(path)!=IFSelect_RetDone || reader.TransferRoots()==0) throw std::runtime_error("Cannot read exact shape");
        publish(reader.OneShape(),result);
    });
}

extern "C" int faber_face_count(const void* input,size_t* count,char* error,size_t capacity) noexcept {
    return protect(error,capacity,[&]{
        TopTools_IndexedMapOfShape faces;TopExp::MapShapes(static_cast<const FaberShape*>(input)->value,TopAbs_FACE,faces);
        *count=faces.Extent();
    });
}

// Binary BRep form. The bytes keep the exact geometry and the face order.
extern "C" int faber_encode(const void* input,void** data,size_t* length,char* error,size_t capacity) noexcept {
    return protect(error,capacity,[&]{
        std::ostringstream stream(std::ios::binary);
        BinTools::Write(static_cast<const FaberShape*>(input)->value,stream);
        if(!stream) throw std::runtime_error("Cannot encode shape");
        const std::string text=stream.str();
        char* copy=new char[text.size()+1];
        std::memcpy(copy,text.data(),text.size());
        *data=copy;*length=text.size();
    });
}

extern "C" void faber_free_bytes(void* data) noexcept {delete[] static_cast<char*>(data);}

extern "C" int faber_decode(const void* data,size_t length,void** result,char* error,size_t capacity) noexcept {
    return protect(error,capacity,[&]{
        std::istringstream stream(std::string(static_cast<const char*>(data),length),std::ios::binary);
        TopoDS_Shape shape;
        BinTools::Read(shape,stream);
        if(shape.IsNull()) throw std::runtime_error("Cannot decode shape");
        *result=new FaberShape{shape};
    });
}

extern "C" void faber_free_shape(void* shape) noexcept {try {delete static_cast<FaberShape*>(shape);} catch(...) {}}
extern "C" void faber_free_mesh(void* mesh) noexcept {try {delete static_cast<FaberMesh*>(mesh);} catch(...) {}}

// OCCT version of the linked library. The text is a static string.
extern "C" const char* faber_occt_version() noexcept {return OCC_VERSION_COMPLETE;}

// Sets the thread count of the default OCCT pool. Call it before any parallel work.
extern "C" int faber_limit_threads(int count,char* error,size_t capacity) noexcept {
    return protect(error,capacity,[&]{OSD_ThreadPool::DefaultPool(count)->Init(count);});
}

// Threads that the default OCCT pool uses, including the calling thread.
extern "C" int faber_thread_count() noexcept {
    try {return OSD_ThreadPool::DefaultPool()->NbThreads();} catch(...) {return 0;}
}
