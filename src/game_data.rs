//! Utilities for loading game resources from a World of Warships installation directory.
//!
//! Provides version-matched resource loading: given a replay's version, finds and loads
//! the corresponding game build rather than blindly using the latest installed build.

use std::borrow::Cow;
use std::fs::read_dir;
use std::io::Read;
use std::path::{Path, PathBuf};

use vfs::VfsPath;

use crate::data::idx;
use crate::data::idx_vfs::IdxVfs;
use crate::data::wrappers::mmap::MmapPkgSource;
use crate::data::{DataFileWithCallback, Version};
use crate::error::ErrorKind;
use crate::rpc::entitydefs::{EntitySpec, parse_scripts};

/// List all available build numbers in the game directory's `bin/` folder, sorted ascending.
pub fn list_available_builds(game_dir: &Path) -> Result<Vec<u32>, ErrorKind> {
    let bin_dir = game_dir.join("bin");
    let mut builds: Vec<u32> = Vec::new();
    for entry in read_dir(&bin_dir)? {
        let entry = entry?;
        if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            if let Some(build_num) = entry
                .file_name()
                .to_str()
                .and_then(|name| name.parse::<u32>().ok())
            {
                builds.push(build_num);
            }
        }
    }
    builds.sort();
    Ok(builds)
}

/// Find the build directory in the game directory that matches the replay's version.
pub fn find_matching_build(game_dir: &Path, replay_version: &Version) -> Result<u32, ErrorKind> {
    let available_builds = list_available_builds(game_dir)?;

    if available_builds.contains(&replay_version.build) {
        Ok(replay_version.build)
    } else {
        let available: Vec<String> = available_builds.iter().map(|b| b.to_string()).collect();
        Err(ErrorKind::ParsingFailure(format!(
            "Replay build {} not found in game directory. Available builds: [{}]",
            replay_version.build,
            available.join(", ")
        )))
    }
}

/// Loaded game resources from a WoWS installation.
pub struct GameResources {
    pub specs: Vec<EntitySpec>,
    pub vfs: VfsPath,
}

/// Load game resources (entity specs, VFS) from a game directory,
/// using the build number that matches the replay version.
pub fn load_game_resources(
    game_dir: &Path,
    replay_version: &Version,
) -> Result<GameResources, ErrorKind> {
    let build = find_matching_build(game_dir, replay_version)?;

    let idx_dir = game_dir.join("bin").join(build.to_string()).join("idx");
    let mut idx_files = Vec::new();

    for entry in read_dir(&idx_dir)? {
        let entry = entry?;
        if entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            let file_data = std::fs::read(entry.path())?;
            idx_files.push(idx::parse(&file_data)?);
        }
    }

    let pkgs_path = game_dir.join("res_packages");
    if !pkgs_path.exists() {
        return Err(ErrorKind::ParsingFailure(
            "Invalid game directory -- res_packages not found".to_string(),
        ));
    }

    let pkg_source = MmapPkgSource::new(&pkgs_path);
    let idx_vfs = IdxVfs::new(pkg_source, &idx_files);
    let vfs = VfsPath::new(idx_vfs);

    let specs = {
        let vfs_ref = &vfs;
        let loader = DataFileWithCallback::new(move |path: &str| {
            let file_path = vfs_ref
                .join(path)
                .map_err(|e| ErrorKind::ParsingFailure(format!("VFS path error: {e}")))?;
            let mut data = Vec::new();
            file_path
                .open_file()
                .map_err(|e| ErrorKind::ParsingFailure(format!("VFS open error: {e}")))?
                .read_to_end(&mut data)
                .map_err(|e| ErrorKind::IoError(e))?;
            Ok(Cow::Owned(data))
        });
        parse_scripts(&loader)?
    };

    Ok(GameResources { specs, vfs })
}

/// Returns the path to the English translations file for the given build.
pub fn translations_path(game_dir: &Path, build: u32) -> PathBuf {
    game_dir
        .join("bin")
        .join(build.to_string())
        .join("res/texts/en/LC_MESSAGES/global.mo")
}
