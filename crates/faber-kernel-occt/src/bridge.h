#pragma once

#include <cstddef>
#include <cstdint>

extern "C" int faber_wrap_cylinder(const void* flat, const double* start, const double* axis,
    double radius, double length, int internal, const double* at, void** output,
    char* error, std::size_t capacity) noexcept;

struct FaberCurve2 {
    int kind, hidden;
    std::size_t offset, count;
    double data[5];
};
struct FaberDrawingResult {
    void* owner;
    const FaberCurve2* curves;
    std::size_t curve_count;
    const double* points;
    std::size_t point_count;
};
extern "C" int faber_project_edges(const void* const* inputs, std::size_t count, const double* origin,
    const double* x, const double* y, int hidden, FaberDrawingResult* result,char* error,std::size_t capacity) noexcept;
extern "C" void faber_free_drawing(void* owner) noexcept;
extern "C" int faber_section_face(const void* shape,const double* origin,const double* normal,
    void** result,char* error,std::size_t capacity) noexcept;

extern "C" int faber_solid_from_mesh(const double* vertices, std::size_t vertex_count,
    const std::uint32_t* triangles, std::size_t triangle_count, int merge_planar,
    void** result, char* error, std::size_t capacity) noexcept;

// Keep this signature for existing callers.
extern "C" void faber_mesh_data(
    const void* mesh, const double** vertices, std::size_t* vertex_count,
    const std::uint32_t** triangles, std::size_t* triangle_count, const double** normals) noexcept;

extern "C" const std::uint32_t* faber_mesh_face_indices(const void* mesh) noexcept;
extern "C" int faber_curved_mesh(const void* shape, double tolerance, void** result,
    char* error, std::size_t capacity) noexcept;
extern "C" void faber_mesh_curvatures(const void* mesh, const double** curvature, std::size_t* nodes,
    const std::uint8_t** kinds, std::size_t* faces) noexcept;

struct FaberEdgeLineData {
    void* owner;
    const double* points;
    std::size_t point_count;
    const std::uint32_t* ends;
    std::size_t edge_count;
};

extern "C" int faber_edge_lines(const void* shape, double tolerance, FaberEdgeLineData* result,
    char* error, std::size_t capacity) noexcept;
extern "C" void faber_free_edge_lines(void* lines) noexcept;
extern "C" int faber_profile_wires(const void* shape, std::size_t size, std::size_t* count,
    void** result, char* error, std::size_t capacity) noexcept;
extern "C" int faber_surface_area(const void* shape, double* area, char* error,
    std::size_t capacity) noexcept;
extern "C" int faber_free_edges(const void* shape, std::uint32_t* ids, std::size_t size,
    std::size_t* count, char* error, std::size_t capacity) noexcept;

extern "C" int faber_linear_form(const void* body, const void* profile, const double* direction,
    double thickness, double offset, void** result, char* error, std::size_t capacity) noexcept;
extern "C" int faber_inset_face(const void* face, std::uint32_t loop, double distance,
    void** result, char* error, std::size_t capacity) noexcept;
extern "C" int faber_offset_sketch_face(const void* face, double distance,
    void** result, char* error, std::size_t capacity) noexcept;

extern "C" int faber_trim_surface(const void* surface, const void* tool, int mode, const double* origin,
    const double* normal, const double* point, void** result, char* error, std::size_t capacity) noexcept;
extern "C" int faber_extend_surface(const void* surface, std::uint32_t edge, double distance,
    void** result, char* error, std::size_t capacity) noexcept;
extern "C" int faber_split_surface(const void* solid, const void* tool, int side,
    void** result, char* error, std::size_t capacity) noexcept;
extern "C" int faber_surface_piece(const void* shape, std::uint32_t index,
    void** result, char* error, std::size_t capacity) noexcept;
extern "C" int faber_replace_surface(const void* solid, std::uint32_t face, const void* surface,
    void** result, char* error, std::size_t capacity) noexcept;
extern "C" int faber_remove_faces(const void* shape, const std::uint32_t* faces, std::size_t count,
    void** result, char* error, std::size_t capacity) noexcept;
extern "C" int faber_move_faces(const void* shape, const std::uint32_t* faces, std::size_t count,
    double distance, void** result, char* error, std::size_t capacity) noexcept;
extern "C" int faber_split_face(const void* shape, std::uint32_t face, const void* tool,
    void** result, char* error, std::size_t capacity) noexcept;
extern "C" int faber_join_edge(const void* shape, std::uint32_t index,
    void** result, char* error, std::size_t capacity) noexcept;
extern "C" int faber_patch_boundary(const void* const* edges, std::size_t edge_count,
    const void* const* support, std::size_t support_count, void** result,
    char* error, std::size_t capacity) noexcept;
extern "C" int faber_stitch_faces(const void* const* surfaces, std::size_t count,
    double tolerance, int solid, std::uint32_t* open_edges, double* largest_gap,
    void** result, char* error, std::size_t capacity) noexcept;
