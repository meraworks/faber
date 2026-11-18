#include <HLRBRep_Algo.hxx>
#include <HLRBRep_HLRToShape.hxx>
#include <GCPnts_QuasiUniformDeflection.hxx>

struct DrawingCurves {
    std::vector<FaberCurve2> curves;
    std::vector<double> points;

    void point(const gp_Pnt& p) {
        points.push_back(p.X());
        points.push_back(p.Y());
    }
    void add(const TopoDS_Shape& edges, bool hidden) {
        if(edges.IsNull()) return;
        constexpr double turn=6.28318530717958647692;
        for(TopExp_Explorer it(edges,TopAbs_EDGE);it.More();it.Next()) {
            const TopoDS_Edge& edge=TopoDS::Edge(it.Current());
            if(BRep_Tool::Degenerated(edge)) continue;
            BRepAdaptor_Curve curve(edge);
            const double lo=curve.FirstParameter(), hi=curve.LastParameter();
            if(hi<=lo) continue;
            const gp_Pnt a=curve.Value(lo), b=curve.Value(hi);
            FaberCurve2 record{};
            record.hidden=hidden;
            record.offset=points.size()/2;
            if(curve.GetType()==GeomAbs_Line) {
                if(a.SquareDistance(b)<1e-16) continue;
                record.kind=0;
                point(a);point(b);
            } else if(curve.GetType()==GeomAbs_Circle) {
                const gp_Circ circle=curve.Circle();
                const auto centre=circle.Location();
                record.data[0]=centre.X();
                record.data[1]=centre.Y();
                record.data[2]=circle.Radius();
                record.kind=(hi-lo>=turn-1e-8)?1:2;
                const auto start=circle.Axis().Direction().Z()>=0.?a:b;
                record.data[3]=std::atan2(start.Y()-centre.Y(),start.X()-centre.X())*360./turn;
                record.data[4]=record.data[3]+(hi-lo)*360./turn;
            } else {
                record.kind=3;
                GCPnts_QuasiUniformDeflection samples(curve,0.009,lo,hi,GeomAbs_C0);
                if(!samples.IsDone() || samples.NbPoints()<2)
                    throw std::runtime_error("Cannot sample the projected edge within 0.01 mm");
                for(int index=1;index<=samples.NbPoints();++index) point(samples.Value(index));
            }
            record.count=points.size()/2-record.offset;
            curves.push_back(record);
        }
    }
};

extern "C" int faber_project_edges(const void* const* inputs,size_t count,const double* origin,
    const double* x,const double* y,int hidden,FaberDrawingResult* result,char* error,size_t capacity) noexcept {
    return protect(error,capacity,[&] {
        const gp_Vec right(x[0],x[1],x[2]), up(y[0],y[1],y[2]);
        const gp_Ax2 frame(gp_Pnt(origin[0],origin[1],origin[2]),gp_Dir(right.Crossed(up)),gp_Dir(right));
        Handle(HLRBRep_Algo) hlr=new HLRBRep_Algo;
        hlr->Projector(HLRAlgo_Projector(frame));
        for(size_t i=0;i<count;++i) hlr->Add(static_cast<const FaberShape*>(inputs[i])->value);
        hlr->Update();
        hlr->Hide();
        HLRBRep_HLRToShape edges(hlr);
        auto output=std::make_unique<DrawingCurves>();
        output->add(edges.VCompound(),false);
        output->add(edges.OutLineVCompound(),false);
        if(hidden) {
            output->add(edges.HCompound(),true);
            output->add(edges.OutLineHCompound(),true);
        }
        result->curves=output->curves.data();
        result->curve_count=output->curves.size();
        result->points=output->points.data();
        result->point_count=output->points.size()/2;
        result->owner=output.release();
    });
}

extern "C" void faber_free_drawing(void* owner) noexcept {delete static_cast<DrawingCurves*>(owner);}

extern "C" int faber_section_face(const void* input,const double* origin,const double* normal,
    void** result,char* error,size_t capacity) noexcept {
    return protect(error,capacity,[&] {
        const auto& shape=static_cast<const FaberShape*>(input)->value;
        const gp_Pln plane(gp_Pnt(origin[0],origin[1],origin[2]),gp_Dir(normal[0],normal[1],normal[2]));
        const auto tool=BRepBuilderAPI_MakeFace(plane).Face();
        BRepAlgoAPI_Common cut(shape,tool);
        if(!cut.IsDone()) throw std::runtime_error("Cannot intersect the part with the section plane");
        publish(cut.Shape(),result);
    });
}
