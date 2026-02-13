//! Game concept types that describe World of Warships mechanics.
//!
//! These types represent game entities, identifiers, positions, and enumerations
//! that are useful across any tool working with WoWS data -- not just replay parsers.

use std::fmt;

// =============================================================================
// Identity Types
// =============================================================================

/// Per-replay-session entity identifier for game objects (ships, buildings, smoke screens).
/// The wire format is u32 but some packet types use i32 or i64.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
pub struct EntityId(u32);

impl EntityId {
    pub fn raw(self) -> u32 {
        self.0
    }
}

impl fmt::Display for EntityId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<u32> for EntityId {
    fn from(v: u32) -> Self {
        EntityId(v)
    }
}

impl From<i32> for EntityId {
    fn from(v: i32) -> Self {
        EntityId(v as u32)
    }
}

impl From<i64> for EntityId {
    fn from(v: i64) -> Self {
        EntityId(v as u32)
    }
}

/// A persistent player account identifier (db_id, avatar_id).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
pub struct AccountId(pub i64);

impl AccountId {
    pub fn raw(self) -> i64 {
        self.0
    }
}

impl fmt::Display for AccountId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<u32> for AccountId {
    fn from(v: u32) -> Self {
        AccountId(v as i64)
    }
}

impl From<i32> for AccountId {
    fn from(v: i32) -> Self {
        AccountId(v as i64)
    }
}

impl From<i64> for AccountId {
    fn from(v: i64) -> Self {
        AccountId(v)
    }
}

/// A game parameter type identifier from GameParams (ships, equipment, etc.).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
pub struct GameParamId(u64);

impl GameParamId {
    pub fn raw(self) -> u64 {
        self.0
    }
}

impl fmt::Display for GameParamId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<u32> for GameParamId {
    fn from(v: u32) -> Self {
        GameParamId(v as u64)
    }
}

impl From<u64> for GameParamId {
    fn from(v: u64) -> Self {
        GameParamId(v)
    }
}

impl From<i64> for GameParamId {
    fn from(v: i64) -> Self {
        GameParamId(v as u64)
    }
}

/// Represents the relation of a player/entity to the recording player.
/// Corresponds to `PLAYER_RELATION` in battle.xml:
/// - 0 = SELF (the player who recorded the replay)
/// - 1 = ALLY (teammate)
/// - 2 = ENEMY
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
pub struct Relation(u32);

impl Relation {
    pub fn new(value: u32) -> Self {
        Self(value)
    }

    pub fn is_self(&self) -> bool {
        self.0 == 0
    }

    pub fn is_ally(&self) -> bool {
        self.0 == 1
    }

    pub fn is_enemy(&self) -> bool {
        self.0 >= 2
    }

    pub fn name(&self) -> &'static str {
        match self.0 {
            0 => "Self",
            1 => "Ally",
            2 => "Enemy",
            _ => "Unknown",
        }
    }

    pub fn value(&self) -> u32 {
        self.0
    }
}

impl fmt::Display for Relation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name())
    }
}

impl From<u32> for Relation {
    fn from(value: u32) -> Self {
        Self(value)
    }
}

/// Packed minimap squadron identifier.
/// Encodes `(avatar_id: u32, index: u3, purpose: u3, departures: u1)` in the low 39 bits.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
pub struct PlaneId(u64);

impl PlaneId {
    pub fn owner_id(self) -> EntityId {
        EntityId((self.0 & 0xFFFF_FFFF) as u32)
    }

    pub fn raw(self) -> u64 {
        self.0
    }
}

impl fmt::Display for PlaneId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<u64> for PlaneId {
    fn from(v: u64) -> Self {
        PlaneId(v)
    }
}

impl From<i64> for PlaneId {
    fn from(v: i64) -> Self {
        PlaneId(v as u64)
    }
}

// =============================================================================
// Position Types
// =============================================================================

/// World-space position in BigWorld coordinates.
/// X = east/west, Y = up/down (altitude), Z = north/south. Origin at map center.
#[derive(Debug, Clone, Copy, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
pub struct WorldPos {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl WorldPos {
    pub fn lerp(self, other: WorldPos, t: f32) -> WorldPos {
        self + (other - self) * t
    }
}

impl std::ops::Add for WorldPos {
    type Output = WorldPos;
    fn add(self, rhs: WorldPos) -> WorldPos {
        WorldPos {
            x: self.x + rhs.x,
            y: self.y + rhs.y,
            z: self.z + rhs.z,
        }
    }
}

impl std::ops::Sub for WorldPos {
    type Output = WorldPos;
    fn sub(self, rhs: WorldPos) -> WorldPos {
        WorldPos {
            x: self.x - rhs.x,
            y: self.y - rhs.y,
            z: self.z - rhs.z,
        }
    }
}

impl std::ops::Mul<f32> for WorldPos {
    type Output = WorldPos;
    fn mul(self, rhs: f32) -> WorldPos {
        WorldPos {
            x: self.x * rhs,
            y: self.y * rhs,
            z: self.z * rhs,
        }
    }
}

/// Normalized minimap position from MinimapUpdate packets.
/// Values roughly in [-0.5, 1.5] range (centered around [0,1]).
#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
pub struct NormalizedPos {
    pub x: f32,
    pub y: f32,
}

// =============================================================================
// Time Types
// =============================================================================

/// A game clock value in seconds since the replay started recording.
/// Note: there is typically a ~30s pre-game countdown, so game_time = clock - 30.
#[derive(Debug, Clone, Copy, Default, PartialEq, PartialOrd)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
pub struct GameClock(pub f32);

impl GameClock {
    pub fn seconds(self) -> f32 {
        self.0
    }

    pub fn to_duration(self) -> std::time::Duration {
        std::time::Duration::from_secs_f32(self.0)
    }

    pub fn game_time(self) -> f32 {
        (self.0 - 30.0).max(0.0)
    }
}

impl fmt::Display for GameClock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:.1}s", self.0)
    }
}

impl std::ops::Add<f32> for GameClock {
    type Output = GameClock;
    fn add(self, rhs: f32) -> GameClock {
        GameClock(self.0 + rhs)
    }
}

impl std::ops::Add<std::time::Duration> for GameClock {
    type Output = GameClock;
    fn add(self, rhs: std::time::Duration) -> GameClock {
        GameClock(self.0 + rhs.as_secs_f32())
    }
}

impl std::ops::Sub for GameClock {
    type Output = f32;
    fn sub(self, rhs: GameClock) -> f32 {
        self.0 - rhs.0
    }
}

impl std::ops::Sub<std::time::Duration> for GameClock {
    type Output = GameClock;
    fn sub(self, rhs: std::time::Duration) -> GameClock {
        GameClock(self.0 - rhs.as_secs_f32())
    }
}

// =============================================================================
// Game Event Enums
// =============================================================================

/// Voice line commands sent by players via quick-chat.
#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
pub enum VoiceLine {
    IntelRequired,
    FairWinds,
    Wilco,
    Negative,
    WellDone,
    Curses,
    UsingRadar,
    UsingHydroSearch,
    DefendTheBase,
    SetSmokeScreen,
    FollowMe,
    MapPointAttention(f32, f32),
    UsingSubmarineLocator,
    ProvideAntiAircraft,
    RequestingSupport(Option<u32>),
    Retreat(Option<i32>),
    AttentionToSquare(u32, u32),
    Unknown(i64),
    QuickTactic(u16, u64),
}

/// Enumerates the ribbons which appear in the top-right.
#[derive(Debug, PartialEq, Eq, Hash, Clone, Copy)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
pub enum Ribbon {
    PlaneShotDown,
    Incapacitation,
    SetFire,
    Citadel,
    SecondaryHit,
    OverPenetration,
    Penetration,
    NonPenetration,
    Ricochet,
    TorpedoProtectionHit,
    Captured,
    AssistedInCapture,
    Spotted,
    Destroyed,
    TorpedoHit,
    Defended,
    Flooding,
    DiveBombPenetration,
    RocketPenetration,
    RocketNonPenetration,
    RocketTorpedoProtectionHit,
    DepthChargeHit,
    ShotDownByAircraft,
    BuffSeized,
    SonarOneHit,
    SonarTwoHits,
    SonarNeutralized,
    Unknown(i8),
}

/// Cause of a ship's destruction.
#[derive(Debug, PartialEq, Eq, Hash, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
pub enum DeathCause {
    None,
    Artillery,
    Secondaries,
    Torpedo,
    DiveBomber,
    AerialTorpedo,
    Fire,
    Ramming,
    Terrain,
    Flooding,
    Mirror,
    SeaMine,
    Special,
    DepthCharge,
    AerialRocket,
    Detonation,
    Health,
    ApShell,
    HeShell,
    CsShell,
    Fel,
    Portal,
    SkipBombs,
    SectorWave,
    Acid,
    Laser,
    Match,
    Timer,
    AerialDepthCharge,
    Event1,
    Event2,
    Event3,
    Event4,
    Event5,
    Event6,
    Missile,
    Unknown(u32),
    UnknownName(String),
}

impl DeathCause {
    pub fn from_name(name: &str) -> Self {
        match name {
            "NONE" => DeathCause::None,
            "ARTILLERY" => DeathCause::Artillery,
            "ATBA" => DeathCause::Secondaries,
            "TORPEDO" => DeathCause::Torpedo,
            "BOMB" => DeathCause::DiveBomber,
            "TBOMB" => DeathCause::AerialTorpedo,
            "BURNING" => DeathCause::Fire,
            "RAM" => DeathCause::Ramming,
            "TERRAIN" => DeathCause::Terrain,
            "FLOOD" => DeathCause::Flooding,
            "MIRROR" => DeathCause::Mirror,
            "SEA_MINE" => DeathCause::SeaMine,
            "SPECIAL" => DeathCause::Special,
            "DBOMB" => DeathCause::DepthCharge,
            "ROCKET" => DeathCause::AerialRocket,
            "DETONATE" => DeathCause::Detonation,
            "HEALTH" => DeathCause::Health,
            "AP_SHELL" => DeathCause::ApShell,
            "HE_SHELL" => DeathCause::HeShell,
            "CS_SHELL" => DeathCause::CsShell,
            "FEL" => DeathCause::Fel,
            "PORTAL" => DeathCause::Portal,
            "SKIP_BOMB" => DeathCause::SkipBombs,
            "SECTOR_WAVE" => DeathCause::SectorWave,
            "ACID" => DeathCause::Acid,
            "LASER" => DeathCause::Laser,
            "MATCH" => DeathCause::Match,
            "TIMER" => DeathCause::Timer,
            "ADBOMB" => DeathCause::AerialDepthCharge,
            "EVENT_1" => DeathCause::Event1,
            "EVENT_2" => DeathCause::Event2,
            "EVENT_3" => DeathCause::Event3,
            "EVENT_4" => DeathCause::Event4,
            "EVENT_5" => DeathCause::Event5,
            "EVENT_6" => DeathCause::Event6,
            "MISSILE" => DeathCause::Missile,
            other => DeathCause::UnknownName(other.to_string()),
        }
    }

    pub fn icon_name(&self) -> Option<&'static str> {
        match self {
            DeathCause::Artillery => Some("icon_frag_main_caliber"),
            DeathCause::Secondaries => Some("icon_frag_atba"),
            DeathCause::Torpedo => Some("icon_frag_torpedo"),
            DeathCause::DiveBomber => Some("icon_frag_bomb"),
            DeathCause::AerialTorpedo => Some("icon_frag_torpedo"),
            DeathCause::Fire => Some("icon_frag_burning"),
            DeathCause::Ramming => Some("icon_frag_ram"),
            DeathCause::Flooding => Some("icon_frag_flood"),
            DeathCause::SeaMine => Some("icon_frag_naval_mine"),
            DeathCause::DepthCharge => Some("icon_frag_depthbomb"),
            DeathCause::AerialRocket => Some("icon_frag_rocket"),
            DeathCause::Detonation => Some("icon_frag_detonate"),
            DeathCause::ApShell => Some("icon_frag_main_caliber"),
            DeathCause::HeShell => Some("icon_frag_main_caliber"),
            DeathCause::CsShell => Some("icon_frag_main_caliber"),
            DeathCause::Fel => Some("icon_frag_fel"),
            DeathCause::Portal => Some("icon_frag_portal"),
            DeathCause::SkipBombs => Some("icon_frag_skip"),
            DeathCause::SectorWave => Some("icon_frag_wave"),
            DeathCause::Acid => Some("icon_frag_acid"),
            DeathCause::Laser => Some("icon_frag_laser"),
            DeathCause::Match => Some("icon_frag_octagon"),
            DeathCause::Timer => Some("icon_timer"),
            DeathCause::AerialDepthCharge => Some("icon_frag_depthbomb"),
            DeathCause::Event1 => Some("icon_frag_fel"),
            DeathCause::Event2 => Some("icon_frag_fel"),
            DeathCause::Event3 => Some("icon_frag_fel"),
            DeathCause::Event4 => Some("icon_frag_fel"),
            DeathCause::Event5 => Some("icon_frag_fel"),
            DeathCause::Event6 => Some("icon_frag_torpedo"),
            _ => Option::None,
        }
    }
}

/// Consumable ability type, mapped from `consumableType` in GameParams.
#[derive(Debug, PartialEq, Eq, Hash, Clone, Copy)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
pub enum Consumable {
    DamageControl,
    SpottingAircraft,
    DefensiveAntiAircraft,
    SpeedBoost,
    MainBatteryReloadBooster,
    Smoke,
    RepairParty,
    CatapultFighter,
    HydroacousticSearch,
    TorpedoReloadBooster,
    Radar,
    Invulnerable,
    HealForsage,
    CallFighters,
    RegenerateHealth,
    DepthCharges,
    WeaponReloadBooster,
    Hydrophone,
    EnhancedRudders,
    ReserveBattery,
    SubmarineSurveillance,
    Unknown(i8),
}

impl Consumable {
    pub fn from_consumable_type(s: &str) -> Option<Self> {
        match s {
            "crashCrew" => Some(Self::DamageControl),
            "scout" => Some(Self::SpottingAircraft),
            "airDefenseDisp" => Some(Self::DefensiveAntiAircraft),
            "speedBoosters" => Some(Self::SpeedBoost),
            "artilleryBoosters" => Some(Self::MainBatteryReloadBooster),
            "smokeGenerator" => Some(Self::Smoke),
            "regenCrew" => Some(Self::RepairParty),
            "fighter" => Some(Self::CatapultFighter),
            "sonar" => Some(Self::HydroacousticSearch),
            "torpedoReloader" => Some(Self::TorpedoReloadBooster),
            "rls" => Some(Self::Radar),
            "invulnerable" => Some(Self::Invulnerable),
            "healForsage" => Some(Self::HealForsage),
            "callFighters" => Some(Self::CallFighters),
            "regenerateHealth" => Some(Self::RegenerateHealth),
            "depthCharges" => Some(Self::DepthCharges),
            "weaponReloadBooster" => Some(Self::WeaponReloadBooster),
            "hydrophone" => Some(Self::Hydrophone),
            "fastRudders" => Some(Self::EnhancedRudders),
            "subsEnergyFreeze" => Some(Self::ReserveBattery),
            "submarineLocator" => Some(Self::SubmarineSurveillance),
            _ => None,
        }
    }
}

/// Camera view mode, from `CAMERA_MODES` in game constants.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
pub enum CameraMode {
    Airplanes,
    Dock,
    OverheadMap,
    DevFree,
    FollowingShells,
    FollowingPlanes,
    DockModule,
    FollowingShip,
    FreeFlying,
    ReplayFpc,
    FollowingSubmarine,
    TacticalConsumables,
    RespawnMap,
    DockFlags,
    DockEnsign,
    DockLootbox,
    DockNavalFlag,
    IdleGame,
    Unknown(u32),
    UnknownName(String),
}

impl CameraMode {
    pub fn from_name(name: &str) -> Self {
        match name {
            "AIRPLANES" => CameraMode::Airplanes,
            "DOCK" => CameraMode::Dock,
            "TACTICALMAP" => CameraMode::OverheadMap,
            "DEVFREE" => CameraMode::DevFree,
            "SHELLTRACKER" => CameraMode::FollowingShells,
            "PLANETRACKER" => CameraMode::FollowingPlanes,
            "DOCKMODULE" => CameraMode::DockModule,
            "SNAKETAIL" => CameraMode::FollowingShip,
            "SPECTATOR" => CameraMode::FreeFlying,
            "REPLAY_FPC" => CameraMode::ReplayFpc,
            "UNDERWATER" => CameraMode::FollowingSubmarine,
            "TACTICAL_CONSUMABLES" => CameraMode::TacticalConsumables,
            "RESPAWN_MAP" => CameraMode::RespawnMap,
            "DOCKFLAGS" => CameraMode::DockFlags,
            "DOCKENSIGN" => CameraMode::DockEnsign,
            "DOCKLOOTBOX" => CameraMode::DockLootbox,
            "DOCKNAVALFLAG" => CameraMode::DockNavalFlag,
            "IDLEGAME" => CameraMode::IdleGame,
            other => CameraMode::UnknownName(other.to_string()),
        }
    }
}

/// How the battle ended, from `FINISH_TYPE` in battle.xml.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
pub enum FinishType {
    Unknown,
    Extermination,
    BaseCaptured,
    Timeout,
    Failure,
    Technical,
    Score,
    ScoreOnTimeout,
    PveMainTaskSucceeded,
    PveMainTaskFailed,
    ScoreZero,
    ScoreExcess,
    Other(u8),
}

impl FinishType {
    pub fn from_name(name: &str) -> Self {
        match name {
            "UNKNOWN" => FinishType::Unknown,
            "EXTERMINATION" => FinishType::Extermination,
            "BASE" => FinishType::BaseCaptured,
            "TIMEOUT" => FinishType::Timeout,
            "FAILURE" => FinishType::Failure,
            "TECHNICAL" => FinishType::Technical,
            "SCORE" => FinishType::Score,
            "SCORE_ON_TIMEOUT" => FinishType::ScoreOnTimeout,
            "PVE_MAIN_TASK_SUCCEEDED" => FinishType::PveMainTaskSucceeded,
            "PVE_MAIN_TASK_FAILED" => FinishType::PveMainTaskFailed,
            "SCORE_ZERO" => FinishType::ScoreZero,
            "SCORE_EXCESS" => FinishType::ScoreExcess,
            _ => FinishType::Unknown,
        }
    }

    pub fn from_id(id: u8) -> Self {
        match id {
            0 => FinishType::Unknown,
            1 => FinishType::Extermination,
            2 => FinishType::BaseCaptured,
            3 => FinishType::Timeout,
            4 => FinishType::Failure,
            5 => FinishType::Technical,
            8 => FinishType::Score,
            9 => FinishType::ScoreOnTimeout,
            10 => FinishType::PveMainTaskSucceeded,
            11 => FinishType::PveMainTaskFailed,
            12 => FinishType::ScoreZero,
            13 => FinishType::ScoreExcess,
            other => FinishType::Other(other),
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            FinishType::Unknown => "Unknown",
            FinishType::Extermination => "Extermination",
            FinishType::BaseCaptured => "Base Captured",
            FinishType::Timeout => "Timeout",
            FinishType::Failure => "Failure",
            FinishType::Technical => "Technical",
            FinishType::Score => "Score",
            FinishType::ScoreOnTimeout => "Score on Timeout",
            FinishType::PveMainTaskSucceeded => "PvE Main Task Succeeded",
            FinishType::PveMainTaskFailed => "PvE Main Task Failed",
            FinishType::ScoreZero => "Score Zero",
            FinishType::ScoreExcess => "Score Excess",
            FinishType::Other(_) => "Other",
        }
    }
}

impl fmt::Display for FinishType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name())
    }
}

/// Submarine depth state, from `DEPTH_STATE` in battle.xml.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
pub enum DepthState {
    Invalid,
    Surface,
    Periscope,
    Working,
    Invulnerable,
    Other(u8),
}

impl DepthState {
    pub fn from_name(name: &str) -> Self {
        match name {
            "INVALID_STATE" => DepthState::Invalid,
            "SURFACE" => DepthState::Surface,
            "PERISCOPE" => DepthState::Periscope,
            "WORKING" => DepthState::Working,
            "INVULNERABLE" => DepthState::Invulnerable,
            _ => DepthState::Other(0),
        }
    }

    pub fn from_id(id: u8) -> Self {
        match id {
            0 => DepthState::Surface,
            1 => DepthState::Periscope,
            2 => DepthState::Working,
            3 => DepthState::Invulnerable,
            0xFF => DepthState::Invalid,
            other => DepthState::Other(other),
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            DepthState::Invalid => "Invalid",
            DepthState::Surface => "Surface",
            DepthState::Periscope => "Periscope",
            DepthState::Working => "Operating Depth",
            DepthState::Invulnerable => "Deep Dive",
            DepthState::Other(_) => "Other",
        }
    }
}

impl Default for DepthState {
    fn default() -> Self {
        DepthState::Surface
    }
}

impl fmt::Display for DepthState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name())
    }
}

/// Selected weapon type, from `SHIP_WEAPON_TYPES` in ships.xml.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
pub enum WeaponType {
    Artillery,
    Secondaries,
    Torpedoes,
    Planes,
    Pinger,
    Other(u32),
}

impl WeaponType {
    pub fn from_name(name: &str) -> Self {
        match name {
            "ARTILLERY" => WeaponType::Artillery,
            "ATBA" => WeaponType::Secondaries,
            "TORPEDO" => WeaponType::Torpedoes,
            "AIRPLANES" => WeaponType::Planes,
            "PINGER" => WeaponType::Pinger,
            _ => WeaponType::Other(0),
        }
    }

    pub fn from_id(id: u32) -> Self {
        match id {
            0 => WeaponType::Artillery,
            1 => WeaponType::Secondaries,
            2 => WeaponType::Torpedoes,
            3 => WeaponType::Planes,
            4 => WeaponType::Pinger,
            other => WeaponType::Other(other),
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            WeaponType::Artillery => "Main Battery",
            WeaponType::Secondaries => "Secondaries",
            WeaponType::Torpedoes => "Torpedoes",
            WeaponType::Planes => "Planes",
            WeaponType::Pinger => "Sonar",
            WeaponType::Other(_) => "Other",
        }
    }
}

impl Default for WeaponType {
    fn default() -> Self {
        WeaponType::Artillery
    }
}

impl fmt::Display for WeaponType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name())
    }
}

/// Submarine battery state, from `BATTERY_STATE` in battle.xml.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
pub enum BatteryState {
    Idle,
    Charging,
    Discharging,
    CriticalDischarging,
    BrokenCharging,
    BrokenIdle,
    Regeneration,
    Empty,
    Other(u8),
}

impl BatteryState {
    pub fn from_name(name: &str) -> Self {
        match name {
            "IDLE" => BatteryState::Idle,
            "CHARGING" => BatteryState::Charging,
            "DISCHARGING" => BatteryState::Discharging,
            "CRITICAL_DISCHARGING" => BatteryState::CriticalDischarging,
            "BROKEN_CHARGING" => BatteryState::BrokenCharging,
            "BROKEN_IDLE" => BatteryState::BrokenIdle,
            "REGENERATION" => BatteryState::Regeneration,
            "EMPTY" => BatteryState::Empty,
            _ => BatteryState::Other(0),
        }
    }

    pub fn from_id(id: u8) -> Self {
        match id {
            0 => BatteryState::Idle,
            1 => BatteryState::Charging,
            2 => BatteryState::Discharging,
            3 => BatteryState::CriticalDischarging,
            4 => BatteryState::BrokenCharging,
            5 => BatteryState::BrokenIdle,
            6 => BatteryState::Regeneration,
            7 => BatteryState::Empty,
            other => BatteryState::Other(other),
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            BatteryState::Idle => "Idle",
            BatteryState::Charging => "Charging",
            BatteryState::Discharging => "Discharging",
            BatteryState::CriticalDischarging => "Critical Discharging",
            BatteryState::BrokenCharging => "Broken Charging",
            BatteryState::BrokenIdle => "Broken Idle",
            BatteryState::Regeneration => "Regeneration",
            BatteryState::Empty => "Empty",
            BatteryState::Other(_) => "Other",
        }
    }
}

impl Default for BatteryState {
    fn default() -> Self {
        BatteryState::Idle
    }
}

impl fmt::Display for BatteryState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name())
    }
}
