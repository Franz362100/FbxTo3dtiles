use std::os::raw::{c_char, c_void};

#[repr(C)]
pub struct UfbxTextureRef {
    pub path: *mut c_char,
    pub content: *mut u8,
    pub content_size: usize,
}

#[repr(C)]
pub struct UfbxMaterialInfo {
    pub name: *mut c_char,
    pub base_color: [f32; 4],
    pub emissive: [f32; 3],
    pub metallic: f32,
    pub roughness: f32,
    pub double_sided: bool,
    pub base_color_texture: UfbxTextureRef,
    pub normal_texture: UfbxTextureRef,
    pub emissive_texture: UfbxTextureRef,
}

#[repr(C)]
pub struct UfbxMeshPartInfo {
    pub name: *mut c_char,
    pub material_index: u32,
    pub vertex_count: u32,
    pub positions: *mut f32,
    pub normals: *mut f32,
    pub uvs: *mut f32,
    pub colors: *mut f32,
    pub has_normals: bool,
    pub has_uvs: bool,
    pub has_colors: bool,
}

#[repr(C)]
pub struct UfbxExportScene {
    pub materials: *mut UfbxMaterialInfo,
    pub material_count: usize,
    pub parts: *mut UfbxMeshPartInfo,
    pub part_count: usize,
    pub right_axis: i32,
    pub up_axis: i32,
    pub scene: *mut c_void,
}

unsafe extern "C" {
    pub fn ufbx_export_scene_from_file(
        path: *const c_char,
        error_msg: *mut *mut c_char,
    ) -> *mut UfbxExportScene;
    pub fn ufbx_free_export_scene(scene: *mut UfbxExportScene);
    pub fn ufbx_free_string(str: *mut c_char);
}
