#pragma once

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

typedef struct ufbx_texture_ref {
    char *path;
    unsigned char *content;
    size_t content_size;
} ufbx_texture_ref;

typedef struct ufbx_material_info {
    char *name;
    float base_color[4];
    float emissive[3];
    float metallic;
    float roughness;
    bool double_sided;
    ufbx_texture_ref base_color_texture;
    ufbx_texture_ref normal_texture;
    ufbx_texture_ref emissive_texture;
} ufbx_material_info;

typedef struct ufbx_mesh_part_info {
    char *name;
    uint32_t material_index;
    uint32_t vertex_count;
    float *positions;
    float *normals;
    float *uvs;
    float *colors;
    bool has_normals;
    bool has_uvs;
    bool has_colors;
} ufbx_mesh_part_info;

typedef struct ufbx_export_scene {
    ufbx_material_info *materials;
    size_t material_count;
    ufbx_mesh_part_info *parts;
    size_t part_count;
    int32_t right_axis;
    int32_t up_axis;
    void *scene;
} ufbx_export_scene;

ufbx_export_scene *ufbx_export_scene_from_file(const char *path, char **error_msg);
void ufbx_free_export_scene(ufbx_export_scene *scene);
void ufbx_free_string(char *str);
