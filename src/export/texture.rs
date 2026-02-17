//! DDS texture loading and conversion for glTF export.

use std::io::Cursor;

use image_dds::image::ExtendedColorType;
use image_dds::image::ImageEncoder;
use image_dds::image::codecs::png::PngEncoder;
use rootcause::Report;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum TextureError {
    #[error("failed to parse DDS: {0}")]
    DdsParse(String),
    #[error("failed to decode DDS image: {0}")]
    DdsDecode(String),
    #[error("failed to encode PNG: {0}")]
    PngEncode(String),
}

/// Decode DDS bytes to PNG bytes (RGBA8).
pub fn dds_to_png(dds_bytes: &[u8]) -> Result<Vec<u8>, Report<TextureError>> {
    let dds = image_dds::ddsfile::Dds::read(&mut Cursor::new(dds_bytes))
        .map_err(|e| Report::new(TextureError::DdsParse(e.to_string())))?;

    let rgba_image = image_dds::image_from_dds(&dds, 0)
        .map_err(|e| Report::new(TextureError::DdsDecode(e.to_string())))?;

    let mut png_buf = Vec::new();
    PngEncoder::new(&mut png_buf)
        .write_image(
            rgba_image.as_raw(),
            rgba_image.width(),
            rgba_image.height(),
            ExtendedColorType::Rgba8,
        )
        .map_err(|e| Report::new(TextureError::PngEncode(e.to_string())))?;

    Ok(png_buf)
}

const TEXTURE_BASE: &str = "content/gameplay/common/camouflage/textures";

/// MFM name suffixes that don't appear in texture filenames.
///
/// E.g. MFM `AGM034_16in50_Mk7_skinned.mfm` → texture `AGM034_16in50_Mk7_camo_01.dds`.
const MFM_STRIP_SUFFIXES: &[&str] = &["_skinned", "_wire", "_dead", "_blaze", "_alpha"];

/// Derive texture base names from an MFM stem.
///
/// Returns the original stem first, then the stem with known MFM-only suffixes
/// stripped (e.g. `_skinned`). This allows matching both hull-style stems
/// (where `JSB039_Yamato_1945_Hull` IS the texture name) and turret-style stems
/// (where `AGM034_16in50_Mk7_skinned` maps to `AGM034_16in50_Mk7`).
pub fn texture_base_names(mfm_stem: &str) -> Vec<String> {
    let mut names = vec![mfm_stem.to_string()];
    for suffix in MFM_STRIP_SUFFIXES {
        if let Some(stripped) = mfm_stem.strip_suffix(suffix) {
            if !names.contains(&stripped.to_string()) {
                names.push(stripped.to_string());
            }
        }
    }
    names
}

/// Load a texture for a given MFM stem and scheme from a VFS.
///
/// Given an MFM leaf like `JSB039_Yamato_1945_Hull` and scheme like `camo_01`,
/// tries `{stem}_{scheme}.dd0` first (highest res), then `.dds`.
/// Also tries with known MFM suffixes stripped (e.g. `_skinned`) to handle
/// turret models where the texture name differs from the MFM name.
///
/// Returns `(base_name, dds_bytes)` if found, or `None`.
pub fn load_texture_bytes(
    vfs: &vfs::VfsPath,
    mfm_stem: &str,
    scheme: &str,
) -> Option<(String, Vec<u8>)> {
    for base in texture_base_names(mfm_stem) {
        let candidates = [
            format!("{TEXTURE_BASE}/{base}_{scheme}.dd0"),
            format!("{TEXTURE_BASE}/{base}_{scheme}.dds"),
        ];

        for path in &candidates {
            if let Ok(vfs_path) = vfs.join(path) {
                if let Ok(mut file) = vfs_path.open_file() {
                    let mut data = Vec::new();
                    if std::io::Read::read_to_end(&mut file, &mut data).is_ok() && !data.is_empty()
                    {
                        return Some((base, data));
                    }
                }
            }
        }
    }

    None
}

/// Load the base albedo texture (`_a.dds`) from the MFM's own directory.
///
/// The base albedo is the "default" ship appearance — gray/weathered paint without
/// any camouflage applied. It lives next to the MFM file, e.g.:
/// `content/gameplay/japan/ship/battleship/textures/JSB039_Yamato_1945_Hull_a.dds`
///
/// `mfm_full_path` is the full VFS path to the MFM file (e.g. ending in `.mfm`).
/// Returns DDS bytes if found.
pub fn load_base_albedo_bytes(vfs: &vfs::VfsPath, mfm_full_path: &str) -> Option<Vec<u8>> {
    let dir = mfm_full_path.rsplit_once('/')?.0;
    let mfm_filename = mfm_full_path.rsplit_once('/')?.1;
    let stem = mfm_filename.strip_suffix(".mfm")?;

    for base in texture_base_names(stem) {
        let candidates = [format!("{dir}/{base}_a.dd0"), format!("{dir}/{base}_a.dds")];
        for path in &candidates {
            if let Ok(vfs_path) = vfs.join(path) {
                if let Ok(mut file) = vfs_path.open_file() {
                    let mut data = Vec::new();
                    if std::io::Read::read_to_end(&mut file, &mut data).is_ok() && !data.is_empty()
                    {
                        return Some(data);
                    }
                }
            }
        }
    }

    None
}

/// Discover available texture schemes for a set of MFM stems by scanning the VFS.
///
/// Returns sorted, deduplicated scheme names (e.g. `["camo_01", "GW_a"]`).
pub fn discover_texture_schemes(vfs: &vfs::VfsPath, mfm_stems: &[String]) -> Vec<String> {
    let mut schemes = std::collections::BTreeSet::new();

    let Ok(tex_dir) = vfs.join(TEXTURE_BASE) else {
        return Vec::new();
    };
    let Ok(entries) = tex_dir.read_dir() else {
        return Vec::new();
    };

    // Collect filenames ending in .dds (base mip level — avoids counting .dd0/.dd1/.dd2 dupes).
    let dds_names: Vec<String> = entries
        .filter_map(|entry| {
            let name = entry.filename();
            if name.ends_with(".dds") {
                Some(name)
            } else {
                None
            }
        })
        .collect();

    for stem in mfm_stems {
        for base in texture_base_names(stem) {
            let prefix = format!("{base}_");
            for name in &dds_names {
                if let Some(rest) = name.strip_prefix(&prefix) {
                    if let Some(scheme) = rest.strip_suffix(".dds") {
                        if !scheme.is_empty() {
                            schemes.insert(scheme.to_string());
                        }
                    }
                }
            }
        }
    }

    schemes.into_iter().collect()
}
