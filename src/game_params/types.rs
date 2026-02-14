use std::collections::HashMap;
use std::ops::{Add, Mul, Sub};

use bon::Builder;
use variantly::Variantly;

use crate::{Rc, data::ResourceLoader, game_types::GameParamId};

use super::provider::GameMetadataProvider;

/// Conversion factor: 1 BigWorld unit = 30 meters.
const BW_TO_METERS: f32 = 30.0;

/// Distance in meters.
#[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
pub struct Meters(f32);

/// Distance in BigWorld coordinate units (1 BW unit = 30 meters).
#[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
pub struct BigWorldDistance(f32);

/// Distance in kilometers.
#[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
pub struct Km(f32);

// --- Construction ---

impl From<f32> for Meters {
    fn from(v: f32) -> Self {
        Self(v)
    }
}
impl From<i32> for Meters {
    fn from(v: i32) -> Self {
        Self(v as f32)
    }
}

impl From<f32> for BigWorldDistance {
    fn from(v: f32) -> Self {
        Self(v)
    }
}
impl From<i32> for BigWorldDistance {
    fn from(v: i32) -> Self {
        Self(v as f32)
    }
}

impl From<f32> for Km {
    fn from(v: f32) -> Self {
        Self(v)
    }
}
impl From<i32> for Km {
    fn from(v: i32) -> Self {
        Self(v as f32)
    }
}

// --- Read access and unit conversions ---

impl Meters {
    pub fn value(self) -> f32 {
        self.0
    }
    pub fn to_bigworld(self) -> BigWorldDistance {
        BigWorldDistance(self.0 / BW_TO_METERS)
    }
    pub fn to_km(self) -> Km {
        Km(self.0 / 1000.0)
    }
}

impl BigWorldDistance {
    pub fn value(self) -> f32 {
        self.0
    }
    pub fn to_meters(self) -> Meters {
        Meters(self.0 * BW_TO_METERS)
    }
}

impl Km {
    pub fn value(self) -> f32 {
        self.0
    }
    pub fn to_meters(self) -> Meters {
        Meters(self.0 * 1000.0)
    }
}

// --- Scalar multiplication (dimensionless coefficients) ---

impl Mul<f32> for Meters {
    type Output = Meters;
    fn mul(self, rhs: f32) -> Meters {
        Meters(self.0 * rhs)
    }
}

impl Mul<f32> for BigWorldDistance {
    type Output = BigWorldDistance;
    fn mul(self, rhs: f32) -> BigWorldDistance {
        BigWorldDistance(self.0 * rhs)
    }
}

impl Mul<f32> for Km {
    type Output = Km;
    fn mul(self, rhs: f32) -> Km {
        Km(self.0 * rhs)
    }
}

// --- Same-type arithmetic ---

impl Add for Meters {
    type Output = Meters;
    fn add(self, rhs: Meters) -> Meters {
        Meters(self.0 + rhs.0)
    }
}
impl Sub for Meters {
    type Output = Meters;
    fn sub(self, rhs: Meters) -> Meters {
        Meters(self.0 - rhs.0)
    }
}

impl Add for BigWorldDistance {
    type Output = BigWorldDistance;
    fn add(self, rhs: BigWorldDistance) -> BigWorldDistance {
        BigWorldDistance(self.0 + rhs.0)
    }
}
impl Sub for BigWorldDistance {
    type Output = BigWorldDistance;
    fn sub(self, rhs: BigWorldDistance) -> BigWorldDistance {
        BigWorldDistance(self.0 - rhs.0)
    }
}

impl Add for Km {
    type Output = Km;
    fn add(self, rhs: Km) -> Km {
        Km(self.0 + rhs.0)
    }
}
impl Sub for Km {
    type Output = Km;
    fn sub(self, rhs: Km) -> Km {
        Km(self.0 - rhs.0)
    }
}

// --- Cross-type arithmetic (converts RHS to LHS unit, returns LHS type) ---

impl Add<BigWorldDistance> for Meters {
    type Output = Meters;
    fn add(self, rhs: BigWorldDistance) -> Meters {
        Meters(self.0 + rhs.to_meters().0)
    }
}
impl Sub<BigWorldDistance> for Meters {
    type Output = Meters;
    fn sub(self, rhs: BigWorldDistance) -> Meters {
        Meters(self.0 - rhs.to_meters().0)
    }
}

impl Add<Meters> for BigWorldDistance {
    type Output = BigWorldDistance;
    fn add(self, rhs: Meters) -> BigWorldDistance {
        BigWorldDistance(self.0 + rhs.to_bigworld().0)
    }
}
impl Sub<Meters> for BigWorldDistance {
    type Output = BigWorldDistance;
    fn sub(self, rhs: Meters) -> BigWorldDistance {
        BigWorldDistance(self.0 - rhs.to_bigworld().0)
    }
}

impl Add<Km> for Meters {
    type Output = Meters;
    fn add(self, rhs: Km) -> Meters {
        Meters(self.0 + rhs.to_meters().0)
    }
}
impl Sub<Km> for Meters {
    type Output = Meters;
    fn sub(self, rhs: Km) -> Meters {
        Meters(self.0 - rhs.to_meters().0)
    }
}

impl Add<Meters> for Km {
    type Output = Km;
    fn add(self, rhs: Meters) -> Km {
        Km(self.0 + rhs.to_km().0)
    }
}
impl Sub<Meters> for Km {
    type Output = Km;
    fn sub(self, rhs: Meters) -> Km {
        Km(self.0 - rhs.to_km().0)
    }
}

#[derive(Clone, Copy, Debug, Variantly, PartialEq, Eq, PartialOrd, Ord, Hash)]
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
    Null,
}

impl Species {
    pub fn from_name(name: &str) -> crate::recognized::Recognized<Self> {
        use crate::recognized::Recognized;
        match name {
            "AAircraft" => Recognized::Known(Self::AAircraft),
            "AbilitiesUnit" => Recognized::Known(Self::AbilitiesUnit),
            "AirBase" => Recognized::Known(Self::AirBase),
            "AirCarrier" => Recognized::Known(Self::AirCarrier),
            "Airship" => Recognized::Known(Self::Airship),
            "AntiAircraft" => Recognized::Known(Self::AntiAircraft),
            "Artillery" => Recognized::Known(Self::Artillery),
            "ArtilleryUnit" => Recognized::Known(Self::ArtilleryUnit),
            "Auxiliary" => Recognized::Known(Self::Auxiliary),
            "Battleship" => Recognized::Known(Self::Battleship),
            "Bomb" => Recognized::Known(Self::Bomb),
            "Bomber" => Recognized::Known(Self::Bomber),
            "BuildingType" => Recognized::Known(Self::BuildingType),
            "Camoboost" => Recognized::Known(Self::Camoboost),
            "Camouflage" => Recognized::Known(Self::Camouflage),
            "Campaign" => Recognized::Known(Self::Campaign),
            "CoastalArtillery" => Recognized::Known(Self::CoastalArtillery),
            "CollectionAlbum" => Recognized::Known(Self::CollectionAlbum),
            "CollectionCard" => Recognized::Known(Self::CollectionCard),
            "Complex" => Recognized::Known(Self::Complex),
            "Cruiser" => Recognized::Known(Self::Cruiser),
            "DCharge" => Recognized::Known(Self::DCharge),
            "DeathSettings" => Recognized::Known(Self::DeathSettings),
            "DepthCharge" => Recognized::Known(Self::DepthCharge),
            "Destroyer" => Recognized::Known(Self::Destroyer),
            "Dive" => Recognized::Known(Self::Dive),
            "DiveBomberTypeUnit" => Recognized::Known(Self::DiveBomberTypeUnit),
            "DogTagDoll" => Recognized::Known(Self::DogTagDoll),
            "DogTagItem" => Recognized::Known(Self::DogTagItem),
            "DogTagSlotsScheme" => Recognized::Known(Self::DogTagSlotsScheme),
            "DogTagUnique" => Recognized::Known(Self::DogTagUnique),
            "Drop" => Recognized::Known(Self::Drop),
            "DropVisual" => Recognized::Known(Self::DropVisual),
            "EngineUnit" => Recognized::Known(Self::EngineUnit),
            "Ensign" => Recognized::Known(Self::Ensign),
            "Event" => Recognized::Known(Self::Event),
            "Fake" => Recognized::Known(Self::Fake),
            "Fighter" => Recognized::Known(Self::Fighter),
            "FighterTypeUnit" => Recognized::Known(Self::FighterTypeUnit),
            "Fire control" | "FireControl" => Recognized::Known(Self::FireControl),
            "Flags" => Recognized::Known(Self::Flags),
            "FlightControlUnit" => Recognized::Known(Self::FlightControlUnit),
            "Generator" => Recognized::Known(Self::Generator),
            "GlobalWeather" => Recognized::Known(Self::GlobalWeather),
            "Globalboost" => Recognized::Known(Self::Globalboost),
            "Hull" => Recognized::Known(Self::Hull),
            "HullUnit" => Recognized::Known(Self::HullUnit),
            "IndividualTask" => Recognized::Known(Self::IndividualTask),
            "Laser" => Recognized::Known(Self::Laser),
            "LocalWeather" => Recognized::Known(Self::LocalWeather),
            "MSkin" => Recognized::Known(Self::MSkin),
            "Main" => Recognized::Known(Self::Main),
            "MapBorder" => Recognized::Known(Self::MapBorder),
            "Military" => Recognized::Known(Self::Military),
            "Mine" => Recognized::Known(Self::Mine),
            "Mission" => Recognized::Known(Self::Mission),
            "Modifier" => Recognized::Known(Self::Modifier),
            "Multiboost" => Recognized::Known(Self::Multiboost),
            "NewbieQuest" => Recognized::Known(Self::NewbieQuest),
            "Operation" => Recognized::Known(Self::Operation),
            "Permoflage" => Recognized::Known(Self::Permoflage),
            "PlaneTracer" => Recognized::Known(Self::PlaneTracer),
            "PrimaryWeaponsUnit" => Recognized::Known(Self::PrimaryWeaponsUnit),
            "RayTower" => Recognized::Known(Self::RayTower),
            "Rocket" => Recognized::Known(Self::Rocket),
            "Scout" => Recognized::Known(Self::Scout),
            "Search" => Recognized::Known(Self::Search),
            "Secondary" => Recognized::Known(Self::Secondary),
            "SecondaryWeaponsUnit" => Recognized::Known(Self::SecondaryWeaponsUnit),
            "SensorTower" => Recognized::Known(Self::SensorTower),
            "Sinking" => Recognized::Known(Self::Sinking),
            "Skin" => Recognized::Known(Self::Skin),
            "Skip" => Recognized::Known(Self::Skip),
            "SkipBomb" => Recognized::Known(Self::SkipBomb),
            "SkipBomberTypeUnit" => Recognized::Known(Self::SkipBomberTypeUnit),
            "SonarUnit" => Recognized::Known(Self::SonarUnit),
            "SpaceStation" => Recognized::Known(Self::SpaceStation),
            "Submarine" => Recognized::Known(Self::Submarine),
            "SuoUnit" => Recognized::Known(Self::SuoUnit),
            "Task" => Recognized::Known(Self::Task),
            "Torpedo" => Recognized::Known(Self::Torpedo),
            "TorpedoBomberTypeUnit" => Recognized::Known(Self::TorpedoBomberTypeUnit),
            "TorpedoesUnit" => Recognized::Known(Self::TorpedoesUnit),
            "Upgrade" => Recognized::Known(Self::Upgrade),
            "Wave" => Recognized::Known(Self::Wave),
            "null" => Recognized::Known(Self::Null),
            other => Recognized::Unknown(other.to_string()),
        }
    }

    pub const fn name(&self) -> &'static str {
        match self {
            Self::AAircraft => "AAircraft",
            Self::AbilitiesUnit => "AbilitiesUnit",
            Self::AirBase => "AirBase",
            Self::AirCarrier => "AirCarrier",
            Self::Airship => "Airship",
            Self::AntiAircraft => "AntiAircraft",
            Self::Artillery => "Artillery",
            Self::ArtilleryUnit => "ArtilleryUnit",
            Self::Auxiliary => "Auxiliary",
            Self::Battleship => "Battleship",
            Self::Bomb => "Bomb",
            Self::Bomber => "Bomber",
            Self::BuildingType => "BuildingType",
            Self::Camoboost => "Camoboost",
            Self::Camouflage => "Camouflage",
            Self::Campaign => "Campaign",
            Self::CoastalArtillery => "CoastalArtillery",
            Self::CollectionAlbum => "CollectionAlbum",
            Self::CollectionCard => "CollectionCard",
            Self::Complex => "Complex",
            Self::Cruiser => "Cruiser",
            Self::DCharge => "DCharge",
            Self::DeathSettings => "DeathSettings",
            Self::DepthCharge => "DepthCharge",
            Self::Destroyer => "Destroyer",
            Self::Dive => "Dive",
            Self::DiveBomberTypeUnit => "DiveBomberTypeUnit",
            Self::DogTagDoll => "DogTagDoll",
            Self::DogTagItem => "DogTagItem",
            Self::DogTagSlotsScheme => "DogTagSlotsScheme",
            Self::DogTagUnique => "DogTagUnique",
            Self::Drop => "Drop",
            Self::DropVisual => "DropVisual",
            Self::EngineUnit => "EngineUnit",
            Self::Ensign => "Ensign",
            Self::Event => "Event",
            Self::Fake => "Fake",
            Self::Fighter => "Fighter",
            Self::FighterTypeUnit => "FighterTypeUnit",
            Self::FireControl => "Fire control",
            Self::Flags => "Flags",
            Self::FlightControlUnit => "FlightControlUnit",
            Self::Generator => "Generator",
            Self::GlobalWeather => "GlobalWeather",
            Self::Globalboost => "Globalboost",
            Self::Hull => "Hull",
            Self::HullUnit => "HullUnit",
            Self::IndividualTask => "IndividualTask",
            Self::Laser => "Laser",
            Self::LocalWeather => "LocalWeather",
            Self::MSkin => "MSkin",
            Self::Main => "Main",
            Self::MapBorder => "MapBorder",
            Self::Military => "Military",
            Self::Mine => "Mine",
            Self::Mission => "Mission",
            Self::Modifier => "Modifier",
            Self::Multiboost => "Multiboost",
            Self::NewbieQuest => "NewbieQuest",
            Self::Operation => "Operation",
            Self::Permoflage => "Permoflage",
            Self::PlaneTracer => "PlaneTracer",
            Self::PrimaryWeaponsUnit => "PrimaryWeaponsUnit",
            Self::RayTower => "RayTower",
            Self::Rocket => "Rocket",
            Self::Scout => "Scout",
            Self::Search => "Search",
            Self::Secondary => "Secondary",
            Self::SecondaryWeaponsUnit => "SecondaryWeaponsUnit",
            Self::SensorTower => "SensorTower",
            Self::Sinking => "Sinking",
            Self::Skin => "Skin",
            Self::Skip => "Skip",
            Self::SkipBomb => "SkipBomb",
            Self::SkipBomberTypeUnit => "SkipBomberTypeUnit",
            Self::SonarUnit => "SonarUnit",
            Self::SpaceStation => "SpaceStation",
            Self::Submarine => "Submarine",
            Self::SuoUnit => "SuoUnit",
            Self::Task => "Task",
            Self::Torpedo => "Torpedo",
            Self::TorpedoBomberTypeUnit => "TorpedoBomberTypeUnit",
            Self::TorpedoesUnit => "TorpedoesUnit",
            Self::Upgrade => "Upgrade",
            Self::Wave => "Wave",
            Self::Null => "null",
        }
    }

    pub fn translation_id(&self) -> String {
        format!("IDS_{}", self.name())
    }
}

#[derive(Builder, Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
pub struct Param {
    id: GameParamId,
    index: String,
    name: String,
    species: Option<crate::recognized::Recognized<Species>>,
    nation: String,
    data: ParamData,
}

impl Param {
    pub fn id(&self) -> GameParamId {
        self.id
    }

    pub fn index(&self) -> &str {
        self.index.as_ref()
    }

    pub fn name(&self) -> &str {
        self.name.as_ref()
    }

    pub fn species(&self) -> Option<&crate::recognized::Recognized<Species>> {
        self.species.as_ref()
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

    /// Returns the Crew data if this param is a Crew type.
    pub fn crew(&self) -> Option<&Crew> {
        match &self.data {
            ParamData::Crew(c) => Some(c),
            _ => None,
        }
    }

    /// Returns the Modernization data if this param is a Modernization type.
    pub fn modernization(&self) -> Option<&Modernization> {
        match &self.data {
            ParamData::Modernization(m) => Some(m),
            _ => None,
        }
    }

    /// Returns the Drop data if this param is a Drop type.
    pub fn drop_data(&self) -> Option<&BuffDrop> {
        match &self.data {
            ParamData::Drop(d) => Some(d),
            _ => None,
        }
    }
}

#[derive(PartialEq, Eq, Hash, Debug, Clone, Copy, Variantly)]
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
    Drop,
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

impl ParamType {
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "Ability" => Some(Self::Ability),
            "Achievement" => Some(Self::Achievement),
            "AdjustmentShotActivator" => Some(Self::AdjustmentShotActivator),
            "Aircraft" => Some(Self::Aircraft),
            "BattleScript" => Some(Self::BattleScript),
            "Building" => Some(Self::Building),
            "Campaign" => Some(Self::Campaign),
            "Catapult" => Some(Self::Catapult),
            "ClanSupply" => Some(Self::ClanSupply),
            "Collection" => Some(Self::Collection),
            "Component" => Some(Self::Component),
            "Crew" => Some(Self::Crew),
            "Director" => Some(Self::Director),
            "DogTag" => Some(Self::DogTag),
            "Drop" => Some(Self::Drop),
            "EventTrigger" => Some(Self::EventTrigger),
            "Exterior" => Some(Self::Exterior),
            "Finder" => Some(Self::Finder),
            "Gun" => Some(Self::Gun),
            "Modernization" => Some(Self::Modernization),
            "Other" => Some(Self::Other),
            "Projectile" => Some(Self::Projectile),
            "Radar" => Some(Self::Radar),
            "RageModeProgressAction" => Some(Self::RageModeProgressAction),
            "Reward" => Some(Self::Reward),
            "RibbonActivator" => Some(Self::RibbonActivator),
            "Sfx" => Some(Self::Sfx),
            "Ship" => Some(Self::Ship),
            "SwitchTrigger" => Some(Self::SwitchTrigger),
            "SwitchVehicleVisualStateAction" => Some(Self::SwitchVehicleVisualStateAction),
            "TimerActivator" => Some(Self::TimerActivator),
            "ToggleTriggerAction" => Some(Self::ToggleTriggerAction),
            "Unit" => Some(Self::Unit),
            "VisibilityChangedActivator" => Some(Self::VisibilityChangedActivator),
            _ => None,
        }
    }
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
    pub detection_km: Km,
    /// Air detection range in km.
    pub air_detection_km: Km,
    /// Main battery max range in meters (from the artillery component tied to this hull).
    pub main_battery_m: Option<Meters>,
    /// Secondary battery max range in meters (from the ATBA component tied to this hull).
    pub secondary_battery_m: Option<Meters>,
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
    pub detection_km: Option<Km>,
    /// Air detection range in km.
    pub air_detection_km: Option<Km>,
    /// Main battery max range in meters.
    pub main_battery_m: Option<Meters>,
    /// Secondary battery max range in meters.
    pub secondary_battery_m: Option<Meters>,
    /// Radar detection range in meters.
    pub radar_m: Option<Meters>,
    /// Hydro detection range in meters.
    pub hydro_m: Option<Meters>,
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
        version: crate::data::Version,
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
                    match cat.consumable_type(version).known() {
                        Some(&crate::game_types::Consumable::Radar) => {
                            ranges.radar_m = cat.detection_radius();
                        }
                        Some(&crate::game_types::Consumable::HydroacousticSearch) => {
                            ranges.hydro_m = cat.detection_radius();
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
    /// Detection radius for ships (radar, hydro, sublocator). BigWorld units.
    #[cfg_attr(feature = "serde", serde(default))]
    dist_ship: Option<BigWorldDistance>,
    /// Detection radius for torpedoes (hydro only). BigWorld units.
    #[cfg_attr(feature = "serde", serde(default))]
    dist_torpedo: Option<BigWorldDistance>,
    /// Hydrophone wave radius in meters.
    #[cfg_attr(feature = "serde", serde(default))]
    hydrophone_wave_radius: Option<Meters>,
    /// Fighter patrol radius. BigWorld units.
    #[cfg_attr(feature = "serde", serde(default))]
    patrol_radius: Option<BigWorldDistance>,
}

impl AbilityCategory {
    pub fn consumable_type_raw(&self) -> &str {
        &self.consumable_type
    }

    pub fn consumable_type(
        &self,
        version: crate::data::Version,
    ) -> crate::recognized::Recognized<crate::game_types::Consumable> {
        crate::game_types::Consumable::from_consumable_type(&self.consumable_type, version)
    }

    pub fn icon_id(&self) -> &str {
        &self.icon_id
    }

    pub fn work_time(&self) -> f32 {
        self.work_time
    }

    /// Detection radius in meters.
    ///
    /// Returns hydrophone_wave_radius if present, otherwise converts dist_ship
    /// from BigWorld units to meters. Returns None if this consumable has no
    /// detection radius.
    pub fn detection_radius(&self) -> Option<Meters> {
        self.hydrophone_wave_radius
            .or_else(|| self.dist_ship.map(|d| d.to_meters()))
    }

    /// Torpedo detection radius in meters.
    pub fn torpedo_detection_radius(&self) -> Option<Meters> {
        self.dist_torpedo.map(|d| d.to_meters())
    }

    /// Fighter patrol radius in meters.
    pub fn patrol_radius(&self) -> Option<Meters> {
        self.patrol_radius.map(|d| d.to_meters())
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

impl CrewSkillModifier {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn get_for_species(&self, species: &Species) -> f32 {
        match species {
            Species::AirCarrier => self.aircraft_carrier,
            Species::Battleship => self.battleship,
            Species::Cruiser => self.cruiser,
            Species::Destroyer => self.destroyer,
            Species::Submarine => self.submarine,
            Species::Auxiliary => self.auxiliary,
            _ => 1.0,
        }
    }
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

#[derive(Clone, Debug, Builder)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
pub struct BuffDrop {
    #[cfg_attr(feature = "serde", serde(default))]
    marker_name_active: String,
    #[cfg_attr(feature = "serde", serde(default))]
    marker_name_inactive: String,
    #[cfg_attr(feature = "serde", serde(default))]
    sorting: i64,
}

impl BuffDrop {
    pub fn marker_name_active(&self) -> &str {
        &self.marker_name_active
    }

    pub fn marker_name_inactive(&self) -> &str {
        &self.marker_name_inactive
    }

    pub fn sorting(&self) -> i64 {
        self.sorting
    }

    /// Returns the game asset path for the active icon.
    pub fn active_icon_path(&self) -> String {
        format!(
            "gui/powerups/drops/icon_marker_{}.png",
            self.marker_name_active
        )
    }

    /// Returns the game asset path for the inactive icon.
    pub fn inactive_icon_path(&self) -> String {
        format!(
            "gui/powerups/drops/icon_marker_{}.png",
            self.marker_name_inactive
        )
    }
}

#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
pub struct Modernization {
    modifiers: Vec<CrewSkillModifier>,
}

impl Modernization {
    pub fn new(modifiers: Vec<CrewSkillModifier>) -> Self {
        Self { modifiers }
    }

    pub fn modifiers(&self) -> &[CrewSkillModifier] {
        &self.modifiers
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
    Modernization(Modernization),
    Exterior,
    Unit,
    Aircraft(Aircraft),
    Projectile(Projectile),
    Drop(BuffDrop),
}

pub trait GameParamProvider {
    fn game_param_by_id(&self, id: GameParamId) -> Option<Rc<Param>>;
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
    id_to_params: HashMap<GameParamId, Rc<Param>>,
    #[cfg_attr(feature = "serde", serde(skip))]
    #[cfg_attr(feature = "rkyv", rkyv(with = rkyv::with::Skip))]
    index_to_params: HashMap<String, Rc<Param>>,
    #[cfg_attr(feature = "serde", serde(skip))]
    #[cfg_attr(feature = "rkyv", rkyv(with = rkyv::with::Skip))]
    name_to_params: HashMap<String, Rc<Param>>,
}

impl GameParamProvider for GameParams {
    fn game_param_by_id(&self, id: GameParamId) -> Option<Rc<Param>> {
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
    HashMap<GameParamId, Rc<Param>>,
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
