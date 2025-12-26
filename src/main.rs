use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

mod geo;
mod gltf_writer;
mod image_utils;
mod tiles;
mod ufbx_loader;
mod ufbx_sys;

#[derive(Parser)]
#[command(author, version, about)]
struct Args {
    /// Input FBX file path (gltf mode)
    input: Option<PathBuf>,
    /// Output GLB file path (gltf mode)
    output: Option<PathBuf>,
    /// Disable V flip on UVs (default: flip V)
    #[arg(long)]
    no_flip_v: bool,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Export a 3D Tiles 1.1 tileset
    Tiles {
        /// Input FBX file path
        input: PathBuf,
        /// Output directory for tileset.json and tiles/
        output_dir: PathBuf,
        /// Origin latitude in degrees
        #[arg(long, default_value_t = 39.918_058)]
        origin_lat: f64,
        /// Origin longitude in degrees
        #[arg(long, default_value_t = 116.397_026)]
        origin_lon: f64,
        /// Origin height in meters
        #[arg(long, default_value_t = 50.0)]
        origin_height: f64,
        /// Heading in degrees (rotation around +Y)
        #[arg(long, default_value_t = 0.0)]
        heading: f64,
        /// Scale factor applied to FBX coordinates
        #[arg(long, default_value_t = 1.0)]
        scale: f64,
        /// Root tile size in meters
        #[arg(long, default_value_t = 100.0)]
        tile_size: f64,
        /// Minimum tile size in meters
        #[arg(long, default_value_t = 12.5)]
        min_tile_size: f64,
        /// Maximum quadtree level override
        #[arg(long)]
        max_level: Option<u32>,
        /// Embed textures in each tile (default: shared external textures)
        #[arg(long)]
        embed_textures: bool,
        /// Disable V flip on UVs (default: flip V)
        #[arg(long)]
        no_flip_v: bool,
    },
}

fn main() -> Result<()> {
    let args = Args::parse();

    match args.command {
        Some(Command::Tiles {
            input,
            output_dir,
            origin_lat,
            origin_lon,
            origin_height,
            heading,
            scale,
            tile_size,
            min_tile_size,
            max_level,
            embed_textures,
            no_flip_v,
        }) => {
            let mut scene = ufbx_loader::load_scene(&input)
                .with_context(|| format!("failed to load FBX: {}", input.display()))?;
            if no_flip_v {
                ufbx_loader::flip_v(&mut scene);
            }
            let options = tiles::TilesetOptions {
                origin_lat,
                origin_lon,
                origin_height,
                heading,
                scale,
                tile_size,
                min_tile_size,
                max_level,
                embed_textures,
            };
            tiles::export_tileset(&scene, &output_dir, &options).with_context(|| {
                format!("failed to export tileset to {}", output_dir.display())
            })?;
        }
        None => {
            let input = args
                .input
                .ok_or_else(|| anyhow::anyhow!("missing input path"))?;
            let output = args
                .output
                .ok_or_else(|| anyhow::anyhow!("missing output path"))?;
            let mut scene = ufbx_loader::load_scene(&input)
                .with_context(|| format!("failed to load FBX: {}", input.display()))?;
            if args.no_flip_v {
                ufbx_loader::flip_v(&mut scene);
            }
            gltf_writer::write_glb(&scene, &output)
                .with_context(|| format!("failed to write GLB: {}", output.display()))?;
        }
    }

    Ok(())
}
