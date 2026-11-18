#pragma once

#include <cstddef>
#include <cstdint>

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

extern "C" int faber_linear_form(const void* body, const void* profile, const double* direction,
    double thickness, double offset, void** result, char* error, std::size_t capacity) noexcept;
extern "C" int faber_inset_face(const void* face, std::uint32_t loop, double distance,
    void** result, char* error, std::size_t capacity) noexcept;
