# FBX 转 glTF 2.0（GLB）/ 3D Tiles 1.1

基于 UFBX 的 FBX 转换工具，支持导出 GLB 与 3D Tiles 1.1。

## 环境与依赖

- GLB 输出默认嵌入纹理（无外部文件）。
- 3D Tiles 输出默认将纹理写入 `output_dir\\textures` 共享目录以减少体积。

UFBX 源码默认放在 `vendor/ufbx/`：
- `vendor/ufbx/ufbx.h`
- `vendor/ufbx/ufbx.c`

也可以通过环境变量 `UFBX_DIR` 指定包含上述文件的目录。

## 构建

```powershell
cargo build
```

## 运行（GLB）

```powershell
cargo run -- path\to\input.fbx path\to\output.glb [--no-flip-v]
```

参数说明（GLB）

- `input`：输入 FBX 路径
- `output`：输出 GLB 路径
- `--no-flip-v`：不翻转 UV 的 V 方向（默认会翻转 V）

## 运行（3D Tiles 1.1）

```powershell
cargo run -- tiles path\to\input.fbx path\to\output_dir [options]
```

输出结构：
- `output_dir\\tileset.json`
- `output_dir\\tiles\\`（每个 tile 一个 GLB）
- `output_dir\\textures\\`（默认共享纹理）

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

参数说明（Tiles）

- `input`：输入 FBX 路径
- `output_dir`：输出目录（写入 `tileset.json`、`tiles/`、`textures/`）
- `--origin-lat`：原点纬度（度）
- `--origin-lon`：原点经度（度）
- `--origin-height`：原点高度（米）
- `--heading`：本地坐标绕 +Y 的旋转角（度）
- `--scale`：FBX 坐标缩放系数
- `--tile-size`：根层 tile 尺寸（米）
- `--min-tile-size`：最小 tile 尺寸（米），用于推导最大层级
- `--max-level`：覆盖最大四叉树层级（优先级高于 `min-tile-size` 推导）
- `--embed-textures`：将纹理嵌入每个 tile（默认共享外部纹理）
- `--no-flip-v`：不翻转 UV 的 V 方向（默认会翻转 V）

## 备注

- 几何通过 UFBX 三角化，并转换为右手系 Y-up 的 glTF。
- 输出包含 POSITION/NORMAL/UV/COLOR/TANGENT。
- Lambert/Phong 材质近似为金属-粗糙度 PBR。
- GLB 输出默认嵌入纹理；不支持的格式会尽量转为 PNG/JPG。
- 3D Tiles 默认共享纹理目录 `output_dir\\textures`，可用 `--embed-textures` 改为每个 tile 内嵌。



## License

This project is licensed under MIT License.

### Embedded Third-party Code

- **UFBX** (MIT License) - Bundled in `vendor/ufbx/`
  - See `vendor/ufbx/LICENSE` for details

### Rust Dependencies

All Rust dependencies are listed in `Cargo.toml` with their respective licenses.
Use `cargo license` to view the full dependency license list.