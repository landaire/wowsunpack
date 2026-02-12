//! Utilities for loading game resources from a World of Warships installation directory.
//!
//! Provides version-matched resource loading: given a replay's version, finds and loads
//! the corresponding game build rather than blindly using the latest installed build.

use std::borrow::Cow;
use std::fs::read_dir;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use crate::data::idx::{self, FileNode};
use crate::data::pkg::PkgFileLoader;
use crate::data::{DataFileWithCallback, Version};
use crate::error::ErrorKind;
use crate::rpc::entitydefs::{EntitySpec, parse_scripts};

/// Find the build directory in the game directory that matches the replay's version.
///
/// The game directory has a `bin/` folder containing numbered build directories
/// (e.g. `bin/11791718/`). The replay's `clientVersionFromExe` field encodes the
/// build number as the fourth component (e.g. `"15,0,0,11791718"`).
///
/// Returns the matching build number, or an error listing available builds.
pub fn find_matching_build(game_dir: &Path, replay_version: &Version) -> Result<u32, ErrorKind> {
    let bin_dir = game_dir.join("bin");
    let mut available_builds: Vec<u32> = Vec::new();

    for entry in read_dir(&bin_dir)? {
        let entry = entry?;
        if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            if let Some(build_num) = entry
                .file_name()
                .to_str()
                .and_then(|name| name.parse::<u32>().ok())
            {
                available_builds.push(build_num);
            }
        }
    }

    available_builds.sort();

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
    pub file_tree: FileNode,
    pub pkg_loader: PkgFileLoader,
}

/// Load game resources (entity specs, file tree, package loader) from a game directory,
/// using the build number that matches the replay version.
///
/// This is the common resource-loading logic shared by replayshark, minimap-renderer,
/// and any other tool that processes replays against game data.
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
            let mut cursor = Cursor::new(file_data.as_slice());
            idx_files.push(idx::parse(&mut cursor)?);
        }
    }

    let pkgs_path = game_dir.join("res_packages");
    if !pkgs_path.exists() {
        return Err(ErrorKind::ParsingFailure(
            "Invalid game directory -- res_packages not found".to_string(),
        ));
    }

    let pkg_loader = PkgFileLoader::new(pkgs_path);
    let file_tree = idx::build_file_tree(idx_files.as_slice());

    let specs = {
        let loader = DataFileWithCallback::new(|path| {
            let path = Path::new(path);
            let mut file_data = Vec::new();
            file_tree
                .read_file_at_path(path, &pkg_loader, &mut file_data)
                .unwrap();
            Ok(Cow::Owned(file_data))
        });
        parse_scripts(&loader)?
    };

    Ok(GameResources {
        specs,
        file_tree,
        pkg_loader,
    })
}

/// Returns the path to the English translations file for the given build.
pub fn translations_path(game_dir: &Path, build: u32) -> PathBuf {
    game_dir
        .join("bin")
        .join(build.to_string())
        .join("res/texts/en/LC_MESSAGES/global.mo")
}
