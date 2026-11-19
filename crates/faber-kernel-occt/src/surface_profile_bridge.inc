// Read the profile wires in face order. Keep the outside wire first.
static std::vector<TopoDS_Wire> surface_profile_wires(const TopoDS_Shape& shape) {
    if (shape.ShapeType() == TopAbs_WIRE) return {TopoDS::Wire(shape)};
    if (shape.ShapeType() == TopAbs_EDGE)
        return {BRepBuilderAPI_MakeWire(TopoDS::Edge(shape)).Wire()};
    if (TopExp_Explorer(shape, TopAbs_SOLID).More())
        throw std::runtime_error("The surface profile must be a sketch or a curve.");
    std::vector<TopoDS_Wire> wires;
    for (TopExp_Explorer f(shape, TopAbs_FACE); f.More(); f.Next()) {
        const TopoDS_Face face = TopoDS::Face(f.Current());
        const TopoDS_Wire outside = BRepTools::OuterWire(face);
        if (outside.IsNull()) throw std::runtime_error("The sketch has no outer wire.");
        wires.push_back(outside);
        for (TopExp_Explorer w(face, TopAbs_WIRE); w.More(); w.Next())
            if (!w.Current().IsSame(outside)) wires.push_back(TopoDS::Wire(w.Current()));
    }
    if (wires.empty()) throw std::runtime_error("The surface profile has no wire.");
    return wires;
}
extern "C" int faber_profile_wires(const void* src, size_t size, size_t* count, void** result,
                                   char* error, size_t capacity) noexcept {
    return protect(error, capacity, [&] {
        const auto wires = surface_profile_wires(static_cast<const FaberShape*>(src)->value);
        *count = wires.size();
        if (!result) return;
        if (size < wires.size()) throw std::runtime_error("The profile wire list is too small.");
        std::vector<std::unique_ptr<FaberShape>> owned;
        owned.reserve(wires.size());
        for (const auto& wire : wires) {
            void* raw = nullptr;
            publish(wire, &raw);
            owned.emplace_back(static_cast<FaberShape*>(raw));
        }
        for (size_t i = 0; i < owned.size(); ++i) result[i] = owned[i].release();
    });
}
extern "C" int faber_surface_area(const void* src, double* area, char* error, size_t capacity) noexcept {
    return protect(error,capacity,[&] {
        const auto& shape = static_cast<const FaberShape*>(src)->value;
        GProp_GProps props;
        if (BRepGProp::SurfaceProperties(shape,props,1e-9) < 0.)
            throw std::runtime_error("Cannot integrate surface area.");
        *area=props.Mass();
    });
}
extern "C" int faber_free_edges(const void* src, uint32_t* ids, size_t size, size_t* count,
                                 char* error, size_t capacity) noexcept {
    return protect(error,capacity,[&] {
        const auto& shape=static_cast<const FaberShape*>(src)->value;
        TopTools_IndexedMapOfShape edges;
        TopExp::MapShapes(shape,TopAbs_EDGE,edges);
        TopTools_IndexedDataMapOfShapeListOfShape adjacent;
        TopExp::MapShapesAndUniqueAncestors(shape,TopAbs_EDGE,TopAbs_FACE,adjacent);
        *count=0;
        for (int i=1;i<=edges.Extent();++i) {
            const TopoDS_Edge edge=TopoDS::Edge(edges(i));
            if (BRep_Tool::Degenerated(edge) || !adjacent.Contains(edge)) continue;
            const auto& faces=adjacent.FindFromKey(edge);
            if(faces.Extent()!=1 || BRep_Tool::IsClosed(edge,TopoDS::Face(faces.First()))) continue;
            if (*count>=size) throw std::runtime_error("The edge list is too small.");
            ids[(*count)++]=static_cast<uint32_t>(i);
        }
    });
}
