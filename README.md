# FbxTo3dtiles

FBX -> glTF 2.0 GLB converter using UFBX.

## Setup

The converter embeds textures into the GLB output (no external files).
3D Tiles export defaults to shared external textures under `output_dir\\textures` to reduce size.

UFBX sources are downloaded into `vendor/ufbx/`:
- `vendor/ufbx/ufbx.h`
- `vendor/ufbx/ufbx.c`

You can also set `UFBX_DIR` to a folder containing those files.

## Build

```powershell
cargo build
```

## Run

```powershell
cargo run -- path\to\input.fbx path\to\output.glb
```

## 3D Tiles 1.1

```powershell
cargo run -- tiles path\to\input.fbx path\to\output_dir
```

默认地理参考：
- 经纬度：116.397026 / 39.918058
- 高度：50m
- heading：0
- scale：1

可用参数（示例）：

```powershell
cargo run -- tiles path\to\input.fbx path\to\output_dir `
  --origin-lat 39.918058 --origin-lon 116.397026 --origin-height 50 `
  --heading 0 --scale 1 --tile-size 100 --min-tile-size 12.5 `
  --embed-textures
```

可选参数：
- `--no-flip-v`：禁用 UV 的 V 方向翻转（默认会翻转 V）。

## Notes

- Geometry is triangulated via UFBX and converted to glTF right-handed Y-up.
- Output includes POSITION/NORMAL/UV/COLOR/TANGENT for each primitive.
- Lambert/Phong materials are approximated to metallic-roughness PBR.
- GLB output embeds textures; unsupported formats are re-encoded to PNG/JPG when possible.
- 3D Tiles output stores textures under `output_dir\\textures` (shared); use `--embed-textures` to force per-tile embedding.

## Third-party

See `THIRD_PARTY.md` for a list of bundled/open-source components.
