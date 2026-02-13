use std::collections::HashMap;

use derive_builder::Builder;
use strum_macros::{EnumString, IntoStaticStr};
use variantly::Variantly;

use crate::{Rc, data::ResourceLoader};

use super::provider::GameMetadataProvider;

#[derive(
    EnumString, Clone, Debug, Variantly, IntoStaticStr, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
pub enum Species {
    AAircraft,
    AbilitiesUnit,
    AirBase,
    AirCarrier,
    Airship,
    AntiAircraft,
    Artillery,
    ArtilleryUnit,
    Auxiliary,
    Battleship,
    Bomb,
    Bomber,
    BuildingType,
    Camoboost,
    Camouflage,
    Campaign,
    CoastalArtillery,
    CollectionAlbum,
    CollectionCard,
    Complex,
    Cruiser,
    DCharge,
    DeathSettings,
    DepthCharge,
    Destroyer,
    Dive,
    DiveBomberTypeUnit,
    DogTagDoll,
    DogTagItem,
    DogTagSlotsScheme,
    DogTagUnique,
    Drop,
    DropVisual,
    EngineUnit,
    Ensign,
    Event,
    Fake,
    Fighter,
    FighterTypeUnit,
    #[strum(serialize = "Fire control")]
    FireControl,
    Flags,
    FlightControlUnit,
    Generator,
    GlobalWeather,
    Globalboost,
    Hull,
    HullUnit,
    IndividualTask,
    Laser,
    LocalWeather,
    MSkin,
    Main,
    MapBorder,
    Military,
    Mine,
    Mission,
    Modifier,
    Multiboost,
    NewbieQuest,
    Operation,
    Permoflage,
    PlaneTracer,
    PrimaryWeaponsUnit,
    RayTower,
    Rocket,
    Scout,
    Search,
    Secondary,
    SecondaryWeaponsUnit,
    SensorTower,
    Sinking,
    Skin,
    Skip,
    SkipBomb,
    SkipBomberTypeUnit,
    SonarUnit,
    SpaceStation,
    Submarine,
    SuoUnit,
    Task,
    Torpedo,
    TorpedoBomberTypeUnit,
    TorpedoesUnit,
    Upgrade,
    Wave,
    #[strum(serialize = "null")]
    Null,
    Unknown(String),
}

impl Species {
    pub fn translation_id(&self) -> String {
        let name: &'static str = self.into();
        format!("IDS_{name}")
    }
}

#[derive(Builder, Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
pub struct Param {
    id: u32,
    index: String,
    name: String,
    species: Option<Species>,
    nation: String,
    data: ParamData,
}

impl Param {
    pub fn id(&self) -> u32 {
        self.id
    }

    pub fn index(&self) -> &str {
        self.index.as_ref()
    }

    pub fn name(&self) -> &str {
        self.name.as_ref()
    }

    pub fn species(&self) -> Option<Species> {
        self.species.clone()
    }

    pub fn nation(&self) -> &str {
        self.nation.as_ref()
    }

    pub fn data(&self) -> &ParamData {
        &self.data
    }

    /// Returns the Aircraft data if this param is an Aircraft type.
    pub fn aircraft(&self) -> Option<&Aircraft> {
        match &self.data {
            ParamData::Aircraft(a) => Some(a),
            _ => None,
        }
    }

    /// Returns the Vehicle data if this param is a Vehicle (ship) type.
    pub fn vehicle(&self) -> Option<&Vehicle> {
        match &self.data {
            ParamData::Vehicle(v) => Some(v),
            _ => None,
        }
    }

    /// Returns the Ability data if this param is an Ability (consumable) type.
    pub fn ability(&self) -> Option<&Ability> {
        match &self.data {
            ParamData::Ability(a) => Some(a),
            _ => None,
        }
    }

    /// Returns the Projectile data if this param is a Projectile type.
    pub fn projectile(&self) -> Option<&Projectile> {
        match &self.data {
            ParamData::Projectile(p) => Some(p),
            _ => None,
        }
    }
}

#[derive(PartialEq, Eq, EnumString, Hash, Debug, Variantly)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
pub enum ParamType {
    Ability,
    Achievement,
    AdjustmentShotActivator,
    Aircraft,
    BattleScript,
    Building,
    Campaign,
    Catapult,
    ClanSupply,
    Collection,
    Component,
    Crew,
    Director,
    DogTag,
    EventTrigger,
    Exterior,
    Finder,
    Gun,
    Modernization,
    Other,
    Projectile,
    Radar,
    RageModeProgressAction,
    Reward,
    RibbonActivator,
    Sfx,
    Ship,
    SwitchTrigger,
    SwitchVehicleVisualStateAction,
    TimerActivator,
    ToggleTriggerAction,
    Unit,
    VisibilityChangedActivator,
}

// #[derive(Serialize, Deserialize, Clone, Builder, Debug)]
// pub struct VehicleAbility {
//     typ: String,

// }

/// All range data associated with a specific hull upgrade.
///
/// Each hull upgrade in `ShipUpgradeInfo` (ucType = "_Hull") references specific
/// hull, artillery, and ATBA components. This struct captures the resolved range
/// data from those components.
#[derive(Clone, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
pub struct HullUpgradeConfig {
    /// Sea detection range in km.
    pub detection_km: f32,
    /// Air detection range in km.
    pub air_detection_km: f32,
    /// Main battery max range in meters (from the artillery component tied to this hull).
    pub main_battery_m: Option<f32>,
    /// Secondary battery max range in meters (from the ATBA component tied to this hull).
    pub secondary_battery_m: Option<f32>,
}

/// Ship configuration data extracted from GameParams.
///
/// Hull configs are keyed by the hull upgrade's GameParam ID so the renderer
/// can look up the player's equipped hull directly from `ShipConfig::hull()`.
#[derive(Clone, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
pub struct ShipConfigData {
    /// Hull upgrade configs keyed by upgrade GameParam name (e.g. "PAUH442_New_York_1934").
    pub hull_upgrades: HashMap<String, HullUpgradeConfig>,
}


/// Resolved ship range values in real-world units.
/// Detection is in km, all weapon/consumable ranges are in meters.
#[derive(Clone, Debug, Default)]
pub struct ShipRanges {
    /// Sea detection range in km.
    pub detection_km: Option<f32>,
    /// Air detection range in km.
    pub air_detection_km: Option<f32>,
    /// Main battery max range in meters.
    pub main_battery_m: Option<f32>,
    /// Secondary battery max range in meters.
    pub secondary_battery_m: Option<f32>,
    /// Radar detection range in meters (converted from BigWorld units).
    pub radar_m: Option<f32>,
    /// Hydro detection range in meters (converted from BigWorld units).
    pub hydro_m: Option<f32>,
}



#[derive(Clone, Builder, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
pub struct Vehicle {
    level: u32,
    group: String,
    abilities: Option<Vec<Vec<(String, String)>>>,
    #[cfg_attr(feature = "serde", serde(default))]
    upgrades: Vec<String>,
    #[builder(default)]
    #[cfg_attr(feature = "serde", serde(default))]
    config_data: Option<ShipConfigData>,
}

impl Vehicle {
    pub fn level(&self) -> u32 {
        self.level
    }

    pub fn group(&self) -> &str {
        self.group.as_ref()
    }

    pub fn abilities(&self) -> Option<&[Vec<(String, String)>]> {
        self.abilities.as_deref()
    }

    pub fn upgrades(&self) -> &[String] {
        self.upgrades.as_slice()
    }

    pub fn config_data(&self) -> Option<&ShipConfigData> {
        self.config_data.as_ref()
    }

    /// Resolve the ship's ranges for a specific hull upgrade.
    ///
    /// `hull_name` is the GameParam name of the equipped hull upgrade.
    /// Look it up from the hull ID via `GameParamProvider::game_param_by_id()`.
    /// If `None`, the first available hull config is used as a fallback.
    ///
    /// Radar and hydro ranges are looked up from the ship's ability slots via
    /// `game_params`. Pass `None` to skip consumable range resolution.
    pub fn resolve_ranges(
        &self,
        game_params: Option<&dyn GameParamProvider>,
        hull_name: Option<&str>,
    ) -> ShipRanges {
        let mut ranges = ShipRanges::default();

        if let Some(config) = &self.config_data {
            let hull_config = hull_name
                .and_then(|name| config.hull_upgrades.get(name))
                .or_else(|| config.hull_upgrades.values().next());
            if let Some(hc) = hull_config {
                ranges.detection_km = Some(hc.detection_km);
                ranges.air_detection_km = Some(hc.air_detection_km);
                ranges.main_battery_m = hc.main_battery_m;
                ranges.secondary_battery_m = hc.secondary_battery_m;
            }
        }

        // Radar and hydro from consumable abilities
        if let (Some(game_params), Some(abilities)) = (game_params, &self.abilities) {
            for slot in abilities {
                for (ability_name, variant_name) in slot {
                    let param = match game_params.game_param_by_name(ability_name) {
                        Some(p) => p,
                        None => continue,
                    };
                    let ability = match param.ability() {
                        Some(a) => a,
                        None => continue,
                    };
                    let cat = match ability.get_category(variant_name) {
                        Some(c) => c,
                        None => continue,
                    };
                    match cat.consumable_type() {
                        Some(crate::game_types::Consumable::Radar) => {
                            if let Some(dist) = cat.detection_radius() {
                                ranges.radar_m = Some(dist * 30.0 / 2.0);
                            }
                        }
                        Some(crate::game_types::Consumable::HydroacousticSearch) => {
                            if let Some(dist) = cat.detection_radius() {
                                ranges.hydro_m = Some(dist * 30.0 / 2.0);
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        ranges
    }
}

#[derive(Clone, Builder, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
pub struct AbilityCategory {
    special_sound_id: Option<String>,
    consumable_type: String,
    description_id: String,
    group: String,
    icon_id: String,
    num_consumables: isize,
    preparation_time: f32,
    reload_time: f32,
    title_id: String,
    work_time: f32,
    /// Detection radius for ships (radar, hydro, sublocator). BigWorld units (same as world coordinates).
    #[cfg_attr(feature = "serde", serde(default))]
    #[builder(default)]
    dist_ship: Option<f32>,
    /// Detection radius for torpedoes (hydro only). BigWorld units (same as world coordinates).
    #[cfg_attr(feature = "serde", serde(default))]
    #[builder(default)]
    dist_torpedo: Option<f32>,
    /// Hydrophone wave radius in meters (already in world units).
    #[cfg_attr(feature = "serde", serde(default))]
    #[builder(default)]
    hydrophone_wave_radius: Option<f32>,
}

impl AbilityCategory {
    pub fn consumable_type_raw(&self) -> &str {
        &self.consumable_type
    }

    pub fn consumable_type(&self) -> Option<crate::game_types::Consumable> {
        crate::game_types::Consumable::from_consumable_type(&self.consumable_type)
    }

    pub fn icon_id(&self) -> &str {
        &self.icon_id
    }

    pub fn work_time(&self) -> f32 {
        self.work_time
    }

    /// Detection radius in BigWorld units.
    ///
    /// Returns hydrophone_wave_radius if present, otherwise dist_ship directly.
    /// Returns None if this consumable has no detection radius.
    pub fn detection_radius(&self) -> Option<f32> {
        self.hydrophone_wave_radius.or(self.dist_ship)
    }
}

#[derive(Clone, Builder, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
pub struct Ability {
    can_buy: bool,
    cost_credits: isize,
    cost_gold: isize,
    is_free: bool,
    categories: HashMap<String, AbilityCategory>,
}

impl Ability {
    pub fn categories(&self) -> &HashMap<String, AbilityCategory> {
        &self.categories
    }

    pub fn get_category(&self, name: &str) -> Option<&AbilityCategory> {
        self.categories.get(name)
    }
}

#[derive(Clone, Builder, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
pub struct CrewPersonalityShips {
    groups: Vec<String>,
    nation: Vec<String>,
    peculiarity: Vec<String>,
    ships: Vec<String>,
}

#[derive(Clone, Builder, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
pub struct CrewPersonality {
    can_reset_skills_for_free: bool,
    cost_credits: usize,
    cost_elite_xp: usize,
    cost_gold: usize,
    cost_xp: usize,
    has_custom_background: bool,
    has_overlay: bool,
    has_rank: bool,
    has_sample_voiceover: bool,
    is_animated: bool,
    is_person: bool,
    is_retrainable: bool,
    is_unique: bool,
    peculiarity: String,
    /// TODO: flags?
    permissions: u32,
    person_name: String,
    ships: CrewPersonalityShips,
    subnation: String,
    tags: Vec<String>,
}

#[derive(Clone, Builder, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
pub struct ConsumableReloadTimeModifier {
    aircraft_carrier: f32,
    auxiliary: f32,
    battleship: f32,
    cruiser: f32,
    destroyer: f32,
    submarine: f32,
}

impl ConsumableReloadTimeModifier {
    pub fn get_for_species(&self, species: Species) -> f32 {
        match species {
            Species::AirCarrier => self.aircraft_carrier,
            Species::Battleship => self.battleship,
            Species::Cruiser => self.cruiser,
            Species::Destroyer => self.destroyer,
            Species::Submarine => self.submarine,
            Species::Auxiliary => self.auxiliary,
            other => panic!("Unexpected species {other:?}"),
        }
    }

    pub fn aircraft_carrier(&self) -> f32 {
        self.aircraft_carrier
    }

    pub fn auxiliary(&self) -> f32 {
        self.auxiliary
    }

    pub fn battleship(&self) -> f32 {
        self.battleship
    }

    pub fn cruiser(&self) -> f32 {
        self.cruiser
    }

    pub fn destroyer(&self) -> f32 {
        self.destroyer
    }

    pub fn submarine(&self) -> f32 {
        self.submarine
    }
}

#[derive(Clone, Builder, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
pub struct CrewSkillModifier {
    name: String,
    aircraft_carrier: f32,
    auxiliary: f32,
    battleship: f32,
    cruiser: f32,
    destroyer: f32,
    submarine: f32,
}

#[derive(Clone, Builder, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
pub struct CrewSkillLogicTrigger {
    /// Sometimes this field isn't present?
    burn_count: Option<usize>,
    change_priority_target_penalty: f32,
    consumable_type: String,
    cooling_delay: f32,
    /// TODO: figure out type
    cooling_interpolator: Vec<()>,
    divider_type: Option<String>,
    divider_value: Option<f32>,
    duration: f32,
    energy_coeff: f32,
    flood_count: Option<usize>,
    health_factor: Option<f32>,
    /// TODO: figure out type
    heat_interpolator: Vec<()>,
    modifiers: Option<Vec<CrewSkillModifier>>,
    trigger_desc_ids: String,
    trigger_type: String,
}

#[derive(Clone, Builder, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
pub struct CrewSkillTiers {
    aircraft_carrier: usize,
    auxiliary: usize,
    battleship: usize,
    cruiser: usize,
    destroyer: usize,
    submarine: usize,
}

impl CrewSkillTiers {
    pub fn get_for_species(&self, species: Species) -> usize {
        match species {
            Species::AirCarrier => self.aircraft_carrier,
            Species::Battleship => self.battleship,
            Species::Cruiser => self.cruiser,
            Species::Destroyer => self.destroyer,
            Species::Submarine => self.submarine,
            Species::Auxiliary => self.auxiliary,
            other => panic!("Unexpected species {other:?}"),
        }
    }

    pub fn aircraft_carrier(&self) -> usize {
        self.aircraft_carrier
    }

    pub fn auxiliary(&self) -> usize {
        self.auxiliary
    }

    pub fn battleship(&self) -> usize {
        self.battleship
    }

    pub fn cruiser(&self) -> usize {
        self.cruiser
    }

    pub fn destroyer(&self) -> usize {
        self.destroyer
    }

    pub fn submarine(&self) -> usize {
        self.submarine
    }
}

#[derive(Clone, Builder, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
pub struct CrewSkill {
    internal_name: String,
    logic_trigger: Option<CrewSkillLogicTrigger>,
    can_be_learned: bool,
    is_epic: bool,
    modifiers: Option<Vec<CrewSkillModifier>>,
    skill_type: usize,
    tier: CrewSkillTiers,
    ui_treat_as_trigger: bool,
}

impl CrewSkill {
    pub fn internal_name(&self) -> &str {
        self.internal_name.as_ref()
    }

    pub fn translated_name(&self, metadata_provider: &GameMetadataProvider) -> Option<String> {
        use convert_case::{Case, Casing};
        let translation_id = format!(
            "IDS_SKILL_{}",
            self.internal_name().to_case(Case::UpperSnake)
        );

        metadata_provider.localized_name_from_id(&translation_id)
    }

    pub fn translated_description(
        &self,
        metadata_provider: &GameMetadataProvider,
    ) -> Option<String> {
        use convert_case::{Case, Casing};
        let translation_id = format!(
            "IDS_SKILL_DESC_{}",
            self.internal_name().to_case(Case::UpperSnake)
        );

        let description = metadata_provider.localized_name_from_id(&translation_id);

        description.and_then(|desc| {
            if desc.is_empty() || desc == " " {
                None
            } else {
                Some(desc)
            }
        })
    }

    pub fn logic_trigger(&self) -> Option<&CrewSkillLogicTrigger> {
        self.logic_trigger.as_ref()
    }

    pub fn can_be_learned(&self) -> bool {
        self.can_be_learned
    }

    pub fn is_epic(&self) -> bool {
        self.is_epic
    }

    pub fn modifiers(&self) -> Option<&Vec<CrewSkillModifier>> {
        self.modifiers.as_ref()
    }

    pub fn skill_type(&self) -> usize {
        self.skill_type
    }

    pub fn tier(&self) -> &CrewSkillTiers {
        &self.tier
    }

    pub fn ui_treat_as_trigger(&self) -> bool {
        self.ui_treat_as_trigger
    }
}

#[derive(Clone, Builder, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
pub struct Crew {
    money_training_level: usize,
    personality: CrewPersonality,
    skills: Option<Vec<CrewSkill>>,
}

impl Crew {
    pub fn skill_by_type(&self, typ: u32) -> Option<&CrewSkill> {
        self.skills
            .as_ref()
            .and_then(|skills| skills.iter().find(|skill| skill.skill_type == typ as usize))
    }

    pub fn skills(&self) -> Option<&[CrewSkill]> {
        self.skills.as_deref()
    }
}

#[derive(Clone, Builder, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
pub struct Achievement {
    is_group: bool,
    one_per_battle: bool,
    ui_type: String,
    ui_name: String,
}

impl Achievement {
    pub fn is_group(&self) -> bool {
        self.is_group
    }

    pub fn one_per_battle(&self) -> bool {
        self.one_per_battle
    }

    pub fn ui_type(&self) -> &str {
        &self.ui_type
    }

    pub fn ui_name(&self) -> &str {
        &self.ui_name
    }
}

/// Which icon directory a plane's icon should be loaded from.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
pub enum PlaneCategory {
    /// Catapult fighters, spotter planes
    Consumable,
    /// ASW depth-charge planes, mine-laying planes
    Airsupport,
    /// CV-controlled squadrons (default)
    #[default]
    Controllable,
}

#[derive(Clone, Builder, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
pub struct Aircraft {
    #[cfg_attr(feature = "serde", serde(default))]
    category: PlaneCategory,
    #[cfg_attr(feature = "serde", serde(default))]
    ammo_type: String,
}

impl Aircraft {
    pub fn category(&self) -> &PlaneCategory {
        &self.category
    }

    pub fn ammo_type(&self) -> &str {
        &self.ammo_type
    }
}

#[derive(Clone, Builder, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
pub struct Projectile {
    #[cfg_attr(feature = "serde", serde(default))]
    ammo_type: String,
}

impl Projectile {
    pub fn ammo_type(&self) -> &str {
        &self.ammo_type
    }
}

#[derive(Clone, Debug, Variantly)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
pub enum ParamData {
    Vehicle(Vehicle),
    Crew(Crew),
    Ability(Ability),
    Achievement(Achievement),
    Modernization,
    Exterior,
    Unit,
    Aircraft(Aircraft),
    Projectile(Projectile),
}

pub trait GameParamProvider {
    fn game_param_by_id(&self, id: u32) -> Option<Rc<Param>>;
    fn game_param_by_index(&self, index: &str) -> Option<Rc<Param>>;
    fn game_param_by_name(&self, name: &str) -> Option<Rc<Param>>;
    fn params(&self) -> &[Rc<Param>];
}

#[derive(Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
pub struct GameParams {
    params: Vec<Rc<Param>>,
    #[cfg_attr(feature = "serde", serde(skip))]
    #[cfg_attr(feature = "rkyv", rkyv(with = rkyv::with::Skip))]
    id_to_params: HashMap<u32, Rc<Param>>,
    #[cfg_attr(feature = "serde", serde(skip))]
    #[cfg_attr(feature = "rkyv", rkyv(with = rkyv::with::Skip))]
    index_to_params: HashMap<String, Rc<Param>>,
    #[cfg_attr(feature = "serde", serde(skip))]
    #[cfg_attr(feature = "rkyv", rkyv(with = rkyv::with::Skip))]
    name_to_params: HashMap<String, Rc<Param>>,
}

impl GameParamProvider for GameParams {
    fn game_param_by_id(&self, id: u32) -> Option<Rc<Param>> {
        self.id_to_params.get(&id).cloned()
    }

    fn game_param_by_index(&self, index: &str) -> Option<Rc<Param>> {
        self.index_to_params.get(index).cloned()
    }

    fn game_param_by_name(&self, name: &str) -> Option<Rc<Param>> {
        self.name_to_params.get(name).cloned()
    }

    fn params(&self) -> &[Rc<Param>] {
        self.params.as_slice()
    }
}

fn build_param_lookups(
    params: &[Rc<Param>],
) -> (
    HashMap<u32, Rc<Param>>,
    HashMap<String, Rc<Param>>,
    HashMap<String, Rc<Param>>,
) {
    let mut id_to_params = HashMap::with_capacity(params.len());
    let mut index_to_params = HashMap::with_capacity(params.len());
    let mut name_to_params = HashMap::with_capacity(params.len());
    for param in params {
        id_to_params.insert(param.id, param.clone());
        index_to_params.insert(param.index.clone(), param.clone());
        name_to_params.insert(param.name.clone(), param.clone());
    }

    (id_to_params, index_to_params, name_to_params)
}

impl<I> From<I> for GameParams
where
    I: IntoIterator<Item = Param>,
{
    fn from(value: I) -> Self {
        let params: Vec<Rc<Param>> = value.into_iter().map(Rc::new).collect();
        let (id_to_params, index_to_params, name_to_params) = build_param_lookups(params.as_ref());

        Self {
            params,
            id_to_params,
            index_to_params,
            name_to_params,
        }
    }
}
