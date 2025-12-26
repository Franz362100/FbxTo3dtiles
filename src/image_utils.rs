use crate::ufbx_loader::TextureSource;
use anyhow::{Context, Result};
use image::{DynamicImage, ImageFormat};
use std::fs;
use std::io::Cursor;
use std::path::Path;

pub struct ImageData {
    pub bytes: Vec<u8>,
    pub mime_type: String,
    pub has_alpha: bool,
}

pub fn encode_texture(source: &TextureSource) -> Result<Option<ImageData>> {
    match source {
        TextureSource::Embedded { bytes, name } => encode_from_bytes(bytes, name.as_deref()),
        TextureSource::File(path) => match encode_from_path(path) {
            Ok(image) => Ok(Some(image)),
            Err(err) => {
                eprintln!("warning: texture {} skipped: {err}", path.display());
                Ok(None)
            }
        },
    }
}

fn encode_from_path(path: &Path) -> Result<ImageData> {
    let ext = path
        .extension()
        .and_then(|v| v.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    if ext == "png" || ext == "jpg" || ext == "jpeg" {
        let bytes =
            fs::read(path).with_context(|| format!("read texture {}", path.display()))?;
        if ext == "png" {
            let image = image::load_from_memory_with_format(&bytes, ImageFormat::Png)
                .with_context(|| format!("decode texture {}", path.display()))?;
            return Ok(ImageData {
                bytes,
                mime_type: "image/png".to_string(),
                has_alpha: image.color().has_alpha(),
            });
        }
        return Ok(ImageData {
            bytes,
            mime_type: "image/jpeg".to_string(),
            has_alpha: false,
        });
    }

    let image = image::open(path)
        .with_context(|| format!("decode texture {}", path.display()))?;
    encode_image(image)
}

fn encode_from_bytes(bytes: &[u8], name: Option<&str>) -> Result<Option<ImageData>> {
    if let Ok(format) = image::guess_format(bytes) {
        if format == ImageFormat::Png {
            if let Ok(image) = image::load_from_memory_with_format(bytes, ImageFormat::Png) {
                return Ok(Some(ImageData {
                    bytes: bytes.to_vec(),
                    mime_type: "image/png".to_string(),
                    has_alpha: image.color().has_alpha(),
                }));
            }
        }
        if format == ImageFormat::Jpeg {
            return Ok(Some(ImageData {
                bytes: bytes.to_vec(),
                mime_type: "image/jpeg".to_string(),
                has_alpha: false,
            }));
        }
        if let Ok(image) = image::load_from_memory_with_format(bytes, format) {
            return Ok(Some(encode_image(image)?));
        }
    }

    if let Some(format) = name.and_then(format_from_name) {
        if let Ok(image) = image::load_from_memory_with_format(bytes, format) {
            return Ok(Some(encode_image(image)?));
        }
    }

    match image::load_from_memory(bytes) {
        Ok(image) => Ok(Some(encode_image(image)?)),
        Err(err) => {
            if let Some(name) = name {
                eprintln!("warning: could not decode embedded texture {name}: {err}");
            } else {
                eprintln!("warning: could not decode embedded texture: {err}");
            }
            Ok(None)
        }
    }
}

fn format_from_name(name: &str) -> Option<ImageFormat> {
    let ext = Path::new(name).extension()?.to_str()?;
    ImageFormat::from_extension(ext)
}

fn encode_image(image: DynamicImage) -> Result<ImageData> {
    let has_alpha = image.color().has_alpha();
    let format = if has_alpha {
        ImageFormat::Png
    } else {
        ImageFormat::Jpeg
    };
    let mime_type = if has_alpha {
        "image/png"
    } else {
        "image/jpeg"
    };

    let mut data = Vec::new();
    let mut cursor = Cursor::new(&mut data);
    image.write_to(&mut cursor, format)?;

    Ok(ImageData {
        bytes: data,
        mime_type: mime_type.to_string(),
        has_alpha,
    })
}
