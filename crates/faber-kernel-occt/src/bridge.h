#pragma once

#include <cstddef>
#include <cstdint>

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
