use crate::geo::GeoContext;
use crate::gltf_writer::{write_glb_with_textures, TextureCache, TextureMode};
use crate::ufbx_loader::{AxisDir, Material, MeshPart, SceneData};
use anyhow::{bail, Context, Result};
use serde_json::json;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

pub struct TilesetOptions {
    pub origin_lat: f64,
    pub origin_lon: f64,
    pub origin_height: f64,
    pub heading: f64,
    pub scale: f64,
    pub tile_size: f64,
    pub min_tile_size: f64,
    pub max_level: Option<u32>,
    pub embed_textures: bool,
}

#[derive(Clone)]
struct PartBuilder {
    name: Option<String>,
    material_index: usize,
    positions: Vec<f32>,
    normals: Vec<f32>,
    uvs: Vec<f32>,
    colors: Vec<f32>,
}

struct TileBucket {
    parts: HashMap<usize, PartBuilder>,
    min_local: [f64; 3],
    max_local: [f64; 3],
}

#[derive(Clone)]
struct TileNode {
    level: u32,
    x: i32,
    z: i32,
    min_local: [f64; 3],
    max_local: [f64; 3],
    has_content: bool,
    children: Vec<TileNode>,
}

#[derive(Clone)]
struct Vertex {
    pos_local: [f64; 3],
    pos_enu: [f64; 3],
    normal: [f32; 3],
    uv: [f32; 2],
    color: [f32; 4],
}

// ufbx 已统一输出为 Y-up，这里无需额外轴变换。

pub fn export_tileset(scene: &SceneData, output_dir: &Path, options: &TilesetOptions) -> Result<()> {
    if scene.parts.is_empty() {
        bail!("no mesh data found in FBX");
    }
    if options.tile_size <= 0.0 {
        bail!("tile_size must be positive");
    }
    if options.min_tile_size <= 0.0 {
        bail!("min_tile_size must be positive");
    }

    let geo = GeoContext::new(
        options.origin_lat,
        options.origin_lon,
        options.origin_height,
        options.heading,
        options.scale,
    );

    let max_level = options
        .max_level
        .unwrap_or_else(|| compute_max_level(options.tile_size, options.min_tile_size));
    let leaf_size = options.tile_size / 2_f64.powi(max_level as i32);

    let mut buckets: HashMap<(i32, i32), TileBucket> = HashMap::new();
    let mut global_min_local = [f64::INFINITY; 3];
    let mut global_max_local = [f64::NEG_INFINITY; 3];

    for part in &scene.parts {
        let positions = &part.positions;
        if positions.len() < 9 {
            continue;
        }
        let has_normals = part.normals.len() == positions.len();
        let has_uvs = part.uvs.len() * 3 == positions.len() * 2;
        let has_colors = part.colors.len() * 3 == positions.len() * 4;

        for tri in (0..positions.len()).step_by(9) {
            let p0 = [
                positions[tri] as f64,
                positions[tri + 1] as f64,
                positions[tri + 2] as f64,
            ];
            let p1 = [
                positions[tri + 3] as f64,
                positions[tri + 4] as f64,
                positions[tri + 5] as f64,
            ];
            let p2 = [
                positions[tri + 6] as f64,
                positions[tri + 7] as f64,
                positions[tri + 8] as f64,
            ];

            let w0 = geo.transform_local(p0);
            let w1 = geo.transform_local(p1);
            let w2 = geo.transform_local(p2);

            let n0 = if has_normals {
                [
                    part.normals[tri],
                    part.normals[tri + 1],
                    part.normals[tri + 2],
                ]
            } else {
                [0.0; 3]
            };
            let n1 = if has_normals {
                [
                    part.normals[tri + 3],
                    part.normals[tri + 4],
                    part.normals[tri + 5],
                ]
            } else {
                [0.0; 3]
            };
            let n2 = if has_normals {
                [
                    part.normals[tri + 6],
                    part.normals[tri + 7],
                    part.normals[tri + 8],
                ]
            } else {
                [0.0; 3]
            };

            let (uv0, uv1, uv2) = if has_uvs {
                let uv_start = (tri / 3) * 2;
                (
                    [part.uvs[uv_start], part.uvs[uv_start + 1]],
                    [part.uvs[uv_start + 2], part.uvs[uv_start + 3]],
                    [part.uvs[uv_start + 4], part.uvs[uv_start + 5]],
                )
            } else {
                ([0.0; 2], [0.0; 2], [0.0; 2])
            };

            let (c0, c1, c2) = if has_colors {
                let color_start = (tri / 3) * 4;
                (
                    [
                        part.colors[color_start],
                        part.colors[color_start + 1],
                        part.colors[color_start + 2],
                        part.colors[color_start + 3],
                    ],
                    [
                        part.colors[color_start + 4],
                        part.colors[color_start + 5],
                        part.colors[color_start + 6],
                        part.colors[color_start + 7],
                    ],
                    [
                        part.colors[color_start + 8],
                        part.colors[color_start + 9],
                        part.colors[color_start + 10],
                        part.colors[color_start + 11],
                    ],
                )
            } else {
                ([0.0; 4], [0.0; 4], [0.0; 4])
            };

            let v0 = Vertex {
                pos_local: p0,
                pos_enu: w0,
                normal: n0,
                uv: uv0,
                color: c0,
            };
            let v1 = Vertex {
                pos_local: p1,
                pos_enu: w1,
                normal: n1,
                uv: uv1,
                color: c1,
            };
            let v2 = Vertex {
                pos_local: p2,
                pos_enu: w2,
                normal: n2,
                uv: uv2,
                color: c2,
            };
            let tri_vertices = [v0, v1, v2];

            let tri_min_x = w0[0].min(w1[0]).min(w2[0]);
            let tri_max_x = w0[0].max(w1[0]).max(w2[0]);
            let tri_min_z = w0[2].min(w1[2]).min(w2[2]);
            let tri_max_z = w0[2].max(w1[2]).max(w2[2]);

            let tile_x_min = (tri_min_x / leaf_size).floor() as i32;
            let tile_x_max = (tri_max_x / leaf_size).floor() as i32;
            let tile_z_min = (tri_min_z / leaf_size).floor() as i32;
            let tile_z_max = (tri_max_z / leaf_size).floor() as i32;

            for tile_x in tile_x_min..=tile_x_max {
                let x0 = tile_x as f64 * leaf_size;
                let x1 = x0 + leaf_size;
                for tile_z in tile_z_min..=tile_z_max {
                    let z0 = tile_z as f64 * leaf_size;
                    let z1 = z0 + leaf_size;

                    let polygon =
                        clip_triangle_to_tile(&tri_vertices, x0, x1, z0, z1, has_normals);
                    if polygon.len() < 3 {
                        continue;
                    }

                    let bucket = buckets
                        .entry((tile_x, tile_z))
                        .or_insert_with(|| TileBucket {
                            parts: HashMap::new(),
                            min_local: [f64::INFINITY; 3],
                            max_local: [f64::NEG_INFINITY; 3],
                        });

                    let first = &polygon[0];
                    for i in 1..polygon.len() - 1 {
                        let a = first;
                        let b = &polygon[i];
                        let c = &polygon[i + 1];
                        if is_degenerate_triangle(a, b, c) {
                            continue;
                        }

                        let mut tri_min_local = [f64::INFINITY; 3];
                        let mut tri_max_local = [f64::NEG_INFINITY; 3];
                        for vertex in [a, b, c] {
                            let local = vertex.pos_local;
                            tri_min_local[0] = tri_min_local[0].min(local[0]);
                            tri_min_local[1] = tri_min_local[1].min(local[1]);
                            tri_min_local[2] = tri_min_local[2].min(local[2]);
                            tri_max_local[0] = tri_max_local[0].max(local[0]);
                            tri_max_local[1] = tri_max_local[1].max(local[1]);
                            tri_max_local[2] = tri_max_local[2].max(local[2]);
                        }

                        for axis in 0..3 {
                            bucket.min_local[axis] =
                                bucket.min_local[axis].min(tri_min_local[axis]);
                            bucket.max_local[axis] =
                                bucket.max_local[axis].max(tri_max_local[axis]);
                            global_min_local[axis] =
                                global_min_local[axis].min(tri_min_local[axis]);
                            global_max_local[axis] =
                                global_max_local[axis].max(tri_max_local[axis]);
                        }

                        let builder =
                            bucket
                                .parts
                                .entry(part.material_index)
                                .or_insert_with(|| PartBuilder {
                                    name: part.name.clone(),
                                    material_index: part.material_index,
                                    positions: Vec::new(),
                                    normals: Vec::new(),
                                    uvs: Vec::new(),
                                    colors: Vec::new(),
                                });

                        builder.positions.extend_from_slice(&[
                            a.pos_local[0] as f32,
                            a.pos_local[1] as f32,
                            a.pos_local[2] as f32,
                            b.pos_local[0] as f32,
                            b.pos_local[1] as f32,
                            b.pos_local[2] as f32,
                            c.pos_local[0] as f32,
                            c.pos_local[1] as f32,
                            c.pos_local[2] as f32,
                        ]);

                        if has_normals {
                            builder.normals.extend_from_slice(&[
                                a.normal[0],
                                a.normal[1],
                                a.normal[2],
                                b.normal[0],
                                b.normal[1],
                                b.normal[2],
                                c.normal[0],
                                c.normal[1],
                                c.normal[2],
                            ]);
                        }
                        if has_uvs {
                            builder.uvs.extend_from_slice(&[
                                a.uv[0], a.uv[1], b.uv[0], b.uv[1], c.uv[0], c.uv[1],
                            ]);
                        }
                        if has_colors {
                            builder.colors.extend_from_slice(&[
                                a.color[0],
                                a.color[1],
                                a.color[2],
                                a.color[3],
                                b.color[0],
                                b.color[1],
                                b.color[2],
                                b.color[3],
                                c.color[0],
                                c.color[1],
                                c.color[2],
                                c.color[3],
                            ]);
                        }
                    }
                }
            }
        }
    }

    if buckets.is_empty() {
        bail!("no triangles were assigned to tiles");
    }

    let (min_tile_x, max_tile_x, min_tile_z, max_tile_z) = tile_index_bounds(&buckets);

    let tiles_dir = output_dir.join("tiles");
    fs::create_dir_all(&tiles_dir)
        .with_context(|| format!("create tiles dir {}", tiles_dir.display()))?;

    let mut texture_cache = if options.embed_textures {
        None
    } else {
        let textures_dir = output_dir.join("textures");
        fs::create_dir_all(&textures_dir)
            .with_context(|| format!("create textures dir {}", textures_dir.display()))?;
        Some(TextureCache::new(textures_dir, "../textures"))
    };

    for ((x, z), bucket) in &buckets {
        let scene_tile = build_tile_scene(bucket, &scene.materials, scene.right_axis, scene.up_axis);
        let filename = tile_filename(max_level, *x, *z);
        let path = tiles_dir.join(filename);
        if let Some(cache) = texture_cache.as_mut() {
            let mut mode = TextureMode::External(cache);
            write_glb_with_textures(&scene_tile, &path, &mut mode)
                .with_context(|| format!("write tile {}", path.display()))?;
        } else {
            let mut mode = TextureMode::Embed;
            write_glb_with_textures(&scene_tile, &path, &mut mode)
                .with_context(|| format!("write tile {}", path.display()))?;
        }
    }

    let root_transform = geo.transform_matrix();
    let root_error = options.tile_size * 0.5;
    let force_refine_error = root_error * 1_000_000.0;
    let heading_rad = options.heading.to_radians();
    let scale = options.scale;
    // 本地坐标按 Y 为上轴。
    let up_axis = 1usize;
    let root_box = rotate_box_y_up_to_z_up(grid_extent_box(
        min_tile_x,
        max_tile_x,
        min_tile_z,
        max_tile_z,
        leaf_size,
        global_min_local[up_axis],
        global_max_local[up_axis],
        heading_rad,
        scale,
    ));

    let mut root_children: Vec<TileNode> = buckets
        .into_iter()
        .map(|((x, z), bucket)| {
            let mut min_local = bucket.min_local;
            let mut max_local = bucket.max_local;
            min_local[up_axis] = global_min_local[up_axis];
            max_local[up_axis] = global_max_local[up_axis];
            TileNode {
                level: max_level,
                x,
                z,
                min_local,
                max_local,
                has_content: true,
                children: Vec::new(),
            }
        })
        .collect();
    root_children.sort_by_key(|node| (node.z, node.x));

    let tileset = json!({
        "asset": {
            "version": "1.1",
            "generator": "ufbx_rust+flat"
        },
        "geometricError": force_refine_error,
        "root": {
            "transform": root_transform,
            "boundingVolume": { "box": root_box },
            "geometricError": force_refine_error,
            "refine": "REPLACE",
            "children": root_children
                .into_iter()
                .map(|node| {
                    tile_node_to_json(
                        node,
                        options.tile_size,
                        heading_rad,
                        scale,
                        root_error,
                        force_refine_error,
                    )
                })
                .collect::<Vec<_>>()
        }
    });

    let tileset_path = output_dir.join("tileset.json");
    let file = fs::File::create(&tileset_path)
        .with_context(|| format!("write tileset {}", tileset_path.display()))?;
    serde_json::to_writer_pretty(file, &tileset)?;

    Ok(())
}

fn compute_max_level(tile_size: f64, min_tile_size: f64) -> u32 {
    let mut level = 0;
    let mut size = tile_size;
    while size > min_tile_size {
        size *= 0.5;
        level += 1;
    }
    level
}

fn tile_filename(level: u32, x: i32, z: i32) -> String {
    format!("L{level}_X{x}_Z{z}.glb")
}

fn build_tile_scene(
    bucket: &TileBucket,
    materials: &[Material],
    right_axis: AxisDir,
    up_axis: AxisDir,
) -> SceneData {
    let mut used_indices: Vec<usize> = bucket.parts.keys().copied().collect();
    used_indices.sort_unstable();

    let mut remap = HashMap::new();
    let mut tile_materials = Vec::new();

    for (new_index, old_index) in used_indices.iter().enumerate() {
        remap.insert(*old_index, new_index);
        tile_materials.push(materials[*old_index].clone());
    }

    let mut tile_parts = Vec::new();
    for builder in bucket.parts.values() {
        let mapped_index = remap.get(&builder.material_index).copied().unwrap_or(0);
        tile_parts.push(MeshPart {
            name: builder.name.clone(),
            material_index: mapped_index,
            positions: builder.positions.clone(),
            normals: builder.normals.clone(),
            uvs: builder.uvs.clone(),
            colors: builder.colors.clone(),
        });
    }

    SceneData {
        materials: tile_materials,
        parts: tile_parts,
        right_axis,
        up_axis,
    }
}

fn tile_index_bounds(buckets: &HashMap<(i32, i32), TileBucket>) -> (i32, i32, i32, i32) {
    let mut min_x = i32::MAX;
    let mut max_x = i32::MIN;
    let mut min_z = i32::MAX;
    let mut max_z = i32::MIN;
    for (x, z) in buckets.keys() {
        min_x = min_x.min(*x);
        max_x = max_x.max(*x);
        min_z = min_z.min(*z);
        max_z = max_z.max(*z);
    }
    (min_x, max_x, min_z, max_z)
}

fn bounds_to_box(min: [f64; 3], max: [f64; 3]) -> [f64; 12] {
    let center = [
        0.5 * (min[0] + max[0]),
        0.5 * (min[1] + max[1]),
        0.5 * (min[2] + max[2]),
    ];
    let half = [
        0.5 * (max[0] - min[0]),
        0.5 * (max[1] - min[1]),
        0.5 * (max[2] - min[2]),
    ];
    [
        center[0],
        center[1],
        center[2],
        half[0],
        0.0,
        0.0,
        0.0,
        half[1],
        0.0,
        0.0,
        0.0,
        half[2],
    ]
}

// Cesium 3D Tiles 以 Z-up 为默认约定，输出包围盒需从 Y-up 旋转到 Z-up。
fn rotate_box_y_up_to_z_up(box_bounds: [f64; 12]) -> [f64; 12] {
    fn rotate(v: [f64; 3]) -> [f64; 3] {
        [v[0], -v[2], v[1]]
    }

    let center = rotate([box_bounds[0], box_bounds[1], box_bounds[2]]);
    let axis_x = rotate([box_bounds[3], box_bounds[4], box_bounds[5]]);
    let axis_y = rotate([box_bounds[6], box_bounds[7], box_bounds[8]]);
    let axis_z = rotate([box_bounds[9], box_bounds[10], box_bounds[11]]);
    [
        center[0],
        center[1],
        center[2],
        axis_x[0],
        axis_x[1],
        axis_x[2],
        axis_y[0],
        axis_y[1],
        axis_y[2],
        axis_z[0],
        axis_z[1],
        axis_z[2],
    ]
}

fn grid_extent_box(
    min_tile_x: i32,
    max_tile_x: i32,
    min_tile_z: i32,
    max_tile_z: i32,
    leaf_size: f64,
    min_y: f64,
    max_y: f64,
    heading_rad: f64,
    scale: f64,
) -> [f64; 12] {
    let pad_ratio = 0.005;
    let pad_enu = leaf_size * pad_ratio;

    let min_x_enu = (min_tile_x as f64) * leaf_size;
    let max_x_enu = ((max_tile_x + 1) as f64) * leaf_size;
    let min_z_enu = (min_tile_z as f64) * leaf_size;
    let max_z_enu = ((max_tile_z + 1) as f64) * leaf_size;

    let center_enu_x = 0.5 * (min_x_enu + max_x_enu);
    let center_enu_z = 0.5 * (min_z_enu + max_z_enu);
    let half_x = 0.5 * (max_x_enu - min_x_enu) + pad_enu;
    let half_z = 0.5 * (max_z_enu - min_z_enu) + pad_enu;

    let (sin_h, cos_h) = heading_rad.sin_cos();
    let inv_scale = if scale.abs() < 1e-12 { 0.0 } else { 1.0 / scale };
    let inv_scale_abs = inv_scale.abs();

    let center_x = (center_enu_x * cos_h + center_enu_z * sin_h) * inv_scale;
    let center_z = (-center_enu_x * sin_h + center_enu_z * cos_h) * inv_scale;

    let mut min_y = min_y;
    let mut max_y = max_y;
    if max_y < min_y {
        std::mem::swap(&mut min_y, &mut max_y);
    }
    let mut pad_y = (max_y - min_y) * 0.02;
    let pad_local = pad_enu * inv_scale_abs;
    if pad_y < pad_local {
        pad_y = pad_local;
    }
    min_y -= pad_y;
    max_y += pad_y;
    let center_y = 0.5 * (min_y + max_y);
    let half_y = 0.5 * (max_y - min_y);

    let axis_x = [half_x * cos_h * inv_scale, 0.0, -half_x * sin_h * inv_scale];
    let axis_y = [0.0, half_y, 0.0];
    let axis_z = [half_z * sin_h * inv_scale, 0.0, half_z * cos_h * inv_scale];

    [
        center_x,
        center_y,
        center_z,
        axis_x[0],
        axis_x[1],
        axis_x[2],
        axis_y[0],
        axis_y[1],
        axis_y[2],
        axis_z[0],
        axis_z[1],
        axis_z[2],
    ]
}

fn tile_node_to_json(
    node: TileNode,
    tile_size: f64,
    _heading_rad: f64,
    _scale: f64,
    base_error: f64,
    force_refine_error: f64,
) -> serde_json::Value {
    let geometric_error = if node.children.is_empty() {
        0.0
    } else if node.has_content {
        base_error / 2_f64.powi(node.level as i32)
    } else {
        force_refine_error
    };

    // Use actual geometry bounds instead of grid cell bounds
    // Add small padding to avoid zero volume
    let mut min = node.min_local;
    let mut max = node.max_local;
    let pad = tile_size * 0.01;
    for i in 0..3 {
        min[i] -= pad;
        max[i] += pad;
    }

    let box_bounds = rotate_box_y_up_to_z_up(bounds_to_box(min, max));
    let mut json_node = json!({
        "boundingVolume": { "box": box_bounds },
        "geometricError": geometric_error,
        "refine": "REPLACE"
    });

    if node.has_content {
        json_node["content"] = json!({
            "uri": format!("tiles/{}", tile_filename(node.level, node.x, node.z))
        });
    }

    if !node.children.is_empty() {
        json_node["children"] = serde_json::Value::Array(
            node.children
                .into_iter()
                .map(|child| {
                    tile_node_to_json(
                        child,
                        tile_size,
                        _heading_rad,
                        _scale,
                        base_error,
                        force_refine_error,
                    )
                })
                .collect(),
        );
    }

    json_node
}

fn clip_triangle_to_tile(
    vertices: &[Vertex; 3],
    x0: f64,
    x1: f64,
    z0: f64,
    z1: f64,
    normalize_normals: bool,
) -> Vec<Vertex> {
    let mut poly = vec![vertices[0].clone(), vertices[1].clone(), vertices[2].clone()];
    poly = clip_polygon(&poly, 0, x0, true, normalize_normals);
    if poly.is_empty() {
        return poly;
    }
    poly = clip_polygon(&poly, 0, x1, false, normalize_normals);
    if poly.is_empty() {
        return poly;
    }
    poly = clip_polygon(&poly, 2, z0, true, normalize_normals);
    if poly.is_empty() {
        return poly;
    }
    clip_polygon(&poly, 2, z1, false, normalize_normals)
}

fn clip_polygon(
    vertices: &[Vertex],
    axis: usize,
    value: f64,
    keep_greater: bool,
    normalize_normals: bool,
) -> Vec<Vertex> {
    if vertices.is_empty() {
        return Vec::new();
    }
    let mut output = Vec::new();
    let mut prev = vertices.last().unwrap();
    let mut prev_inside = inside_plane(prev, axis, value, keep_greater);
    for curr in vertices {
        let curr_inside = inside_plane(curr, axis, value, keep_greater);
        if curr_inside {
            if !prev_inside {
                output.push(intersect_plane(prev, curr, axis, value, normalize_normals));
            }
            output.push(curr.clone());
        } else if prev_inside {
            output.push(intersect_plane(prev, curr, axis, value, normalize_normals));
        }
        prev = curr;
        prev_inside = curr_inside;
    }
    output
}

fn inside_plane(vertex: &Vertex, axis: usize, value: f64, keep_greater: bool) -> bool {
    let eps = 1e-9;
    if keep_greater {
        vertex.pos_enu[axis] >= value - eps
    } else {
        vertex.pos_enu[axis] <= value + eps
    }
}

fn intersect_plane(
    a: &Vertex,
    b: &Vertex,
    axis: usize,
    value: f64,
    normalize_normals: bool,
) -> Vertex {
    let denom = b.pos_enu[axis] - a.pos_enu[axis];
    let mut t = if denom.abs() < 1e-12 {
        0.0
    } else {
        (value - a.pos_enu[axis]) / denom
    };
    if t < 0.0 {
        t = 0.0;
    } else if t > 1.0 {
        t = 1.0;
    }
    interpolate_vertex(a, b, t, normalize_normals)
}

fn interpolate_vertex(a: &Vertex, b: &Vertex, t: f64, normalize_normals: bool) -> Vertex {
    let tf = t as f32;
    let mut normal = [
        lerp_f32(a.normal[0], b.normal[0], tf),
        lerp_f32(a.normal[1], b.normal[1], tf),
        lerp_f32(a.normal[2], b.normal[2], tf),
    ];
    if normalize_normals {
        normal = normalize3(normal);
    }
    Vertex {
        pos_local: [
            lerp_f64(a.pos_local[0], b.pos_local[0], t),
            lerp_f64(a.pos_local[1], b.pos_local[1], t),
            lerp_f64(a.pos_local[2], b.pos_local[2], t),
        ],
        pos_enu: [
            lerp_f64(a.pos_enu[0], b.pos_enu[0], t),
            lerp_f64(a.pos_enu[1], b.pos_enu[1], t),
            lerp_f64(a.pos_enu[2], b.pos_enu[2], t),
        ],
        normal,
        uv: [
            lerp_f32(a.uv[0], b.uv[0], tf),
            lerp_f32(a.uv[1], b.uv[1], tf),
        ],
        color: [
            lerp_f32(a.color[0], b.color[0], tf),
            lerp_f32(a.color[1], b.color[1], tf),
            lerp_f32(a.color[2], b.color[2], tf),
            lerp_f32(a.color[3], b.color[3], tf),
        ],
    }
}

fn lerp_f64(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

fn lerp_f32(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn normalize3(v: [f32; 3]) -> [f32; 3] {
    let len_sq = v[0] * v[0] + v[1] * v[1] + v[2] * v[2];
    if len_sq <= 0.0 {
        return v;
    }
    let inv_len = 1.0 / len_sq.sqrt();
    [v[0] * inv_len, v[1] * inv_len, v[2] * inv_len]
}

fn is_degenerate_triangle(a: &Vertex, b: &Vertex, c: &Vertex) -> bool {
    let ab = [
        b.pos_enu[0] - a.pos_enu[0],
        b.pos_enu[1] - a.pos_enu[1],
        b.pos_enu[2] - a.pos_enu[2],
    ];
    let ac = [
        c.pos_enu[0] - a.pos_enu[0],
        c.pos_enu[1] - a.pos_enu[1],
        c.pos_enu[2] - a.pos_enu[2],
    ];
    let cross = [
        ab[1] * ac[2] - ab[2] * ac[1],
        ab[2] * ac[0] - ab[0] * ac[2],
        ab[0] * ac[1] - ab[1] * ac[0],
    ];
    let area_sq = cross[0] * cross[0] + cross[1] * cross[1] + cross[2] * cross[2];
    area_sq < 1e-20
}
