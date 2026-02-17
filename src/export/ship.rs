//! High-level ship model export API.
//!
//! Provides [`ShipAssets`] (shared expensive resources, created once) and
//! [`ShipModelContext`] (a fully-loaded ship, ready for GLB export).
//!
//! # Quick start
//! ```no_run
//! use wowsunpack::export::ship::{ShipAssets, ShipExportOptions};
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! # let vfs: vfs::VfsPath = todo!();
//! let assets = ShipAssets::load(&vfs)?;
//! let ctx = assets.load_ship("Yamato", &ShipExportOptions::default())?;
//! let mut file = std::fs::File::create("yamato.glb")?;
//! ctx.export_glb(&mut file)?;
//! # Ok(())
//! # }
//! ```

use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};

use rootcause::prelude::*;
use vfs::VfsPath;

use crate::data::ResourceLoader;
use crate::game_params::keys;
use crate::game_params::provider::GameMetadataProvider;
use crate::game_params::types::{GameParamProvider, MountPoint};
use crate::models::assets_bin::{self, PrototypeDatabase};
use crate::models::geometry;
use crate::models::visual::{self, VisualPrototype};

use super::gltf_export::{self, SubModel, TextureSet};
use super::texture;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Options controlling ship model export.
#[derive(Debug, Clone)]
pub struct ShipExportOptions {
    /// LOD level (0 = highest detail). Default: 0.
    pub lod: usize,
    /// Hull upgrade selection. `None` = first/stock hull.
    /// Accepts full upgrade name (e.g. "PJUH911_Yamato_1944") or a prefix
    /// match against the hull component name (e.g. "B").
    pub hull: Option<String>,
    /// Whether to embed textures in the GLB. Default: true.
    pub textures: bool,
    /// Export the damaged/destroyed hull state instead of intact.
    /// When true, crack geometry is included and patch geometry is excluded.
    /// Default: false (intact hull).
    pub damaged: bool,
}

impl Default for ShipExportOptions {
    fn default() -> Self {
        Self {
            lod: 0,
            hull: None,
            textures: true,
            damaged: false,
        }
    }
}

/// Resolved ship identity information.
#[derive(Debug, Clone)]
pub struct ShipInfo {
    /// Model directory name, e.g. "JSB039_Yamato_1945".
    pub model_dir: String,
    /// Translated display name if translations are loaded, e.g. "Yamato".
    pub display_name: Option<String>,
    /// GameParam index key, e.g. "PJSB018".
    pub param_index: String,
}

/// Summary of a hull upgrade for listing purposes.
#[derive(Debug, Clone)]
pub struct HullUpgradeInfo {
    /// Upgrade name (GameParam key), e.g. "PJUH911_Yamato_1944".
    pub name: String,
    /// Components in this upgrade: (type_key, component_name, mount_count).
    pub components: Vec<(String, String, usize)>,
}

// ---------------------------------------------------------------------------
// ShipAssets — shared expensive resources (created once)
// ---------------------------------------------------------------------------

/// Shared game assets for ship export operations.
///
/// Creating this is the expensive step (~18 seconds for GameParams parsing).
/// Reuse a single instance across multiple ship exports.
pub struct ShipAssets {
    assets_bin_bytes: Vec<u8>,
    vfs: VfsPath,
    metadata: GameMetadataProvider,
}

impl ShipAssets {
    /// Load shared assets from the VFS.
    ///
    /// This is expensive (~18 seconds) because it parses GameParams.
    /// Create once and reuse for multiple ships.
    pub fn load(vfs: &VfsPath) -> Result<Self, Report> {
        let mut assets_bin_bytes = Vec::new();
        vfs.join("content/assets.bin")
            .context("VFS path error")?
            .open_file()
            .context("Could not find content/assets.bin in VFS")?
            .read_to_end(&mut assets_bin_bytes)?;

        let metadata = GameMetadataProvider::from_vfs(vfs).context("Failed to load GameParams")?;

        Ok(Self {
            assets_bin_bytes,
            vfs: vfs.clone(),
            metadata,
        })
    }

    /// Set translations for display name resolution.
    pub fn set_translations(&mut self, catalog: gettext::Catalog) {
        self.metadata.set_translations(catalog);
    }

    /// Access the underlying `GameMetadataProvider`.
    pub fn metadata(&self) -> &GameMetadataProvider {
        &self.metadata
    }

    /// Find a ship by name (fuzzy display-name match or exact model dir).
    pub fn find_ship(&self, name: &str) -> Result<ShipInfo, Report> {
        let db = self.db()?;
        let self_id_index = db.build_self_id_index();

        // Strategy 1: try direct match against assets.bin paths.
        let needle = format!("/{name}/");
        let has_direct = db.paths_storage.iter().any(|e| {
            e.name.ends_with(".visual") && {
                // Reconstruct is expensive; just check if any visual file's
                // full path contains the needle. We only need one hit.
                let idx = db
                    .paths_storage
                    .iter()
                    .position(|x| std::ptr::eq(x, e))
                    .unwrap();
                db.reconstruct_path(idx, &self_id_index).contains(&needle)
            }
        });

        if has_direct {
            // Direct model dir match — no GameParams needed for identity.
            // Try to find the GameParam for richer info.
            let param = self.metadata.params().iter().find(|p| {
                p.vehicle()
                    .and_then(|v| v.model_path())
                    .map(|mp| mp.contains(name))
                    .unwrap_or(false)
            });

            return Ok(ShipInfo {
                model_dir: name.to_string(),
                display_name: param.and_then(|p| {
                    self.metadata
                        .localized_name_from_param(p)
                        .map(|s: &str| s.to_string())
                }),
                param_index: param.map(|p| p.index().to_string()).unwrap_or_default(),
            });
        }

        // Strategy 2: fuzzy display name match via GameParams.
        let normalized_input = unidecode::unidecode(name).to_lowercase();
        let mut matches: Vec<(String, String, String)> = Vec::new();

        for param in self.metadata.params() {
            let vehicle = match param.vehicle() {
                Some(v) => v,
                None => continue,
            };
            let model_path = match vehicle.model_path() {
                Some(p) => p,
                None => continue,
            };

            let display_name = self
                .metadata
                .localized_name_from_param(param)
                .map(|s: &str| s.to_string())
                .unwrap_or_else(|| param.index().to_string());

            let normalized_display = unidecode::unidecode(&display_name).to_lowercase();
            if normalized_display.contains(&normalized_input) {
                let dir = model_path
                    .rsplit_once('/')
                    .map(|(d, _)| d)
                    .unwrap_or(model_path);
                let dir_name = dir.rsplit('/').next().unwrap_or(dir);
                matches.push((
                    display_name,
                    param.index().to_string(),
                    dir_name.to_string(),
                ));
            }
        }

        match matches.len() {
            0 => bail!(
                "No ship found matching '{name}'. Try using the model directory name \
                 (e.g. 'JSB039_Yamato_1945')."
            ),
            1 => Ok(ShipInfo {
                model_dir: matches[0].2.clone(),
                display_name: Some(matches[0].0.clone()),
                param_index: matches[0].1.clone(),
            }),
            _ => {
                // If all matches share the same model dir, use it.
                let unique_dirs: HashSet<&str> =
                    matches.iter().map(|(_, _, d)| d.as_str()).collect();
                if unique_dirs.len() == 1 {
                    return Ok(ShipInfo {
                        model_dir: matches[0].2.clone(),
                        display_name: Some(matches[0].0.clone()),
                        param_index: matches[0].1.clone(),
                    });
                }

                let listing: Vec<String> = matches
                    .iter()
                    .map(|(display, idx, dir)| format!("  {display} ({idx}) -> {dir}"))
                    .collect();
                bail!(
                    "Multiple ships match '{name}':\n{}\nPlease refine your search \
                     or use the model directory name directly.",
                    listing.join("\n")
                );
            }
        }
    }

    /// List hull upgrades for a ship.
    pub fn list_hull_upgrades(&self, name: &str) -> Result<Vec<HullUpgradeInfo>, Report> {
        let info = self.find_ship(name)?;
        let vehicle = self.find_vehicle(&info.model_dir)?;

        let Some(upgrades) = vehicle.hull_upgrades() else {
            return Ok(Vec::new());
        };

        let mut result = Vec::new();
        let mut sorted: Vec<_> = upgrades.iter().collect();
        sorted.sort_by_key(|(k, _)| (*k).clone());

        for (upgrade_name, config) in sorted {
            let mut components = Vec::new();
            for &ct in keys::ALL_COMPONENT_TYPES {
                let comp = config.component_name(ct).unwrap_or("(none)").to_string();
                let mount_count = config.mounts(ct).map(|m| m.len()).unwrap_or(0);
                components.push((ct.to_string(), comp, mount_count));
            }
            result.push(HullUpgradeInfo {
                name: upgrade_name.clone(),
                components,
            });
        }

        Ok(result)
    }

    /// List available camouflage texture schemes for a ship.
    pub fn list_texture_schemes(&self, name: &str) -> Result<Vec<String>, Report> {
        let info = self.find_ship(name)?;
        let db = self.db()?;
        let self_id_index = db.build_self_id_index();

        // Collect visuals for the ship model dir.
        let visual_paths = self.find_visual_paths(&db, &self_id_index, &info.model_dir);
        let sub_models = self.load_sub_models(&db, &self_id_index, &visual_paths)?;

        // Also load turret models to include their stems.
        let vehicle = self.find_vehicle(&info.model_dir).ok();
        let mount_points = vehicle
            .and_then(|v| self.select_hull_mount_points(v, None))
            .unwrap_or_default();
        let turret_data = self.load_turret_models(&db, &self_id_index, &mount_points)?;

        let mut all_stems = Vec::new();
        for smd in &sub_models {
            for mfm in collect_mfm_info(&smd.visual, &db) {
                all_stems.push(mfm.stem);
            }
        }
        for tmd in &turret_data {
            for mfm in collect_mfm_info(&tmd.visual, &db) {
                all_stems.push(mfm.stem);
            }
        }

        Ok(texture::discover_texture_schemes(&self.vfs, &all_stems))
    }

    /// Load a complete ship model, ready for export.
    pub fn load_ship(
        &self,
        name: &str,
        options: &ShipExportOptions,
    ) -> Result<ShipModelContext, Report> {
        let info = self.find_ship(name)?;
        let db = self.db()?;
        let self_id_index = db.build_self_id_index();

        // Find all .visual files in the model directory.
        let visual_paths = self.find_visual_paths(&db, &self_id_index, &info.model_dir);
        if visual_paths.is_empty() {
            bail!(
                "No .visual files found for '{}'. Try using the model directory name directly.",
                name
            );
        }

        // Load hull sub-models.
        let hull_parts = self.load_sub_models(&db, &self_id_index, &visual_paths)?;

        // Load turret/mount models from GameParams.
        let vehicle = self.find_vehicle(&info.model_dir).ok();
        let mount_points: Vec<MountPoint> = vehicle
            .and_then(|v| self.select_hull_mount_points(v, options.hull.as_deref()))
            .unwrap_or_default();

        let (turret_models, _turret_model_index, mounts) =
            self.load_mounts(&db, &self_id_index, &mount_points, &hull_parts)?;

        Ok(ShipModelContext {
            vfs: self.vfs.clone(),
            assets_bin_bytes: self.assets_bin_bytes.clone(),
            hull_parts,
            turret_models,
            mounts,
            info,
            options: options.clone(),
        })
    }

    // --- Internal helpers ---

    /// Re-parse the PrototypeDatabase from owned bytes.
    fn db(&self) -> Result<PrototypeDatabase<'_>, Report> {
        Ok(assets_bin::parse_assets_bin(&self.assets_bin_bytes)
            .context("Failed to parse assets.bin")?)
    }

    /// Find a Vehicle by model directory name.
    fn find_vehicle(&self, model_dir: &str) -> Result<&crate::game_params::types::Vehicle, Report> {
        self.metadata
            .params()
            .iter()
            .filter_map(|p| p.vehicle())
            .find(|v| {
                v.model_path()
                    .map(|mp| mp.contains(model_dir))
                    .unwrap_or(false)
            })
            .ok_or_else(|| rootcause::report!("Ship '{}' not found in GameParams", model_dir))
    }

    /// Scan paths_storage for .visual files in a directory matching the name.
    fn find_visual_paths(
        &self,
        db: &PrototypeDatabase<'_>,
        self_id_index: &HashMap<u64, usize>,
        model_dir: &str,
    ) -> Vec<(String, String)> {
        let needle = format!("/{model_dir}/");
        let mut result = Vec::new();

        for (i, entry) in db.paths_storage.iter().enumerate() {
            if !entry.name.ends_with(".visual") {
                continue;
            }
            let full_path = db.reconstruct_path(i, self_id_index);
            if full_path.contains(&needle) {
                let sub_name = entry
                    .name
                    .strip_suffix(".visual")
                    .unwrap_or(&entry.name)
                    .to_string();
                result.push((sub_name, full_path));
            }
        }

        result.sort_by(|a, b| a.0.cmp(&b.0));
        result
    }

    /// Load and parse all sub-models from (name, full_path) pairs.
    fn load_sub_models(
        &self,
        db: &PrototypeDatabase<'_>,
        self_id_index: &HashMap<u64, usize>,
        visual_paths: &[(String, String)],
    ) -> Result<Vec<OwnedSubModel>, Report> {
        let mut result = Vec::new();

        for (sub_name, _) in visual_paths {
            let visual_suffix = format!("{sub_name}.visual");
            let (vis_location, _) = db
                .resolve_path(&visual_suffix, self_id_index)
                .context_with(|| format!("Could not resolve visual: {visual_suffix}"))?;

            if vis_location.blob_index != 1 {
                eprintln!(
                    "Warning: '{visual_suffix}' resolved to blob {} (not VisualPrototype), skipping",
                    vis_location.blob_index
                );
                continue;
            }

            let vis_data = db
                .get_prototype_data(vis_location, visual::VISUAL_ITEM_SIZE)
                .context("Failed to get visual prototype data")?;
            let vp = visual::parse_visual(vis_data).context("Failed to parse VisualPrototype")?;

            let geom_path_idx =
                self_id_index
                    .get(&vp.merged_geometry_path_id)
                    .ok_or_else(|| {
                        rootcause::report!(
                            "Could not resolve mergedGeometryPathId 0x{:016X} for {}",
                            vp.merged_geometry_path_id,
                            sub_name
                        )
                    })?;
            let geom_full_path = db.reconstruct_path(*geom_path_idx, self_id_index);

            let mut geom_bytes = Vec::new();
            self.vfs
                .join(&geom_full_path)
                .context("VFS path error")?
                .open_file()
                .context_with(|| format!("Could not open geometry: {geom_full_path}"))?
                .read_to_end(&mut geom_bytes)?;

            result.push(OwnedSubModel {
                name: sub_name.clone(),
                visual: vp,
                geom_bytes,
            });
        }

        Ok(result)
    }

    /// Select mount points for the chosen hull upgrade.
    fn select_hull_mount_points(
        &self,
        vehicle: &crate::game_params::types::Vehicle,
        hull_selection: Option<&str>,
    ) -> Option<Vec<MountPoint>> {
        let upgrades = vehicle.hull_upgrades()?;
        let mut sorted: Vec<_> = upgrades.iter().collect();
        sorted.sort_by_key(|(k, _)| (*k).clone());

        let selected = if let Some(sel) = hull_selection {
            sorted
                .iter()
                .find(|(name, _)| *name == sel || name.to_lowercase().contains(&sel.to_lowercase()))
                .or_else(|| {
                    let prefix = format!("{sel}_");
                    sorted.iter().find(|(_, config)| {
                        config
                            .component_name(keys::COMP_HULL)
                            .map(|n| n.starts_with(&prefix))
                            .unwrap_or(false)
                    })
                })
                .copied()
        } else {
            sorted.first().copied()
        };

        selected.map(|(_, config)| config.all_mount_points().cloned().collect())
    }

    /// Load turret models and build mount resolution data.
    fn load_mounts(
        &self,
        db: &PrototypeDatabase<'_>,
        self_id_index: &HashMap<u64, usize>,
        mount_points: &[MountPoint],
        hull_parts: &[OwnedSubModel],
    ) -> Result<
        (
            Vec<OwnedSubModel>,
            HashMap<String, usize>,
            Vec<ResolvedMount>,
        ),
        Report,
    > {
        // Collect hardpoint transforms from hull visuals.
        let mut hp_transforms: HashMap<String, [f32; 16]> = HashMap::new();
        for smd in hull_parts {
            for &name_id in &smd.visual.nodes.name_map_name_ids {
                if let Some(name) = db.strings.get_string_by_id(name_id) {
                    if name.starts_with("HP_") {
                        if let Some(xform) = smd.visual.find_hardpoint_transform(name, &db.strings)
                        {
                            hp_transforms.insert(name.to_string(), xform);
                        }
                    }
                }
            }
        }

        // Load unique turret models.
        let (turret_models, turret_model_index) =
            self.load_turret_models_deduped(db, self_id_index, mount_points)?;

        // Build resolved mounts.
        let mut mounts = Vec::new();
        for mi in mount_points {
            let Some(&model_idx) = turret_model_index.get(mi.model_path()) else {
                continue;
            };

            // Skip compound hardpoints.
            let hp_parts: Vec<&str> = mi.hp_name().split("_HP_").collect();
            if hp_parts.len() > 1 {
                continue;
            }

            let transform = hp_transforms.get(mi.hp_name()).copied();
            if transform.is_none() {
                continue;
            }

            // Apply 180° Y rotation for BigWorld → glTF coordinate conversion.
            let transform = transform.map(apply_turret_rotation);

            mounts.push(ResolvedMount {
                hp_name: mi.hp_name().to_string(),
                turret_model_index: model_idx,
                transform,
            });
        }

        Ok((turret_models, turret_model_index, mounts))
    }

    /// Load unique turret models, deduplicating by model path.
    fn load_turret_models_deduped(
        &self,
        db: &PrototypeDatabase<'_>,
        self_id_index: &HashMap<u64, usize>,
        mount_points: &[MountPoint],
    ) -> Result<(Vec<OwnedSubModel>, HashMap<String, usize>), Report> {
        let mut index_map: HashMap<String, usize> = HashMap::new();
        let mut models = Vec::new();

        for mi in mount_points {
            if index_map.contains_key(mi.model_path()) {
                continue;
            }

            match self.load_single_turret(db, self_id_index, mi.model_path()) {
                Ok(smd) => {
                    let idx = models.len();
                    index_map.insert(mi.model_path().to_string(), idx);
                    models.push(smd);
                }
                Err(e) => {
                    eprintln!("Warning: could not load turret '{}': {e}", mi.model_path());
                }
            }
        }

        Ok((models, index_map))
    }

    /// Load turret models (non-deduplicating variant for texture listing).
    fn load_turret_models(
        &self,
        db: &PrototypeDatabase<'_>,
        self_id_index: &HashMap<u64, usize>,
        mount_points: &[MountPoint],
    ) -> Result<Vec<OwnedSubModel>, Report> {
        let (models, _) = self.load_turret_models_deduped(db, self_id_index, mount_points)?;
        Ok(models)
    }

    /// Load a single turret model from its .model path.
    fn load_single_turret(
        &self,
        db: &PrototypeDatabase<'_>,
        self_id_index: &HashMap<u64, usize>,
        model_path: &str,
    ) -> Result<OwnedSubModel, Report> {
        let visual_path = model_path.replace(".model", ".visual");
        let visual_suffix = visual_path
            .rsplit('/')
            .next()
            .unwrap_or(&visual_path)
            .to_string();

        let (vis_location, _) = db
            .resolve_path(&visual_suffix, self_id_index)
            .context_with(|| format!("Could not resolve turret visual: {visual_suffix}"))?;

        if vis_location.blob_index != 1 {
            bail!(
                "Turret visual '{}' resolved to blob {} (expected 1)",
                visual_suffix,
                vis_location.blob_index
            );
        }

        let vis_data = db
            .get_prototype_data(vis_location, visual::VISUAL_ITEM_SIZE)
            .context("Failed to get turret visual data")?;
        let vp = visual::parse_visual(vis_data).context("Failed to parse turret visual")?;

        let geom_path_idx = self_id_index
            .get(&vp.merged_geometry_path_id)
            .ok_or_else(|| {
                rootcause::report!("Could not resolve geometry for turret '{}'", visual_suffix)
            })?;
        let geom_full_path = db.reconstruct_path(*geom_path_idx, self_id_index);

        let mut geom_bytes = Vec::new();
        self.vfs
            .join(&geom_full_path)
            .context("VFS path error")?
            .open_file()
            .context_with(|| format!("Could not open turret geometry: {geom_full_path}"))?
            .read_to_end(&mut geom_bytes)?;

        let model_short_name = model_path
            .rsplit('/')
            .next()
            .unwrap_or(model_path)
            .strip_suffix(".model")
            .unwrap_or(model_path);

        Ok(OwnedSubModel {
            name: model_short_name.to_string(),
            visual: vp,
            geom_bytes,
        })
    }
}

// ---------------------------------------------------------------------------
// ShipModelContext — fully-loaded ship, ready for export
// ---------------------------------------------------------------------------

/// A fully-loaded ship model. Owns all bytes and parsed visuals.
///
/// Created via [`ShipAssets::load_ship()`]. Call [`export_glb()`](Self::export_glb)
/// to write the model to a file or buffer.
pub struct ShipModelContext {
    vfs: VfsPath,
    assets_bin_bytes: Vec<u8>,
    hull_parts: Vec<OwnedSubModel>,
    turret_models: Vec<OwnedSubModel>,
    mounts: Vec<ResolvedMount>,
    info: ShipInfo,
    options: ShipExportOptions,
}

impl ShipModelContext {
    /// Ship identity information.
    pub fn info(&self) -> &ShipInfo {
        &self.info
    }

    /// Hull part names (sub-model names).
    pub fn hull_part_names(&self) -> Vec<&str> {
        self.hull_parts.iter().map(|p| p.name.as_str()).collect()
    }

    /// Number of mounted components (turrets, AA, etc.).
    pub fn mount_count(&self) -> usize {
        self.mounts.len()
    }

    /// Number of unique turret/mount 3D models.
    pub fn unique_turret_count(&self) -> usize {
        self.turret_models.len()
    }

    /// Export the loaded ship model to GLB format.
    pub fn export_glb(&self, writer: &mut impl Write) -> Result<(), Report> {
        let db = assets_bin::parse_assets_bin(&self.assets_bin_bytes)
            .context("Failed to re-parse assets.bin")?;

        // Parse geometries (scoped borrows — no self-referential issue).
        let hull_geoms: Vec<geometry::MergedGeometry<'_>> = self
            .hull_parts
            .iter()
            .map(|d| geometry::parse_geometry(&d.geom_bytes).expect("Failed to parse geometry"))
            .collect();

        let turret_geoms: Vec<geometry::MergedGeometry<'_>> = self
            .turret_models
            .iter()
            .map(|d| {
                geometry::parse_geometry(&d.geom_bytes).expect("Failed to parse turret geometry")
            })
            .collect();

        // Build SubModel list.
        let mut sub_models: Vec<SubModel<'_>> = Vec::new();

        // Hull sub-models.
        for (data, geom) in self.hull_parts.iter().zip(hull_geoms.iter()) {
            sub_models.push(SubModel {
                name: data.name.clone(),
                visual: &data.visual,
                geometry: geom,
                transform: None,
            });
        }

        // Mounted components.
        for mount in &self.mounts {
            let turret_data = &self.turret_models[mount.turret_model_index];
            let turret_geom = &turret_geoms[mount.turret_model_index];

            sub_models.push(SubModel {
                name: format!("{} ({})", mount.hp_name, turret_data.name),
                visual: &turret_data.visual,
                geometry: turret_geom,
                transform: mount.transform,
            });
        }

        // Load textures.
        let texture_set = if self.options.textures {
            let mut all_mfm_infos = Vec::new();
            for sub in &sub_models {
                all_mfm_infos.extend(collect_mfm_info(sub.visual, &db));
            }
            build_texture_set(&all_mfm_infos, &self.vfs)
        } else {
            TextureSet::empty()
        };

        gltf_export::export_ship_glb(
            &sub_models,
            &db,
            self.options.lod,
            &texture_set,
            self.options.damaged,
            writer,
        )
        .context("Failed to export ship GLB")?;

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Internal types
// ---------------------------------------------------------------------------

/// Owns visual + geometry bytes for one sub-model (no lifetime parameters).
struct OwnedSubModel {
    name: String,
    visual: VisualPrototype,
    geom_bytes: Vec<u8>,
}

/// A mount instance with resolved transform.
struct ResolvedMount {
    hp_name: String,
    turret_model_index: usize,
    transform: Option<[f32; 16]>,
}

// ---------------------------------------------------------------------------
// Shared helpers (pub so main.rs export-model can use them too)
// ---------------------------------------------------------------------------

/// Resolved MFM info: stem (leaf name without `.mfm`) and full VFS path.
pub struct MfmInfo {
    pub stem: String,
    pub full_path: String,
}

/// Collect MFM stems and full paths from a visual's render sets.
pub fn collect_mfm_info(visual: &VisualPrototype, db: &PrototypeDatabase<'_>) -> Vec<MfmInfo> {
    let self_id_index = db.build_self_id_index();
    let mut result = Vec::new();
    let mut seen = HashSet::new();

    for rs in &visual.render_sets {
        if rs.material_mfm_path_id == 0 {
            continue;
        }
        let Some(&path_idx) = self_id_index.get(&rs.material_mfm_path_id) else {
            continue;
        };
        let mfm_name = &db.paths_storage[path_idx].name;
        let stem = mfm_name.strip_suffix(".mfm").unwrap_or(mfm_name);

        if seen.insert(stem.to_string()) {
            let full_path = db.reconstruct_path(path_idx, &self_id_index);
            result.push(MfmInfo {
                stem: stem.to_string(),
                full_path,
            });
        }
    }

    result
}

/// Build a `TextureSet` from MFM infos: base albedo + all camo schemes.
pub fn build_texture_set(mfm_infos: &[MfmInfo], vfs: &VfsPath) -> TextureSet {
    let mut base = HashMap::new();

    let mut seen_stems = HashSet::new();
    let mut unique_infos: Vec<&MfmInfo> = Vec::new();
    for info in mfm_infos {
        if seen_stems.insert(info.stem.clone()) {
            unique_infos.push(info);
        }
    }

    // Load base albedo textures.
    for info in &unique_infos {
        if let Some(dds_bytes) = texture::load_base_albedo_bytes(vfs, &info.full_path) {
            match texture::dds_to_png(&dds_bytes) {
                Ok(png_bytes) => {
                    base.insert(info.stem.clone(), png_bytes);
                }
                Err(e) => {
                    eprintln!(
                        "  Warning: failed to decode base texture for {}: {e}",
                        info.stem
                    );
                }
            }
        }
    }

    // Discover camo schemes.
    let stems: Vec<String> = unique_infos.iter().map(|i| i.stem.clone()).collect();
    let schemes = texture::discover_texture_schemes(vfs, &stems);

    let mut camo_schemes = Vec::new();
    for scheme in &schemes {
        let mut scheme_textures = HashMap::new();
        for info in &unique_infos {
            if let Some((_base_name, dds_bytes)) =
                texture::load_texture_bytes(vfs, &info.stem, scheme)
            {
                match texture::dds_to_png(&dds_bytes) {
                    Ok(png_bytes) => {
                        scheme_textures.insert(info.stem.clone(), png_bytes);
                    }
                    Err(e) => {
                        eprintln!(
                            "  Warning: failed to decode camo texture {}_{}: {e}",
                            info.stem, scheme
                        );
                    }
                }
            }
        }
        if !scheme_textures.is_empty() {
            camo_schemes.push((scheme.clone(), scheme_textures));
        }
    }

    TextureSet { base, camo_schemes }
}

/// Apply 180° Y rotation to a column-major 4×4 transform matrix.
///
/// Turret meshes are authored with barrels pointing +Z (BigWorld forward).
/// In glTF's right-handed coordinate system +Z points backward, so we
/// post-multiply by Ry(180°) which negates columns 0 and 2.
fn apply_turret_rotation(mut m: [f32; 16]) -> [f32; 16] {
    // Negate column 0 (indices 0..3)
    m[0] = -m[0];
    m[1] = -m[1];
    m[2] = -m[2];
    m[3] = -m[3];
    // Negate column 2 (indices 8..11)
    m[8] = -m[8];
    m[9] = -m[9];
    m[10] = -m[10];
    m[11] = -m[11];
    m
}

// ---------------------------------------------------------------------------
// Convenience function
// ---------------------------------------------------------------------------

/// One-shot: load game data, resolve ship, export GLB.
///
/// For multiple ships, use [`ShipAssets`] directly to amortize the ~18s
/// GameParams parsing cost.
pub fn export_ship_glb(
    vfs: &VfsPath,
    name: &str,
    options: &ShipExportOptions,
    writer: &mut impl Write,
) -> Result<ShipInfo, Report> {
    let assets = ShipAssets::load(vfs)?;
    let ctx = assets.load_ship(name, options)?;
    let info = ctx.info().clone();
    ctx.export_glb(writer)?;
    Ok(info)
}
