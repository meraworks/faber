#pragma once

#include <cstddef>
#include <cstdint>

// Keep this signature for existing callers.
extern "C" void faber_mesh_data(
    const void* mesh, const double** vertices, std::size_t* vertex_count,
    const std::uint32_t** triangles, std::size_t* triangle_count, const double** normals) noexcept;

extern "C" const std::uint32_t* faber_mesh_face_indices(const void* mesh) noexcept;
