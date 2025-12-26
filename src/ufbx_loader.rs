use crate::ufbx_sys::{
    ufbx_export_scene_from_file, ufbx_free_export_scene, ufbx_free_string, UfbxExportScene,
    UfbxMaterialInfo, UfbxMeshPartInfo, UfbxTextureRef,
};
use anyhow::{bail, Result};
use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::path::{Path, PathBuf};
use std::slice;

#[derive(Clone, Debug)]
pub enum TextureSource {
    Embedded { bytes: Vec<u8>, name: Option<String> },
    File(PathBuf),
}

#[derive(Clone, Debug)]
pub struct Material {
    pub name: Option<String>,
    pub base_color: [f32; 4],
    pub emissive: [f32; 3],
    pub metallic: f32,
    pub roughness: f32,
    pub double_sided: bool,
    pub base_color_texture: Option<TextureSource>,
    pub normal_texture: Option<TextureSource>,
    pub emissive_texture: Option<TextureSource>,
}

#[derive(Clone, Debug)]
pub struct MeshPart {
    pub name: Option<String>,
    pub material_index: usize,
    pub positions: Vec<f32>,
    pub normals: Vec<f32>,
    pub uvs: Vec<f32>,
    pub colors: Vec<f32>,
}

#[derive(Clone, Debug)]
pub struct SceneData {
    pub materials: Vec<Material>,
    pub parts: Vec<MeshPart>,
    pub right_axis: AxisDir,
    pub up_axis: AxisDir,
}

#[derive(Clone, Copy, Debug)]
pub enum AxisDir {
    PosX,
    NegX,
    PosY,
    NegY,
    PosZ,
    NegZ,
    Unknown,
}

impl AxisDir {
    pub fn from_ufbx(value: i32) -> Self {
        match value {
            0 => AxisDir::PosX,
            1 => AxisDir::NegX,
            2 => AxisDir::PosY,
            3 => AxisDir::NegY,
            4 => AxisDir::PosZ,
            5 => AxisDir::NegZ,
            _ => AxisDir::Unknown,
        }
    }

}

pub fn flip_v(scene: &mut SceneData) {
    for part in &mut scene.parts {
        for uv in part.uvs.chunks_mut(2) {
            if uv.len() == 2 {
                uv[1] = 1.0 - uv[1];
            }
        }
    }
}

fn read_optional_c_string(ptr: *const c_char) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    let value = unsafe { CStr::from_ptr(ptr) };
    Some(value.to_string_lossy().into_owned())
}

fn texture_from_ref(tex: &UfbxTextureRef, base_dir: &Path) -> Option<TextureSource> {
    if !tex.content.is_null() && tex.content_size > 0 {
        let bytes = unsafe { slice::from_raw_parts(tex.content, tex.content_size) };
        let name = read_optional_c_string(tex.path);
        return Some(TextureSource::Embedded {
            bytes: bytes.to_vec(),
            name,
        });
    }

    if tex.path.is_null() {
        return None;
    }
    let path_str = unsafe { CStr::from_ptr(tex.path) }.to_string_lossy().into_owned();
    let path = PathBuf::from(path_str);
    let resolved = if path.is_absolute() {
        path
    } else {
        base_dir.join(path)
    };
    Some(TextureSource::File(resolved))
}

fn material_from_raw(raw: &UfbxMaterialInfo, base_dir: &Path) -> Material {
    Material {
        name: read_optional_c_string(raw.name),
        base_color: raw.base_color,
        emissive: raw.emissive,
        metallic: raw.metallic,
        roughness: raw.roughness,
        double_sided: raw.double_sided,
        base_color_texture: texture_from_ref(&raw.base_color_texture, base_dir),
        normal_texture: texture_from_ref(&raw.normal_texture, base_dir),
        emissive_texture: texture_from_ref(&raw.emissive_texture, base_dir),
    }
}

fn mesh_part_from_raw(raw: &UfbxMeshPartInfo) -> MeshPart {
    let vertex_count = raw.vertex_count as usize;
    let positions_len = vertex_count * 3;
    let normals_len = vertex_count * 3;
    let uvs_len = vertex_count * 2;
    let colors_len = vertex_count * 4;

    let positions = if raw.positions.is_null() || positions_len == 0 {
        Vec::new()
    } else {
        unsafe { slice::from_raw_parts(raw.positions, positions_len) }.to_vec()
    };

    let normals = if raw.normals.is_null() || normals_len == 0 {
        Vec::new()
    } else {
        unsafe { slice::from_raw_parts(raw.normals, normals_len) }.to_vec()
    };

    let uvs = if raw.uvs.is_null() || uvs_len == 0 {
        Vec::new()
    } else {
        unsafe { slice::from_raw_parts(raw.uvs, uvs_len) }.to_vec()
    };

    let colors = if raw.colors.is_null() || colors_len == 0 {
        Vec::new()
    } else {
        unsafe { slice::from_raw_parts(raw.colors, colors_len) }.to_vec()
    };

    MeshPart {
        name: read_optional_c_string(raw.name),
        material_index: raw.material_index as usize,
        positions,
        normals,
        uvs,
        colors,
    }
}

pub fn load_scene(path: &Path) -> Result<SceneData> {
    let base_dir = path.parent().unwrap_or_else(|| Path::new("."));
    let c_path = CString::new(path.to_string_lossy().as_bytes())?;

    let mut error_ptr = std::ptr::null_mut();
    let raw_scene = unsafe { ufbx_export_scene_from_file(c_path.as_ptr(), &mut error_ptr) };

    if raw_scene.is_null() {
        let message = if !error_ptr.is_null() {
            let msg = read_optional_c_string(error_ptr).unwrap_or_else(|| "Unknown error".to_string());
            unsafe {
                ufbx_free_string(error_ptr);
            }
            msg
        } else {
            "Unknown error".to_string()
        };
        bail!("ufbx load failed: {message}");
    }

    let export = unsafe { &*raw_scene };
    let right_axis = AxisDir::from_ufbx(export.right_axis);
    let up_axis = AxisDir::from_ufbx(export.up_axis);
    let materials = unsafe { slice::from_raw_parts(export.materials, export.material_count) }
        .iter()
        .map(|raw| material_from_raw(raw, base_dir))
        .collect::<Vec<_>>();

    let parts = unsafe { slice::from_raw_parts(export.parts, export.part_count) }
        .iter()
        .map(mesh_part_from_raw)
        .collect::<Vec<_>>();

    unsafe {
        ufbx_free_export_scene(raw_scene);
    }

    if parts.is_empty() {
        bail!("no mesh data found in FBX");
    }

    Ok(SceneData {
        materials,
        parts,
        right_axis,
        up_axis,
    })
}

#[allow(dead_code)]
fn _ensure_linked(_scene: &UfbxExportScene) {}
