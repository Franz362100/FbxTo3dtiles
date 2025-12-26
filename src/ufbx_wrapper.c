#include "ufbx_wrapper.h"

#include "ufbx.h"

#include <math.h>
#include <stdlib.h>
#include <string.h>

static char *copy_string_len(const char *data, size_t len)
{
    if (!data || len == 0) {
        return NULL;
    }
    char *out = (char *)malloc(len + 1);
    if (!out) {
        return NULL;
    }
    memcpy(out, data, len);
    out[len] = '\0';
    return out;
}

static char *copy_ufbx_string(ufbx_string str)
{
    return copy_string_len(str.data, str.length);
}

static void free_texture_ref(ufbx_texture_ref *tex)
{
    if (!tex) {
        return;
    }
    free(tex->path);
    free(tex->content);
    tex->path = NULL;
    tex->content = NULL;
    tex->content_size = 0;
}

static ufbx_texture *resolve_texture(ufbx_texture *tex)
{
    if (!tex) {
        return NULL;
    }
    if (tex->type == UFBX_TEXTURE_LAYERED && tex->layers.count > 0) {
        ufbx_texture_layer *layer = &tex->layers.data[tex->layers.count - 1];
        return resolve_texture(layer->texture);
    }
    if (tex->type == UFBX_TEXTURE_SHADER && tex->shader && tex->shader->main_texture) {
        return resolve_texture(tex->shader->main_texture);
    }
    if (tex->file_textures.count > 0) {
        return tex->file_textures.data[0];
    }
    return tex;
}

static void fill_texture_ref(ufbx_texture *tex, ufbx_texture_ref *out)
{
    memset(out, 0, sizeof(*out));
    if (!tex) {
        return;
    }

    tex = resolve_texture(tex);
    if (!tex) {
        return;
    }

    if (tex->content.data && tex->content.size > 0) {
        out->content = (unsigned char *)malloc(tex->content.size);
        if (out->content) {
            memcpy(out->content, tex->content.data, tex->content.size);
            out->content_size = tex->content.size;
        }
    }

    if (tex->filename.length > 0) {
        out->path = copy_ufbx_string(tex->filename);
    } else if (tex->relative_filename.length > 0) {
        out->path = copy_ufbx_string(tex->relative_filename);
    } else if (tex->absolute_filename.length > 0) {
        out->path = copy_ufbx_string(tex->absolute_filename);
    }
}

static float clamp01(float v)
{
    if (v < 0.0f) {
        return 0.0f;
    }
    if (v > 1.0f) {
        return 1.0f;
    }
    return v;
}

static float get_real(const ufbx_material_map *map, float def)
{
    if (map && map->has_value) {
        return (float)map->value_real;
    }
    return def;
}

static ufbx_vec3 get_vec3(const ufbx_material_map *map, ufbx_vec3 def)
{
    if (map && map->has_value && map->value_components >= 3) {
        return map->value_vec3;
    }
    return def;
}

static void fill_material_info(const ufbx_material *mat, ufbx_material_info *out)
{
    memset(out, 0, sizeof(*out));
    if (!mat) {
        out->base_color[0] = 1.0f;
        out->base_color[1] = 1.0f;
        out->base_color[2] = 1.0f;
        out->base_color[3] = 1.0f;
        out->roughness = 1.0f;
        return;
    }

    out->name = copy_ufbx_string(mat->name);

    bool use_pbr = mat->features.pbr.enabled || mat->pbr.base_color.has_value || mat->pbr.base_factor.has_value ||
                   (mat->pbr.base_color.texture != NULL);

    ufbx_vec3 base_color = {1.0, 1.0, 1.0};
    float base_factor = 1.0f;

    if (use_pbr) {
        base_color = get_vec3(&mat->pbr.base_color, base_color);
        base_factor = get_real(&mat->pbr.base_factor, base_factor);
    } else {
        base_color = get_vec3(&mat->fbx.diffuse_color, base_color);
        base_factor = get_real(&mat->fbx.diffuse_factor, base_factor);
    }

    float alpha = 1.0f;
    if (mat->fbx.transparency_factor.has_value) {
        alpha = clamp01(1.0f - (float)mat->fbx.transparency_factor.value_real);
    }

    out->base_color[0] = (float)base_color.x * base_factor;
    out->base_color[1] = (float)base_color.y * base_factor;
    out->base_color[2] = (float)base_color.z * base_factor;
    out->base_color[3] = alpha;

    float metallic = 0.0f;
    float roughness = 1.0f;
    if (mat->pbr.metalness.has_value) {
        metallic = get_real(&mat->pbr.metalness, metallic);
    }
    if (mat->pbr.roughness.has_value) {
        roughness = get_real(&mat->pbr.roughness, roughness);
    } else if (mat->pbr.glossiness.has_value) {
        roughness = 1.0f - get_real(&mat->pbr.glossiness, 0.0f);
    } else if (mat->fbx.specular_exponent.has_value) {
        float shininess = get_real(&mat->fbx.specular_exponent, 0.0f);
        roughness = sqrtf(2.0f / (shininess + 2.0f));
    }

    out->metallic = clamp01(metallic);
    out->roughness = clamp01(roughness);

    ufbx_vec3 emissive = {0.0, 0.0, 0.0};
    float emissive_factor = 1.0f;
    if (mat->pbr.emission_color.has_value || mat->pbr.emission_factor.has_value) {
        emissive = get_vec3(&mat->pbr.emission_color, emissive);
        emissive_factor = get_real(&mat->pbr.emission_factor, emissive_factor);
    } else if (mat->fbx.emission_color.has_value || mat->fbx.emission_factor.has_value) {
        emissive = get_vec3(&mat->fbx.emission_color, emissive);
        emissive_factor = get_real(&mat->fbx.emission_factor, emissive_factor);
    }

    out->emissive[0] = (float)emissive.x * emissive_factor;
    out->emissive[1] = (float)emissive.y * emissive_factor;
    out->emissive[2] = (float)emissive.z * emissive_factor;

    out->double_sided = mat->features.double_sided.enabled ? true : false;

    ufbx_texture *base_tex = NULL;
    if (mat->pbr.base_color.texture) {
        base_tex = mat->pbr.base_color.texture;
    } else if (mat->fbx.diffuse_color.texture) {
        base_tex = mat->fbx.diffuse_color.texture;
    }
    fill_texture_ref(base_tex, &out->base_color_texture);

    ufbx_texture *normal_tex = NULL;
    if (mat->pbr.normal_map.texture) {
        normal_tex = mat->pbr.normal_map.texture;
    } else if (mat->fbx.normal_map.texture) {
        normal_tex = mat->fbx.normal_map.texture;
    } else if (mat->fbx.bump.texture) {
        normal_tex = mat->fbx.bump.texture;
    }
    fill_texture_ref(normal_tex, &out->normal_texture);

    ufbx_texture *emissive_tex = NULL;
    if (mat->pbr.emission_color.texture) {
        emissive_tex = mat->pbr.emission_color.texture;
    } else if (mat->fbx.emission_color.texture) {
        emissive_tex = mat->fbx.emission_color.texture;
    }
    fill_texture_ref(emissive_tex, &out->emissive_texture);
}

static ufbx_vec3 normalize_vec3(ufbx_vec3 v)
{
    double len = sqrt(v.x * v.x + v.y * v.y + v.z * v.z);
    if (len <= 0.0) {
        return v;
    }
    v.x /= len;
    v.y /= len;
    v.z /= len;
    return v;
}

static ufbx_texture *pick_uv_texture(const ufbx_material *mat)
{
    if (!mat) {
        return NULL;
    }
    if (mat->pbr.base_color.texture) {
        return mat->pbr.base_color.texture;
    }
    if (mat->fbx.diffuse_color.texture) {
        return mat->fbx.diffuse_color.texture;
    }
    if (mat->pbr.emission_color.texture) {
        return mat->pbr.emission_color.texture;
    }
    if (mat->fbx.emission_color.texture) {
        return mat->fbx.emission_color.texture;
    }
    return NULL;
}

static const ufbx_uv_set *find_uv_set(const ufbx_mesh *mesh, ufbx_string name)
{
    if (!mesh || name.length == 0) {
        return NULL;
    }
    for (size_t i = 0; i < mesh->uv_sets.count; i++) {
        const ufbx_uv_set *set = &mesh->uv_sets.data[i];
        if (set->name.length == name.length &&
            memcmp(set->name.data, name.data, name.length) == 0) {
            return set;
        }
    }
    return NULL;
}

static size_t count_material_parts(const ufbx_mesh *mesh)
{
    if (mesh->material_parts.count > 0) {
        return mesh->material_parts.count;
    }
    return 1;
}

static size_t count_total_parts(const ufbx_scene *scene)
{
    size_t total = 0;
    for (size_t i = 0; i < scene->nodes.count; i++) {
        const ufbx_node *node = scene->nodes.data[i];
        if (node->mesh) {
            total += count_material_parts(node->mesh);
        }
    }
    return total;
}

static void fill_part_from_faces(const ufbx_node *node, const ufbx_mesh *mesh, const ufbx_material *material,
                                 const uint32_t *face_indices, size_t face_count, ufbx_mesh_part_info *part)
{
    part->has_normals = mesh->vertex_normal.exists ? true : false;
    part->has_colors = mesh->vertex_color.exists ? true : false;

    const ufbx_vertex_vec2 *uv_attrib = &mesh->vertex_uv;
    ufbx_matrix uv_to_texture = {0};
    bool apply_uv_transform = false;
    ufbx_texture *uv_tex = resolve_texture(pick_uv_texture(material));
    if (uv_tex) {
        const ufbx_uv_set *set = find_uv_set(mesh, uv_tex->uv_set);
        if (set) {
            uv_attrib = &set->vertex_uv;
        }
        if (uv_tex->has_uv_transform) {
            uv_to_texture = uv_tex->uv_to_texture;
            apply_uv_transform = true;
        }
    }
    part->has_uvs = uv_attrib->exists ? true : false;

    size_t tri_count = 0;
    for (size_t i = 0; i < face_count; i++) {
        ufbx_face face = mesh->faces.data[face_indices[i]];
        if (face.num_indices >= 3) {
            tri_count += (size_t)face.num_indices - 2;
        }
    }

    part->vertex_count = (uint32_t)(tri_count * 3);
    if (part->vertex_count == 0) {
        return;
    }

        part->positions = (float *)malloc(sizeof(float) * part->vertex_count * 3);
    part->normals = (float *)malloc(sizeof(float) * part->vertex_count * 3);
    part->uvs = (float *)malloc(sizeof(float) * part->vertex_count * 2);
    part->colors = (float *)malloc(sizeof(float) * part->vertex_count * 4);

    if (!part->positions || !part->normals || !part->uvs || !part->colors) {
        free(part->positions);
        free(part->normals);
        free(part->uvs);
        free(part->colors);
        part->positions = NULL;
        part->normals = NULL;
        part->uvs = NULL;
        part->colors = NULL;
        part->vertex_count = 0;
        return;
    }
    ufbx_matrix normal_m = ufbx_matrix_for_normals(&node->geometry_to_world);
    double det = ufbx_matrix_determinant(&node->geometry_to_world);
    bool flip_winding = det < 0.0;

    size_t max_tri_indices = mesh->max_face_triangles * 3;
    uint32_t *tri_indices = NULL;
    if (max_tri_indices > 0) {
        tri_indices = (uint32_t *)malloc(sizeof(uint32_t) * max_tri_indices);
    }
    if (!tri_indices) {
        free(part->positions);
        free(part->normals);
        free(part->uvs);
        free(part->colors);
        part->positions = NULL;
        part->normals = NULL;
        part->uvs = NULL;
        part->colors = NULL;
        part->vertex_count = 0;
        return;
    }

    size_t out_index = 0;

    for (size_t i = 0; i < face_count; i++) {
        ufbx_face face = mesh->faces.data[face_indices[i]];
        if (face.num_indices < 3) {
            continue;
        }

        uint32_t tri_count_face = ufbx_triangulate_face(tri_indices, max_tri_indices, mesh, face);
        ufbx_vec3 face_normal = {0.0, 1.0, 0.0};
        if (!mesh->vertex_normal.exists) {
            face_normal = ufbx_get_weighted_face_normal(&mesh->vertex_position, face);
            face_normal = normalize_vec3(face_normal);
            face_normal = ufbx_transform_direction(&normal_m, face_normal);
            face_normal = normalize_vec3(face_normal);
        }

        for (uint32_t tri = 0; tri < tri_count_face; tri++) {
            uint32_t ix0 = tri_indices[tri * 3 + 0];
            uint32_t ix1 = tri_indices[tri * 3 + 1];
            uint32_t ix2 = tri_indices[tri * 3 + 2];
            if (flip_winding) {
                uint32_t tmp = ix1;
                ix1 = ix2;
                ix2 = tmp;
            }
            uint32_t tri_ix[3] = { ix0, ix1, ix2 };

            for (uint32_t v = 0; v < 3; v++) {
                uint32_t ix = tri_ix[v];
                uint32_t pos_ix = mesh->vertex_position.indices.data[ix];
                ufbx_vec3 pos = mesh->vertex_position.values.data[pos_ix];
                pos = ufbx_transform_position(&node->geometry_to_world, pos);

                part->positions[out_index * 3 + 0] = (float)pos.x;
                part->positions[out_index * 3 + 1] = (float)pos.y;
                part->positions[out_index * 3 + 2] = (float)pos.z;

                ufbx_vec3 normal = face_normal;
                if (mesh->vertex_normal.exists) {
                    uint32_t n_ix = mesh->vertex_normal.indices.data[ix];
                    normal = mesh->vertex_normal.values.data[n_ix];
                }
                normal = ufbx_transform_direction(&normal_m, normal);
                normal = normalize_vec3(normal);

                part->normals[out_index * 3 + 0] = (float)normal.x;
                part->normals[out_index * 3 + 1] = (float)normal.y;
                part->normals[out_index * 3 + 2] = (float)normal.z;

                ufbx_vec2 uv = {0.0, 0.0};
                if (uv_attrib->exists) {
                    uint32_t uv_ix = uv_attrib->indices.data[ix];
                    uv = uv_attrib->values.data[uv_ix];
                }
                if (apply_uv_transform) {
                    ufbx_vec3 uv3 = { uv.x, uv.y, 0.0 };
                    uv3 = ufbx_transform_position(&uv_to_texture, uv3);
                    uv.x = uv3.x;
                    uv.y = uv3.y;
                }
                uv.y = 1.0f - uv.y;
                part->uvs[out_index * 2 + 0] = (float)uv.x;
                part->uvs[out_index * 2 + 1] = (float)uv.y;

                ufbx_vec4 color = {1.0, 1.0, 1.0, 1.0};
                if (mesh->vertex_color.exists) {
                    uint32_t c_ix = mesh->vertex_color.indices.data[ix];
                    color = mesh->vertex_color.values.data[c_ix];
                }
                part->colors[out_index * 4 + 0] = (float)color.x;
                part->colors[out_index * 4 + 1] = (float)color.y;
                part->colors[out_index * 4 + 2] = (float)color.z;
                part->colors[out_index * 4 + 3] = (float)color.w;

                out_index++;
            }
        }
    }

    free(tri_indices);
}

static uint32_t find_material_index(const ufbx_material *mat, ufbx_material **materials, size_t material_count)
{
    if (!mat || material_count == 0) {
        return 0;
    }
    for (size_t i = 0; i < material_count; i++) {
        if (materials[i] == mat) {
            return (uint32_t)i;
        }
    }
    return 0;
}

ufbx_export_scene *ufbx_export_scene_from_file(const char *path, char **error_msg)
{
    if (error_msg) {
        *error_msg = NULL;
    }

    ufbx_load_opts opts = {0};
    opts.generate_missing_normals = true;
    opts.normalize_normals = true;
    opts.normalize_tangents = true;
    opts.retain_vertex_attrib_w = true;
    opts.target_axes.right = UFBX_COORDINATE_AXIS_POSITIVE_X;
    opts.target_axes.up = UFBX_COORDINATE_AXIS_POSITIVE_Y;
    opts.target_axes.front = UFBX_COORDINATE_AXIS_POSITIVE_Z;
    opts.target_unit_meters = 1.0;

    ufbx_error error;
    memset(&error, 0, sizeof(error));

    ufbx_scene *scene = ufbx_load_file(path, &opts, &error);
    if (!scene) {
        if (error_msg) {
            char buffer[1024];
            ufbx_format_error(buffer, sizeof(buffer), &error);
            *error_msg = copy_string_len(buffer, strlen(buffer));
        }
        return NULL;
    }

    size_t material_count = scene->materials.count;
    bool has_materials = material_count > 0;
    if (!has_materials) {
        material_count = 1;
    }

    ufbx_material **material_ptrs = NULL;
    if (has_materials) {
        material_ptrs = (ufbx_material **)malloc(sizeof(ufbx_material *) * material_count);
        for (size_t i = 0; i < material_count; i++) {
            material_ptrs[i] = scene->materials.data[i];
        }
    }

    ufbx_export_scene *export_scene = (ufbx_export_scene *)calloc(1, sizeof(ufbx_export_scene));
    export_scene->scene = scene;
    export_scene->right_axis = (int32_t)UFBX_COORDINATE_AXIS_POSITIVE_X;
    export_scene->up_axis = (int32_t)UFBX_COORDINATE_AXIS_POSITIVE_Y;
    export_scene->materials = (ufbx_material_info *)calloc(material_count, sizeof(ufbx_material_info));
    export_scene->material_count = material_count;

    if (has_materials) {
        for (size_t i = 0; i < material_count; i++) {
            fill_material_info(scene->materials.data[i], &export_scene->materials[i]);
        }
    } else {
        export_scene->materials[0].base_color[0] = 1.0f;
        export_scene->materials[0].base_color[1] = 1.0f;
        export_scene->materials[0].base_color[2] = 1.0f;
        export_scene->materials[0].base_color[3] = 1.0f;
        export_scene->materials[0].roughness = 1.0f;
    }

    size_t part_count = count_total_parts(scene);
    export_scene->parts = (ufbx_mesh_part_info *)calloc(part_count, sizeof(ufbx_mesh_part_info));
    export_scene->part_count = part_count;

    size_t part_index = 0;
    for (size_t i = 0; i < scene->nodes.count; i++) {
        const ufbx_node *node = scene->nodes.data[i];
        const ufbx_mesh *mesh = node->mesh;
        if (!mesh) {
            continue;
        }

        size_t mesh_part_count = count_material_parts(mesh);
        if (mesh->material_parts.count > 0) {
            for (size_t p = 0; p < mesh_part_count; p++) {
                const ufbx_mesh_part *mesh_part = &mesh->material_parts.data[p];
                ufbx_mesh_part_info *part = &export_scene->parts[part_index++];
                part->name = copy_ufbx_string(node->name);

                uint32_t mat_index = mesh_part->index;
                ufbx_material *mat = NULL;
                if (node->materials.count > mat_index) {
                    mat = node->materials.data[mat_index];
                } else if (mesh->materials.count > mat_index) {
                    mat = mesh->materials.data[mat_index];
                }
                part->material_index = find_material_index(mat, material_ptrs, export_scene->material_count);

                fill_part_from_faces(
                    node,
                    mesh,
                    mat,
                    mesh_part->face_indices.data,
                    mesh_part->face_indices.count,
                    part);
            }
        } else {
            ufbx_mesh_part_info *part = &export_scene->parts[part_index++];
            part->name = copy_ufbx_string(node->name);
            part->material_index = 0;

            if (mesh->faces.count > 0) {
                uint32_t *face_indices = (uint32_t *)malloc(sizeof(uint32_t) * mesh->faces.count);
                for (size_t f = 0; f < mesh->faces.count; f++) {
                    face_indices[f] = (uint32_t)f;
                }
                fill_part_from_faces(node, mesh, NULL, face_indices, mesh->faces.count, part);
                free(face_indices);
            }
        }
    }

    free(material_ptrs);

    return export_scene;
}

void ufbx_free_export_scene(ufbx_export_scene *scene)
{
    if (!scene) {
        return;
    }

    if (scene->materials) {
        for (size_t i = 0; i < scene->material_count; i++) {
            ufbx_material_info *mat = &scene->materials[i];
            free(mat->name);
            free_texture_ref(&mat->base_color_texture);
            free_texture_ref(&mat->normal_texture);
            free_texture_ref(&mat->emissive_texture);
        }
        free(scene->materials);
    }

    if (scene->parts) {
        for (size_t i = 0; i < scene->part_count; i++) {
            ufbx_mesh_part_info *part = &scene->parts[i];
            free(part->name);
            free(part->positions);
            free(part->normals);
            free(part->uvs);
            free(part->colors);
        }
        free(scene->parts);
    }

    if (scene->scene) {
        ufbx_free_scene((ufbx_scene *)scene->scene);
    }

    free(scene);
}

void ufbx_free_string(char *str)
{
    free(str);
}
