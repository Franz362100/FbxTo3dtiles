use crate::image_utils::{encode_texture, ImageData};
use crate::ufbx_loader::{SceneData, TextureSource};
use anyhow::{bail, Context, Result};
use serde_json::{json, Map, Value};
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::fs::{self, File};
use std::hash::{Hash, Hasher};
use std::io::Write;
use std::path::{Path, PathBuf};

const GLTF_MAGIC: u32 = 0x46546C67;
const GLTF_VERSION: u32 = 2;
const CHUNK_TYPE_JSON: u32 = 0x4E4F534A;
const CHUNK_TYPE_BIN: u32 = 0x004E4942;

const TARGET_ARRAY_BUFFER: u32 = 34962;

pub struct TextureCache {
    pub dir: PathBuf,
    pub uri_prefix: String,
    pub map: HashMap<u64, String>,
}

impl TextureCache {
    pub fn new(dir: PathBuf, uri_prefix: impl Into<String>) -> Self {
        Self {
            dir,
            uri_prefix: uri_prefix.into(),
            map: HashMap::new(),
        }
    }
}

pub enum TextureMode<'a> {
    Embed,
    External(&'a mut TextureCache),
}

struct TextureRef {
    texture_index: usize,
    has_alpha: bool,
}

enum ImageEntry {
    Embedded(ImageData),
    External {
        uri: String,
        mime_type: String,
        has_alpha: bool,
    },
}

impl ImageEntry {
    fn has_alpha(&self) -> bool {
        match self {
            ImageEntry::Embedded(image) => image.has_alpha,
            ImageEntry::External { has_alpha, .. } => *has_alpha,
        }
    }
}

pub fn write_glb(scene: &SceneData, path: &Path) -> Result<()> {
    let mut mode = TextureMode::Embed;
    write_glb_with_textures(scene, path, &mut mode)
}

pub fn write_glb_with_textures(
    scene: &SceneData,
    path: &Path,
    texture_mode: &mut TextureMode,
) -> Result<()> {
    let mut buffer = BufferBuilder::default();
    let mut buffer_views = Vec::new();
    let mut accessors = Vec::new();
    let mut primitives = Vec::new();

    for part in &scene.parts {
        if part.positions.is_empty() {
            continue;
        }
        let positions = &part.positions;
        let vertex_count = positions.len() / 3;
        let normals = ensure_normals(positions, &part.normals);
        let uvs = ensure_uvs(vertex_count, &part.uvs);
        let colors = ensure_colors(vertex_count, &part.colors);
        let tangents = compute_tangents(positions, &uvs, &normals);

        let (pos_accessor, min, max) = push_accessor_vec3(
            &mut buffer,
            &mut buffer_views,
            &mut accessors,
            positions,
            TARGET_ARRAY_BUFFER,
        )?;
        update_accessor_bounds(&mut accessors[pos_accessor], min, max);

        let normal_accessor = push_accessor_vec3(
            &mut buffer,
            &mut buffer_views,
            &mut accessors,
            &normals,
            TARGET_ARRAY_BUFFER,
        )?
        .0;
        let uv_accessor = push_accessor_vec2(
            &mut buffer,
            &mut buffer_views,
            &mut accessors,
            &uvs,
            TARGET_ARRAY_BUFFER,
        )?
        .0;
        let color_accessor = push_accessor_vec4(
            &mut buffer,
            &mut buffer_views,
            &mut accessors,
            &colors,
            TARGET_ARRAY_BUFFER,
        )?
        .0;
        let tangent_accessor = push_accessor_vec4(
            &mut buffer,
            &mut buffer_views,
            &mut accessors,
            &tangents,
            TARGET_ARRAY_BUFFER,
        )?
        .0;

        let mut attributes = Map::new();
        attributes.insert("POSITION".to_string(), json!(pos_accessor));
        attributes.insert("NORMAL".to_string(), json!(normal_accessor));
        attributes.insert("TEXCOORD_0".to_string(), json!(uv_accessor));
        attributes.insert("COLOR_0".to_string(), json!(color_accessor));
        attributes.insert("TANGENT".to_string(), json!(tangent_accessor));

        let material_index = if part.material_index < scene.materials.len() {
            part.material_index
        } else {
            0
        };

        primitives.push(json!({
            "attributes": Value::Object(attributes),
            "material": material_index,
            "mode": 4
        }));
    }

    if primitives.is_empty() {
        bail!("no primitives generated");
    }

    let mut images: Vec<ImageEntry> = Vec::new();
    let mut textures = Vec::new();
    let mut samplers = Vec::new();
    let mut image_map = HashMap::<u64, usize>::new();
    let mut texture_map = HashMap::<usize, usize>::new();

    let sampler_index = samplers.len();
    samplers.push(json!({
        "magFilter": 9729,
        "minFilter": 9729,
        "wrapS": 10497,
        "wrapT": 10497
    }));

    let mut materials = Vec::new();
    for material in &scene.materials {
        let base_color_texture = texture_index(
            &material.base_color_texture,
            &mut images,
            &mut textures,
            &mut image_map,
            &mut texture_map,
            sampler_index,
            texture_mode,
        )?;
        let normal_texture = texture_index(
            &material.normal_texture,
            &mut images,
            &mut textures,
            &mut image_map,
            &mut texture_map,
            sampler_index,
            texture_mode,
        )?;
        let emissive_texture = texture_index(
            &material.emissive_texture,
            &mut images,
            &mut textures,
            &mut image_map,
            &mut texture_map,
            sampler_index,
            texture_mode,
        )?;

        let mut pbr = json!({
            "baseColorFactor": material.base_color,
            "metallicFactor": material.metallic,
            "roughnessFactor": material.roughness
        });

        if let Some(tex) = &base_color_texture {
            pbr["baseColorTexture"] = json!({ "index": tex.texture_index });
        }

        let has_texture = base_color_texture.is_some()
            || normal_texture.is_some()
            || emissive_texture.is_some();
        let base_color_has_alpha = base_color_texture
            .as_ref()
            .map(|tex| tex.has_alpha)
            .unwrap_or(false);

        let mut material_value = json!({
            "pbrMetallicRoughness": pbr,
            "doubleSided": material.double_sided
        });

        if has_texture {
            material_value["doubleSided"] = json!(true);
        }
        if base_color_has_alpha || material.base_color[3] < 0.999 {
            material_value["alphaMode"] = json!("BLEND");
        }

        if let Some(tex) = normal_texture {
            material_value["normalTexture"] = json!({ "index": tex.texture_index });
        }
        if let Some(tex) = emissive_texture {
            material_value["emissiveTexture"] = json!({ "index": tex.texture_index });
        }
        if material.emissive != [0.0, 0.0, 0.0] {
            material_value["emissiveFactor"] = json!(material.emissive);
        }
        if let Some(name) = &material.name {
            material_value["name"] = json!(name);
        }

        materials.push(material_value);
    }

    if materials.is_empty() {
        materials.push(json!({
            "pbrMetallicRoughness": {
                "baseColorFactor": [1.0, 1.0, 1.0, 1.0],
                "metallicFactor": 0.0,
                "roughnessFactor": 1.0
            }
        }));
    }

    let mut images_json = Vec::new();
    for image in &images {
        match image {
            ImageEntry::Embedded(data) => {
                let (view_index, _) = buffer.push_bytes(&mut buffer_views, &data.bytes, None)?;
                images_json.push(json!({ "bufferView": view_index, "mimeType": data.mime_type }));
            }
            ImageEntry::External { uri, mime_type, .. } => {
                images_json.push(json!({ "uri": uri, "mimeType": mime_type }));
            }
        }
    }

    let buffers = vec![json!({ "byteLength": buffer.data.len() })];

    let gltf = json!({
        "asset": {
            "version": "2.0",
            "generator": "ufbx_rust"
        },
        "buffers": buffers,
        "bufferViews": buffer_views,
        "accessors": accessors,
        "images": images_json,
        "samplers": samplers,
        "textures": textures,
        "materials": materials,
        "meshes": [ { "primitives": primitives } ],
        "nodes": [ { "mesh": 0 } ],
        "scenes": [ { "nodes": [0] } ],
        "scene": 0
    });

    write_glb_container(path, gltf, buffer.data)
}

fn texture_index(
    texture: &Option<TextureSource>,
    images: &mut Vec<ImageEntry>,
    textures: &mut Vec<Value>,
    image_map: &mut HashMap<u64, usize>,
    texture_map: &mut HashMap<usize, usize>,
    sampler_index: usize,
    texture_mode: &mut TextureMode,
) -> Result<Option<TextureRef>> {
    let Some(texture) = texture else {
        return Ok(None);
    };

    let Some(image) = encode_texture(texture)? else {
        return Ok(None);
    };
    let hash = hash_bytes(&image.bytes);

    let image_index = if let Some(existing) = image_map.get(&hash) {
        *existing
    } else {
        let entry = match texture_mode {
            TextureMode::Embed => ImageEntry::Embedded(image),
            TextureMode::External(cache) => {
                let ext = if image.mime_type == "image/png" { "png" } else { "jpg" };
                let filename = cache
                    .map
                    .entry(hash)
                    .or_insert_with(|| format!("tex_{hash:016x}.{ext}"))
                    .clone();
                let path = cache.dir.join(&filename);
                if !path.exists() {
                    fs::write(&path, &image.bytes)
                        .with_context(|| format!("write texture {}", path.display()))?;
                }
                let prefix = cache.uri_prefix.trim_end_matches('/');
                let uri = if prefix.is_empty() {
                    filename
                } else {
                    format!("{}/{}", prefix, filename)
                };
                ImageEntry::External {
                    uri,
                    mime_type: image.mime_type,
                    has_alpha: image.has_alpha,
                }
            }
        };
        let idx = images.len();
        images.push(entry);
        image_map.insert(hash, idx);
        idx
    };

    let has_alpha = images[image_index].has_alpha();
    if let Some(existing) = texture_map.get(&image_index) {
        return Ok(Some(TextureRef {
            texture_index: *existing,
            has_alpha,
        }));
    }

    let texture_index = textures.len();
    textures.push(json!({
        "sampler": sampler_index,
        "source": image_index
    }));
    texture_map.insert(image_index, texture_index);
    Ok(Some(TextureRef {
        texture_index,
        has_alpha,
    }))
}

fn hash_bytes(bytes: &[u8]) -> u64 {
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    hasher.finish()
}

fn ensure_normals(positions: &[f32], normals: &[f32]) -> Vec<f32> {
    if normals.len() == positions.len() && !normals.is_empty() {
        return normals.to_vec();
    }
    generate_flat_normals(positions)
}

fn ensure_uvs(vertex_count: usize, uvs: &[f32]) -> Vec<f32> {
    if uvs.len() == vertex_count * 2 {
        return uvs.to_vec();
    }
    vec![0.0; vertex_count * 2]
}

fn ensure_colors(vertex_count: usize, colors: &[f32]) -> Vec<f32> {
    if colors.len() == vertex_count * 4 {
        return colors.to_vec();
    }
    vec![1.0; vertex_count * 4]
}

fn generate_flat_normals(positions: &[f32]) -> Vec<f32> {
    let vertex_count = positions.len() / 3;
    let mut normals = vec![0.0f32; vertex_count * 3];

    for tri in (0..vertex_count).step_by(3) {
        let p0 = vec3_from_slice(positions, tri * 3);
        let p1 = vec3_from_slice(positions, (tri + 1) * 3);
        let p2 = vec3_from_slice(positions, (tri + 2) * 3);

        let e1 = [p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]];
        let e2 = [p2[0] - p0[0], p2[1] - p0[1], p2[2] - p0[2]];

        let mut n = [
            e1[1] * e2[2] - e1[2] * e2[1],
            e1[2] * e2[0] - e1[0] * e2[2],
            e1[0] * e2[1] - e1[1] * e2[0],
        ];
        let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        if len > f32::EPSILON {
            n[0] /= len;
            n[1] /= len;
            n[2] /= len;
        } else {
            n = [0.0, 1.0, 0.0];
        }

        for v in 0..3 {
            let idx = tri + v;
            normals[idx * 3 + 0] = n[0];
            normals[idx * 3 + 1] = n[1];
            normals[idx * 3 + 2] = n[2];
        }
    }

    normals
}

fn compute_tangents(positions: &[f32], uvs: &[f32], normals: &[f32]) -> Vec<f32> {
    let vertex_count = positions.len() / 3;
    if vertex_count == 0 {
        return Vec::new();
    }

    let mut tangents = vec![0.0f32; vertex_count * 4];

    for tri in (0..vertex_count).step_by(3) {
        let p0 = vec3_from_slice(positions, tri * 3);
        let p1 = vec3_from_slice(positions, (tri + 1) * 3);
        let p2 = vec3_from_slice(positions, (tri + 2) * 3);

        let uv0 = vec2_from_slice(uvs, tri * 2);
        let uv1 = vec2_from_slice(uvs, (tri + 1) * 2);
        let uv2 = vec2_from_slice(uvs, (tri + 2) * 2);

        let edge1 = [p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]];
        let edge2 = [p2[0] - p0[0], p2[1] - p0[1], p2[2] - p0[2]];

        let delta_uv1 = [uv1[0] - uv0[0], uv1[1] - uv0[1]];
        let delta_uv2 = [uv2[0] - uv0[0], uv2[1] - uv0[1]];

        let denom = delta_uv1[0] * delta_uv2[1] - delta_uv1[1] * delta_uv2[0];
        let (tangent, bitangent) = if denom.abs() > f32::EPSILON {
            let r = 1.0 / denom;
            let tangent = [
                (edge1[0] * delta_uv2[1] - edge2[0] * delta_uv1[1]) * r,
                (edge1[1] * delta_uv2[1] - edge2[1] * delta_uv1[1]) * r,
                (edge1[2] * delta_uv2[1] - edge2[2] * delta_uv1[1]) * r,
            ];
            let bitangent = [
                (edge2[0] * delta_uv1[0] - edge1[0] * delta_uv2[0]) * r,
                (edge2[1] * delta_uv1[0] - edge1[1] * delta_uv2[0]) * r,
                (edge2[2] * delta_uv1[0] - edge1[2] * delta_uv2[0]) * r,
            ];
            (tangent, bitangent)
        } else {
            ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0])
        };

        for v in 0..3 {
            let idx = tri + v;
            let normal = vec3_from_slice(normals, idx * 3);
            let t = orthonormalize(normal, tangent);
            let w = handedness(normal, t, bitangent);
            tangents[idx * 4 + 0] = t[0];
            tangents[idx * 4 + 1] = t[1];
            tangents[idx * 4 + 2] = t[2];
            tangents[idx * 4 + 3] = w;
        }
    }

    tangents
}

fn vec2_from_slice(data: &[f32], start: usize) -> [f32; 2] {
    if data.len() >= start + 2 {
        [data[start], data[start + 1]]
    } else {
        [0.0, 0.0]
    }
}

fn vec3_from_slice(data: &[f32], start: usize) -> [f32; 3] {
    if data.len() >= start + 3 {
        [data[start], data[start + 1], data[start + 2]]
    } else {
        [0.0, 1.0, 0.0]
    }
}

fn orthonormalize(normal: [f32; 3], tangent: [f32; 3]) -> [f32; 3] {
    let dot = normal[0] * tangent[0] + normal[1] * tangent[1] + normal[2] * tangent[2];
    let mut t = [
        tangent[0] - normal[0] * dot,
        tangent[1] - normal[1] * dot,
        tangent[2] - normal[2] * dot,
    ];
    let len = (t[0] * t[0] + t[1] * t[1] + t[2] * t[2]).sqrt();
    if len > f32::EPSILON {
        t[0] /= len;
        t[1] /= len;
        t[2] /= len;
    } else {
        t = [1.0, 0.0, 0.0];
    }
    t
}

fn handedness(normal: [f32; 3], tangent: [f32; 3], bitangent: [f32; 3]) -> f32 {
    let cross = [
        normal[1] * tangent[2] - normal[2] * tangent[1],
        normal[2] * tangent[0] - normal[0] * tangent[2],
        normal[0] * tangent[1] - normal[1] * tangent[0],
    ];
    let dot = cross[0] * bitangent[0] + cross[1] * bitangent[1] + cross[2] * bitangent[2];
    if dot < 0.0 {
        -1.0
    } else {
        1.0
    }
}

fn push_accessor_vec3(
    buffer: &mut BufferBuilder,
    buffer_views: &mut Vec<Value>,
    accessors: &mut Vec<Value>,
    data: &[f32],
    target: u32,
) -> Result<(usize, [f32; 3], [f32; 3])> {
    let view_index = buffer.push_f32(buffer_views, data, Some(target))?;
    let count = data.len() / 3;
    let accessor_index = accessors.len();
    accessors.push(json!({
        "bufferView": view_index,
        "componentType": 5126,
        "count": count,
        "type": "VEC3"
    }));

    let (min, max) = min_max_vec3(data);
    Ok((accessor_index, min, max))
}


fn push_accessor_vec2(
    buffer: &mut BufferBuilder,
    buffer_views: &mut Vec<Value>,
    accessors: &mut Vec<Value>,
    data: &[f32],
    target: u32,
) -> Result<(usize, usize)> {
    let view_index = buffer.push_f32(buffer_views, data, Some(target))?;
    let count = data.len() / 2;
    let accessor_index = accessors.len();
    accessors.push(json!({
        "bufferView": view_index,
        "componentType": 5126,
        "count": count,
        "type": "VEC2"
    }));
    Ok((accessor_index, count))
}


fn push_accessor_vec4(
    buffer: &mut BufferBuilder,
    buffer_views: &mut Vec<Value>,
    accessors: &mut Vec<Value>,
    data: &[f32],
    target: u32,
) -> Result<(usize, usize)> {
    let view_index = buffer.push_f32(buffer_views, data, Some(target))?;
    let count = data.len() / 4;
    let accessor_index = accessors.len();
    accessors.push(json!({
        "bufferView": view_index,
        "componentType": 5126,
        "count": count,
        "type": "VEC4"
    }));
    Ok((accessor_index, count))
}


fn update_accessor_bounds(accessor: &mut Value, min: [f32; 3], max: [f32; 3]) {
    accessor["min"] = json!(min);
    accessor["max"] = json!(max);
}

fn min_max_vec3(data: &[f32]) -> ([f32; 3], [f32; 3]) {
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];

    for chunk in data.chunks(3) {
        if chunk.len() < 3 {
            continue;
        }
        for i in 0..3 {
            if chunk[i] < min[i] {
                min[i] = chunk[i];
            }
            if chunk[i] > max[i] {
                max[i] = chunk[i];
            }
        }
    }

    if min[0] == f32::INFINITY {
        min = [0.0, 0.0, 0.0];
        max = [0.0, 0.0, 0.0];
    }

    (min, max)
}

#[derive(Default)]
struct BufferBuilder {
    data: Vec<u8>,
}

impl BufferBuilder {
    fn align4(&mut self) {
        let pad = (4 - (self.data.len() % 4)) % 4;
        if pad > 0 {
            self.data.extend(std::iter::repeat(0u8).take(pad));
        }
    }

    fn push_f32(
        &mut self,
        buffer_views: &mut Vec<Value>,
        data: &[f32],
        target: Option<u32>,
    ) -> Result<usize> {
        let mut bytes = Vec::with_capacity(data.len() * 4);
        for value in data {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        let (view_index, _) = self.push_bytes(buffer_views, &bytes, target)?;
        Ok(view_index)
    }

    fn push_bytes(
        &mut self,
        buffer_views: &mut Vec<Value>,
        bytes: &[u8],
        target: Option<u32>,
    ) -> Result<(usize, usize)> {
        self.align4();
        let offset = self.data.len();
        self.data.extend_from_slice(bytes);
        let length = bytes.len();
        self.align4();

        let mut view = json!({
            "buffer": 0,
            "byteOffset": offset,
            "byteLength": length
        });
        if let Some(target) = target {
            view["target"] = json!(target);
        }
        let view_index = buffer_views.len();
        buffer_views.push(view);
        Ok((view_index, length))
    }
}


fn write_glb_container(path: &Path, gltf: Value, mut bin: Vec<u8>) -> Result<()> {
    let mut json_bytes = serde_json::to_vec(&gltf)?;
    pad_bytes(&mut json_bytes, 0x20);

    pad_bytes(&mut bin, 0x00);

    let total_length = 12 + 8 + json_bytes.len() + 8 + bin.len();

    let mut file = File::create(path)
        .with_context(|| format!("open output file {}", path.display()))?;

    file.write_all(&GLTF_MAGIC.to_le_bytes())?;
    file.write_all(&GLTF_VERSION.to_le_bytes())?;
    file.write_all(&(total_length as u32).to_le_bytes())?;

    file.write_all(&(json_bytes.len() as u32).to_le_bytes())?;
    file.write_all(&CHUNK_TYPE_JSON.to_le_bytes())?;
    file.write_all(&json_bytes)?;

    file.write_all(&(bin.len() as u32).to_le_bytes())?;
    file.write_all(&CHUNK_TYPE_BIN.to_le_bytes())?;
    file.write_all(&bin)?;

    Ok(())
}

fn pad_bytes(bytes: &mut Vec<u8>, pad: u8) {
    let pad_len = (4 - (bytes.len() % 4)) % 4;
    if pad_len > 0 {
        bytes.extend(std::iter::repeat(pad).take(pad_len));
    }
}
