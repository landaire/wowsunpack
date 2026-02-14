use std::collections::HashMap;
use std::path::Path;

use crate::data::idx::FileNode;
use crate::data::pkg::PkgFileLoader;

/// Constants parsed from `gui/data/constants/battle.xml`.
#[derive(Clone)]
pub struct BattleConstants {
    camera_modes: HashMap<i32, String>,
    death_reasons: HashMap<i32, String>,
    game_modes: HashMap<i32, String>,
    battle_results: HashMap<i32, String>,
    player_relations: HashMap<i32, String>,
    damage_modules: HashMap<i32, String>,
    finish_types: HashMap<i32, String>,
    consumable_states: HashMap<i32, String>,
    planes_types: HashMap<i32, String>,
    diplomacy_relations: HashMap<i32, String>,
    modules_states: HashMap<i32, String>,
    entity_types: HashMap<i32, String>,
    entity_states: HashMap<i32, String>,
    battery_states: HashMap<i32, String>,
    depth_states: HashMap<i32, String>,
    building_types: HashMap<i32, String>,
    torpedo_marker_types: HashMap<i32, String>,
}

impl BattleConstants {
    /// Load from game files, falling back to defaults if the file can't be read.
    pub fn load(file_tree: &FileNode, pkg_loader: &PkgFileLoader) -> Self {
        let mut buf = Vec::new();
        if file_tree
            .read_file_at_path(Path::new(BATTLE_CONSTANTS_PATH), pkg_loader, &mut buf)
            .is_ok()
        {
            Self::from_xml(&buf)
        } else {
            Self::defaults()
        }
    }

    /// Parse from raw XML bytes. Falls back to defaults per-field on parse failure.
    pub fn from_xml(xml: &[u8]) -> Self {
        let xml_str = match std::str::from_utf8(xml) {
            Ok(s) => s,
            Err(_) => return Self::defaults(),
        };

        let defaults = Self::defaults();
        Self {
            camera_modes: parse_integer_enum(xml_str, "CAMERA_MODE")
                .unwrap_or(defaults.camera_modes),
            death_reasons: parse_positional_enum(xml_str, "DEATH_REASON_NAME")
                .unwrap_or(defaults.death_reasons),
            game_modes: parse_integer_enum(xml_str, "GAME_MODE").unwrap_or(defaults.game_modes),
            battle_results: parse_integer_enum(xml_str, "BATTLE_RESULT")
                .unwrap_or(defaults.battle_results),
            player_relations: parse_integer_enum(xml_str, "PLAYER_RELATION")
                .unwrap_or(defaults.player_relations),
            damage_modules: parse_integer_enum(xml_str, "DAMAGE_MODULES")
                .unwrap_or(defaults.damage_modules),
            finish_types: parse_integer_enum(xml_str, "FINISH_TYPE")
                .unwrap_or(defaults.finish_types),
            consumable_states: parse_integer_enum(xml_str, "CONSUMABLE_STATES")
                .unwrap_or(defaults.consumable_states),
            planes_types: parse_integer_enum(xml_str, "PLANES_TYPES")
                .unwrap_or(defaults.planes_types),
            diplomacy_relations: parse_integer_enum(xml_str, "DIPLOMACY_RELATIONS")
                .unwrap_or(defaults.diplomacy_relations),
            modules_states: parse_integer_enum(xml_str, "MODULES_STATES")
                .unwrap_or(defaults.modules_states),
            entity_types: parse_integer_enum(xml_str, "ENTITY_TYPES")
                .unwrap_or(defaults.entity_types),
            entity_states: parse_integer_enum(xml_str, "ENTITY_STATES")
                .unwrap_or(defaults.entity_states),
            battery_states: parse_integer_enum(xml_str, "BATTERY_STATE")
                .unwrap_or(defaults.battery_states),
            depth_states: parse_integer_enum(xml_str, "DEPTH_STATE")
                .unwrap_or(defaults.depth_states),
            building_types: parse_positional_enum(xml_str, "BUILDING_TYPES")
                .unwrap_or(defaults.building_types),
            torpedo_marker_types: parse_positional_enum(xml_str, "TORPEDO_MARKER_TYPE")
                .unwrap_or(defaults.torpedo_marker_types),
        }
    }

    /// Hardcoded defaults matching known game versions (v15.0/v15.1).
    pub fn defaults() -> Self {
        Self {
            camera_modes: HashMap::from([
                (1, "AIRPLANES".into()),
                (2, "DOCK".into()),
                (3, "TACTICALMAP".into()),
                (4, "DEVFREE".into()),
                (5, "SHELLTRACKER".into()),
                (6, "PLANETRACKER".into()),
                (7, "DOCKMODULE".into()),
                (8, "SNAKETAIL".into()),
                (9, "SPECTATOR".into()),
                (10, "REPLAY_FPC".into()),
                (11, "UNDERWATER".into()),
                (12, "TACTICAL_CONSUMABLES".into()),
                (13, "RESPAWN_MAP".into()),
                (19, "DOCKFLAGS".into()),
                (20, "DOCKENSIGN".into()),
                (21, "DOCKLOOTBOX".into()),
                (22, "DOCKNAVALFLAG".into()),
                (23, "IDLEGAME".into()),
            ]),
            death_reasons: HashMap::from([
                (0, "NONE".into()),
                (1, "ARTILLERY".into()),
                (2, "ATBA".into()),
                (3, "TORPEDO".into()),
                (4, "BOMB".into()),
                (5, "TBOMB".into()),
                (6, "BURNING".into()),
                (7, "RAM".into()),
                (8, "TERRAIN".into()),
                (9, "FLOOD".into()),
                (10, "MIRROR".into()),
                (11, "SEA_MINE".into()),
                (12, "SPECIAL".into()),
                (13, "DBOMB".into()),
                (14, "ROCKET".into()),
                (15, "DETONATE".into()),
                (16, "HEALTH".into()),
                (17, "AP_SHELL".into()),
                (18, "HE_SHELL".into()),
                (19, "CS_SHELL".into()),
                (20, "FEL".into()),
                (21, "PORTAL".into()),
                (22, "SKIP_BOMB".into()),
                (23, "SECTOR_WAVE".into()),
                (24, "ACID".into()),
                (25, "LASER".into()),
                (26, "MATCH".into()),
                (27, "TIMER".into()),
                (28, "ADBOMB".into()),
                (29, "EVENT_1".into()),
                (30, "EVENT_2".into()),
                (31, "EVENT_3".into()),
                (32, "EVENT_4".into()),
                (33, "EVENT_5".into()),
                (34, "EVENT_6".into()),
                (35, "MISSILE".into()),
            ]),
            game_modes: HashMap::from([
                (-1, "INVALID".into()),
                (0, "TEST".into()),
                (1, "STANDART".into()),
                (2, "SINGLEBASE".into()),
                (7, "DOMINATION".into()),
                (8, "TUTORIAL".into()),
                (9, "MEGABASE".into()),
                (10, "FORTS".into()),
                (11, "STANDARD_DOMINATION".into()),
                (12, "EPICENTER".into()),
                (13, "ASSAULT_DEFENSE".into()),
                (14, "PVE".into()),
                (15, "ARMS_RACE".into()),
                (16, "EPICENTER_RING".into()),
                (17, "ANTI_STANDARD".into()),
                (18, "ATTACK_DEFENSE".into()),
                (19, "TORPEDO_BEAT".into()),
                (20, "TEAM_BATTLE_ROYALE".into()),
                (21, "ESCAPE_TO_PORTAL".into()),
                (22, "DOMINATION_ASYMM".into()),
                (23, "KEY_BATTLE".into()),
                (24, "PORTAL_2021".into()),
                (25, "TEAM_BATTLE_ROYALE_2021".into()),
                (26, "CONVOY_EVENT".into()),
                (27, "CONVOY_AIRSHIP".into()),
                (28, "TWO_TEAMS_BATTLE_ROYALE".into()),
                (29, "PINATA_EVENT".into()),
                (30, "RESPAWNS".into()),
                (31, "RESPAWNS_SECTORS".into()),
            ]),
            battle_results: HashMap::from([
                (0, "DEFEAT".into()),
                (1, "VICTORY".into()),
                (2, "DRAW".into()),
                (3, "SUCCESS".into()),
                (4, "FAILURE".into()),
                (5, "PORTAL".into()),
                (6, "MATCH".into()),
                (7, "DEATH".into()),
                (8, "TEAM_LADDER_WINNER".into()),
                (9, "TEAM_LADDER_LOSER".into()),
            ]),
            player_relations: HashMap::from([
                (0, "SELF".into()),
                (1, "ALLY".into()),
                (2, "ENEMY".into()),
                (3, "NEUTRAL".into()),
            ]),
            damage_modules: HashMap::from([
                (0, "ENGINE".into()),
                (1, "MAIN_CALIBER".into()),
                (2, "ATBA_GUN".into()),
                (3, "AVIATION".into()),
                (4, "AIR_DEFENSE".into()),
                (5, "OBSERVATION".into()),
                (6, "TORPEDO_TUBE".into()),
                (7, "PATH_CONTROL".into()),
                (8, "DEPTH_CHARGE_GUN".into()),
                (9, "BURN".into()),
                (10, "FLOOD".into()),
                (11, "ACID".into()),
                (12, "HEATED".into()),
                (13, "WAVED".into()),
                (14, "PINGER".into()),
                (15, "OIL_LEAK".into()),
                (16, "OIL_LEAK_PENDING".into()),
                (17, "WILD_FIRE".into()),
            ]),
            finish_types: HashMap::from([
                (0, "UNKNOWN".into()),
                (1, "EXTERMINATION".into()),
                (2, "BASE".into()),
                (3, "TIMEOUT".into()),
                (4, "FAILURE".into()),
                (5, "TECHNICAL".into()),
                (8, "SCORE".into()),
                (9, "SCORE_ON_TIMEOUT".into()),
                (10, "PVE_MAIN_TASK_SUCCEEDED".into()),
                (11, "PVE_MAIN_TASK_FAILED".into()),
                (12, "SCORE_ZERO".into()),
                (13, "SCORE_EXCESS".into()),
            ]),
            consumable_states: HashMap::from([
                (0, "READY".into()),
                (1, "SELECTED".into()),
                (2, "AT_WORK".into()),
                (3, "RELOAD".into()),
                (4, "NO_AMMO".into()),
                (5, "PREPARATION".into()),
                (6, "REGENERATION".into()),
            ]),
            planes_types: HashMap::from([
                (0, "SCOUT".into()),
                (1, "DIVEBOMBER".into()),
                (2, "TORPEDOBOMBER".into()),
                (3, "FIGHTER".into()),
                (4, "AUXILIARY".into()),
                (5, "SKIP_BOMBER".into()),
                (6, "AIR_SUPPORT".into()),
                (7, "AIRSHIP".into()),
            ]),
            diplomacy_relations: HashMap::from([
                (0, "SELF".into()),
                (1, "ALLY".into()),
                (2, "NEUTRAL".into()),
                (3, "ENEMY".into()),
                (4, "AGGRESSOR".into()),
            ]),
            modules_states: HashMap::from([
                (0, "NORMAL".into()),
                (1, "DAMAGED".into()),
                (2, "CRIT".into()),
                (3, "BROKEN".into()),
                (4, "DEAD".into()),
            ]),
            entity_types: HashMap::from([
                (-1, "INVALID".into()),
                (0, "SHIP".into()),
                (1, "PLANE".into()),
                (2, "TORPEDO".into()),
                (3, "BUILDING".into()),
                (11, "CAPTURE_POINT".into()),
                (12, "PLAYER".into()),
                (13, "EPICENTER".into()),
                (14, "SCENARIO_OBJECT".into()),
                (15, "DROP_ZONE".into()),
                (16, "ATTENTION_POINT".into()),
                (17, "KEY_OBJECT".into()),
                (18, "INTERACTIVE_ZONE".into()),
                (27, "PINATA_SHIP".into()),
                (28, "STARTREK_SCRAP".into()),
                (29, "MISSILE".into()),
                (99, "NAVPOINT".into()),
                (100, "EMPTY".into()),
            ]),
            entity_states: HashMap::from([
                (0, "EMPTY".into()),
                (1, "REPAIR".into()),
                (2, "GATHERING_SURVIVORS".into()),
                (3, "FILTH".into()),
                (4, "FROZEN".into()),
                (5, "UNLOADING_MARINES".into()),
                (6, "INSIDE_WEATHER".into()),
                (7, "NEAR_WEATHER".into()),
                (8, "ILLUMINATED".into()),
                (9, "BY_NIGHT".into()),
                (10, "INSIDE_MINEFIELD".into()),
                (11, "NEAR_MINEFIELD".into()),
                (12, "CAPTURING_LOCKED".into()),
                (13, "DANGER".into()),
                (14, "TEAM_01".into()),
                (15, "TEAM_02".into()),
                (16, "TEAM_03".into()),
                (17, "TEAM_11".into()),
                (18, "TEAM_12".into()),
                (19, "TEAM_13".into()),
                (20, "TEAM_21".into()),
                (21, "TEAM_22".into()),
                (22, "TEAM_23".into()),
                (23, "TEAM_31".into()),
                (24, "TEAM_32".into()),
                (25, "TEAM_33".into()),
                (26, "SPOTTED_BY_ENEMY".into()),
                (27, "INVULNERABLE".into()),
                (28, "FLAGSHIP".into()),
                (29, "MIGNON".into()),
                (30, "REPAIR_SHIP".into()),
                (100, "SHIP_PARAMS_CHANGE_BY_BROKEN_MODULES".into()),
                (101, "BURN".into()),
                (102, "FLOOD".into()),
                (103, "HOLD_RESOURCE".into()),
                (104, "HOLD_RESOURCE_FILTHIOR_TEAM_0".into()),
                (105, "HOLD_RESOURCE_FILTHIOR_TEAM_1".into()),
                (106, "HOLD_RESOURCE_FILTHIOR_TEAM_2".into()),
                (107, "HOLD_RESOURCE_FILTHIOR_TEAM_3".into()),
                (108, "HOLD_RESOURCE_POINTS".into()),
                (109, "SHIP_PARAMS_CHANGE_BY_CRIT_MODULES".into()),
                (110, "SHIP_PARAMS_CHANGE_BY_SPECIAL_MODULES".into()),
                (111, "SHIP_PARAMS_CHANGE_BY_MODIFIERS".into()),
                (112, "SHIP_PARAMS_CHANGE_BY_TALENTS".into()),
                (113, "SHIP_PARAMS_CHANGE_BY_ATBA_ACCURACY_PERK".into()),
                (114, "SHIP_PARAMS_CHANGE_BY_PERKS".into()),
                (115, "SHIP_PARAMS_CHANGE_BY_BUFFS".into()),
                (116, "SHIP_PARAMS_CHANGE_BY_WEATHER".into()),
                (117, "SHIP_PARAMS_CHANGE_BY_INTERACTIVE_ZONE".into()),
                (118, "EMERGENCY_SURFACING".into()),
                (119, "SHIP_PARAMS_CHANGE_BY_BATTERY_STATE".into()),
                (120, "SHIP_PARAMS_CHANGE_BY_DEPTH".into()),
                (121, "SHIP_PARAMS_CHANGE_BY_ARTILLERY_FIRE_MODE".into()),
                (122, "SHIP_PARAMS_CHANGE_BY_RAGE_MODE".into()),
                (123, "SHIP_PARAMS_CHANGE_BY_CONSUMABLES".into()),
                (124, "SHIP_PARAMS_CHANGE_BY_NOT_SPECIAL_MODULES".into()),
                (125, "SHIP_PARAMS_CHANGE_BY_CONSUMABLE_LOCKER".into()),
                (126, "SHIP_PARAMS_CHANGE_BY_ANTI_ABUSE_SYSTEM".into()),
                (127, "BATTLE_CARD_SELECTOR_AVAILABLE".into()),
                (128, "SHIP_PARAMS_CHANGE_BY_INNATE_SKILLS".into()),
            ]),
            battery_states: HashMap::from([
                (0, "IDLE".into()),
                (1, "CHARGING".into()),
                (2, "FROZEN".into()),
                (3, "SPENDING_NORMAL".into()),
                (4, "SPENDING_WARNING".into()),
                (5, "SPENDING_CRITICAL".into()),
                (6, "BURNING".into()),
                (7, "EMPTY".into()),
            ]),
            depth_states: HashMap::from([
                (-1, "INVALID_STATE".into()),
                (0, "SURFACE".into()),
                (1, "PERISCOPE".into()),
                (2, "WORKING".into()),
                (3, "INVULNERABLE".into()),
            ]),
            building_types: HashMap::from([
                (0, "ANTI_AIRCRAFT".into()),
                (1, "AIR_BASE".into()),
                (2, "COASTAL_ARTILLERY".into()),
                (3, "MILITARY".into()),
                (4, "SENSOR_TOWER".into()),
                (5, "COMPLEX".into()),
                (6, "RAY_TOWER".into()),
                (7, "GENERATOR".into()),
                (8, "SPACE_STATION".into()),
            ]),
            torpedo_marker_types: HashMap::from([
                (0, "NORMAL".into()),
                (1, "DEEP_WATER".into()),
                (2, "ACOUSTIC".into()),
                (3, "MAGNETIC".into()),
                (4, "NOT_DANGEROUS".into()),
            ]),
        }
    }

    pub fn camera_mode(&self, id: i32) -> Option<&str> {
        self.camera_modes.get(&id).map(|s| s.as_str())
    }

    pub fn death_reason(&self, id: i32) -> Option<&str> {
        self.death_reasons.get(&id).map(|s| s.as_str())
    }

    pub fn game_mode(&self, id: i32) -> Option<&str> {
        self.game_modes.get(&id).map(|s| s.as_str())
    }

    pub fn battle_result(&self, id: i32) -> Option<&str> {
        self.battle_results.get(&id).map(|s| s.as_str())
    }

    pub fn player_relation(&self, id: i32) -> Option<&str> {
        self.player_relations.get(&id).map(|s| s.as_str())
    }

    pub fn damage_module(&self, id: i32) -> Option<&str> {
        self.damage_modules.get(&id).map(|s| s.as_str())
    }

    pub fn finish_type(&self, id: i32) -> Option<&str> {
        self.finish_types.get(&id).map(|s| s.as_str())
    }

    pub fn consumable_state(&self, id: i32) -> Option<&str> {
        self.consumable_states.get(&id).map(|s| s.as_str())
    }

    pub fn planes_type(&self, id: i32) -> Option<&str> {
        self.planes_types.get(&id).map(|s| s.as_str())
    }

    pub fn diplomacy_relation(&self, id: i32) -> Option<&str> {
        self.diplomacy_relations.get(&id).map(|s| s.as_str())
    }

    pub fn modules_state(&self, id: i32) -> Option<&str> {
        self.modules_states.get(&id).map(|s| s.as_str())
    }

    pub fn entity_type(&self, id: i32) -> Option<&str> {
        self.entity_types.get(&id).map(|s| s.as_str())
    }

    pub fn entity_state(&self, id: i32) -> Option<&str> {
        self.entity_states.get(&id).map(|s| s.as_str())
    }

    pub fn battery_state(&self, id: i32) -> Option<&str> {
        self.battery_states.get(&id).map(|s| s.as_str())
    }

    pub fn depth_state(&self, id: i32) -> Option<&str> {
        self.depth_states.get(&id).map(|s| s.as_str())
    }

    pub fn building_type(&self, id: i32) -> Option<&str> {
        self.building_types.get(&id).map(|s| s.as_str())
    }

    pub fn torpedo_marker_type(&self, id: i32) -> Option<&str> {
        self.torpedo_marker_types.get(&id).map(|s| s.as_str())
    }

    pub fn camera_modes_mut(&mut self) -> &mut HashMap<i32, String> {
        &mut self.camera_modes
    }

    pub fn death_reasons_mut(&mut self) -> &mut HashMap<i32, String> {
        &mut self.death_reasons
    }

    pub fn game_modes_mut(&mut self) -> &mut HashMap<i32, String> {
        &mut self.game_modes
    }

    pub fn battle_results_mut(&mut self) -> &mut HashMap<i32, String> {
        &mut self.battle_results
    }

    pub fn player_relations_mut(&mut self) -> &mut HashMap<i32, String> {
        &mut self.player_relations
    }

    pub fn damage_modules_mut(&mut self) -> &mut HashMap<i32, String> {
        &mut self.damage_modules
    }

    pub fn finish_types_mut(&mut self) -> &mut HashMap<i32, String> {
        &mut self.finish_types
    }

    pub fn consumable_states_mut(&mut self) -> &mut HashMap<i32, String> {
        &mut self.consumable_states
    }

    pub fn planes_types_mut(&mut self) -> &mut HashMap<i32, String> {
        &mut self.planes_types
    }

    pub fn diplomacy_relations_mut(&mut self) -> &mut HashMap<i32, String> {
        &mut self.diplomacy_relations
    }

    pub fn modules_states_mut(&mut self) -> &mut HashMap<i32, String> {
        &mut self.modules_states
    }

    pub fn entity_types_mut(&mut self) -> &mut HashMap<i32, String> {
        &mut self.entity_types
    }

    pub fn entity_states_mut(&mut self) -> &mut HashMap<i32, String> {
        &mut self.entity_states
    }

    pub fn battery_states_mut(&mut self) -> &mut HashMap<i32, String> {
        &mut self.battery_states
    }

    pub fn depth_states_mut(&mut self) -> &mut HashMap<i32, String> {
        &mut self.depth_states
    }

    pub fn building_types_mut(&mut self) -> &mut HashMap<i32, String> {
        &mut self.building_types
    }

    pub fn torpedo_marker_types_mut(&mut self) -> &mut HashMap<i32, String> {
        &mut self.torpedo_marker_types
    }

    pub fn camera_modes(&self) -> &HashMap<i32, String> {
        &self.camera_modes
    }

    pub fn death_reasons(&self) -> &HashMap<i32, String> {
        &self.death_reasons
    }

    pub fn game_modes(&self) -> &HashMap<i32, String> {
        &self.game_modes
    }

    pub fn battle_results(&self) -> &HashMap<i32, String> {
        &self.battle_results
    }

    pub fn player_relations(&self) -> &HashMap<i32, String> {
        &self.player_relations
    }

    pub fn damage_modules(&self) -> &HashMap<i32, String> {
        &self.damage_modules
    }

    pub fn finish_types(&self) -> &HashMap<i32, String> {
        &self.finish_types
    }

    pub fn consumable_states(&self) -> &HashMap<i32, String> {
        &self.consumable_states
    }

    pub fn planes_types(&self) -> &HashMap<i32, String> {
        &self.planes_types
    }

    pub fn diplomacy_relations(&self) -> &HashMap<i32, String> {
        &self.diplomacy_relations
    }

    pub fn modules_states(&self) -> &HashMap<i32, String> {
        &self.modules_states
    }

    pub fn entity_types(&self) -> &HashMap<i32, String> {
        &self.entity_types
    }

    pub fn entity_states(&self) -> &HashMap<i32, String> {
        &self.entity_states
    }

    pub fn battery_states(&self) -> &HashMap<i32, String> {
        &self.battery_states
    }

    pub fn depth_states(&self) -> &HashMap<i32, String> {
        &self.depth_states
    }

    pub fn building_types(&self) -> &HashMap<i32, String> {
        &self.building_types
    }

    pub fn torpedo_marker_types(&self) -> &HashMap<i32, String> {
        &self.torpedo_marker_types
    }
}

/// Constants parsed from `gui/data/constants/ships.xml`.
#[derive(Clone)]
pub struct ShipsConstants {
    weapon_types: HashMap<i32, String>,
    module_types: HashMap<i32, String>,
}

impl ShipsConstants {
    /// Load from game files, falling back to defaults if the file can't be read.
    pub fn load(file_tree: &FileNode, pkg_loader: &PkgFileLoader) -> Self {
        let mut buf = Vec::new();
        if file_tree
            .read_file_at_path(Path::new(SHIPS_CONSTANTS_PATH), pkg_loader, &mut buf)
            .is_ok()
        {
            Self::from_xml(&buf)
        } else {
            Self::defaults()
        }
    }

    /// Parse from raw XML bytes. Falls back to defaults on parse failure.
    pub fn from_xml(xml: &[u8]) -> Self {
        let xml_str = match std::str::from_utf8(xml) {
            Ok(s) => s,
            Err(_) => return Self::defaults(),
        };

        let defaults = Self::defaults();
        Self {
            weapon_types: parse_integer_enum(xml_str, "SHIP_WEAPON_TYPES")
                .unwrap_or(defaults.weapon_types),
            module_types: parse_integer_enum(xml_str, "SHIP_MODULE_TYPES")
                .unwrap_or(defaults.module_types),
        }
    }

    /// Hardcoded defaults matching known game versions (v15.0/v15.1).
    pub fn defaults() -> Self {
        Self {
            weapon_types: HashMap::from([
                (-1, "NONE".into()),
                (0, "ARTILLERY".into()),
                (1, "ATBA".into()),
                (2, "TORPEDO".into()),
                (3, "AIRPLANES".into()),
                (4, "AIRDEFENCE".into()),
                (5, "DEPTH_CHARGES".into()),
                (6, "PINGER".into()),
                (7, "CHARGE_LASER".into()),
                (8, "IMPULSE_LASER".into()),
                (9, "AXIS_LASER".into()),
                (10, "PHASER_LASER".into()),
                (11, "WAVES".into()),
                (12, "AIR_SUPPORT".into()),
                (13, "ANTI_MISSILE".into()),
                (14, "MISSILES".into()),
                (100, "SQUADRON".into()),
                (200, "PULSE_PHASERS".into()),
            ]),
            module_types: HashMap::from([
                (0, "ARTILLERY".into()),
                (1, "HULL".into()),
                (2, "TORPEDOES".into()),
                (3, "SUO".into()),
                (4, "ENGINE".into()),
                (5, "TORPEDO_BOMBER".into()),
                (6, "DIVE_BOMBER".into()),
                (7, "FIGHTER".into()),
                (8, "FLIGHT_CONTROLL".into()),
                (9, "HYDROPHONE".into()),
                (10, "SKIP_BOMBER".into()),
                (11, "PRIMARY_WEAPONS".into()),
                (12, "SECONDARY_WEAPONS".into()),
                (13, "ABILITIES".into()),
            ]),
        }
    }

    pub fn weapon_type(&self, id: i32) -> Option<&str> {
        self.weapon_types.get(&id).map(|s| s.as_str())
    }

    pub fn module_type(&self, id: i32) -> Option<&str> {
        self.module_types.get(&id).map(|s| s.as_str())
    }

    pub fn weapon_types(&self) -> &HashMap<i32, String> {
        &self.weapon_types
    }

    pub fn module_types(&self) -> &HashMap<i32, String> {
        &self.module_types
    }

    pub fn weapon_types_mut(&mut self) -> &mut HashMap<i32, String> {
        &mut self.weapon_types
    }

    pub fn module_types_mut(&mut self) -> &mut HashMap<i32, String> {
        &mut self.module_types
    }
}

/// Constants parsed from `gui/data/constants/weapons.xml`.
#[derive(Clone)]
pub struct WeaponsConstants {
    gun_states: HashMap<i32, String>,
}

impl WeaponsConstants {
    /// Load from game files, falling back to defaults if the file can't be read.
    pub fn load(file_tree: &FileNode, pkg_loader: &PkgFileLoader) -> Self {
        let mut buf = Vec::new();
        if file_tree
            .read_file_at_path(Path::new(WEAPONS_CONSTANTS_PATH), pkg_loader, &mut buf)
            .is_ok()
        {
            Self::from_xml(&buf)
        } else {
            Self::defaults()
        }
    }

    /// Parse from raw XML bytes. Falls back to defaults on parse failure.
    pub fn from_xml(xml: &[u8]) -> Self {
        let xml_str = match std::str::from_utf8(xml) {
            Ok(s) => s,
            Err(_) => return Self::defaults(),
        };

        let defaults = Self::defaults();
        Self {
            gun_states: parse_integer_enum(xml_str, "GUN_STATE").unwrap_or(defaults.gun_states),
        }
    }

    /// Hardcoded defaults matching known game versions (v15.0/v15.1).
    pub fn defaults() -> Self {
        Self {
            gun_states: HashMap::from([
                (1, "READY".into()),
                (2, "WORK".into()),
                (3, "RELOAD".into()),
                (4, "SWITCHING_AMMO".into()),
                (5, "RELOAD_STOPPED".into()),
                (6, "CHARGE".into()),
                (7, "CRITICAL".into()),
                (8, "DESTROYED".into()),
                (9, "SWITCHING_CRITICAL".into()),
                (10, "DISABLED".into()),
                (11, "BROKEN".into()),
            ]),
        }
    }

    pub fn gun_state(&self, id: i32) -> Option<&str> {
        self.gun_states.get(&id).map(|s| s.as_str())
    }

    pub fn gun_states(&self) -> &HashMap<i32, String> {
        &self.gun_states
    }

    pub fn gun_states_mut(&mut self) -> &mut HashMap<i32, String> {
        &mut self.gun_states
    }
}

/// Constants parsed from `gui/data/constants/common.xml`.
#[derive(Clone)]
pub struct CommonConstants {
    plane_ammo_types: HashMap<i32, String>,
    torpedo_types: HashMap<i32, String>,
    consumable_types: Option<HashMap<i32, crate::game_types::Consumable>>,
}

impl CommonConstants {
    /// Load from game files, falling back to defaults if the file can't be read.
    pub fn load(file_tree: &FileNode, pkg_loader: &PkgFileLoader) -> Self {
        let mut buf = Vec::new();
        if file_tree
            .read_file_at_path(Path::new(COMMON_CONSTANTS_PATH), pkg_loader, &mut buf)
            .is_ok()
        {
            Self::from_xml(&buf)
        } else {
            Self::defaults()
        }
    }

    /// Parse from raw XML bytes. Falls back to defaults on parse failure.
    pub fn from_xml(xml: &[u8]) -> Self {
        let xml_str = match std::str::from_utf8(xml) {
            Ok(s) => s,
            Err(_) => return Self::defaults(),
        };

        let defaults = Self::defaults();
        Self {
            plane_ammo_types: parse_integer_enum(xml_str, "PLANE_AMMO_TYPES")
                .unwrap_or(defaults.plane_ammo_types),
            torpedo_types: parse_integer_enum(xml_str, "TORPEDO_TYPE")
                .unwrap_or(defaults.torpedo_types),
            consumable_types: None,
        }
    }

    /// Hardcoded defaults matching known game versions (v15.0/v15.1).
    pub fn defaults() -> Self {
        Self {
            plane_ammo_types: HashMap::from([
                (-1, "NONE".into()),
                (0, "PROJECTILE".into()),
                (1, "BOMB_HE".into()),
                (2, "BOMB_AP".into()),
                (3, "SKIP_BOMB_HE".into()),
                (4, "SKIP_BOMB_AP".into()),
                (5, "TORPEDO".into()),
                (6, "TORPEDO_DEEPWATER".into()),
                (7, "PROJECTILE_AP".into()),
                (8, "DEPTH_CHARGE".into()),
                (9, "MINE".into()),
                (10, "SMOKE".into()),
            ]),
            torpedo_types: HashMap::from([
                (0, "COMMON".into()),
                (1, "SUBMARINE".into()),
                (2, "PHOTON".into()),
            ]),
            consumable_types: None,
        }
    }

    pub fn plane_ammo_type(&self, id: i32) -> Option<&str> {
        self.plane_ammo_types.get(&id).map(|s| s.as_str())
    }

    pub fn torpedo_type(&self, id: i32) -> Option<&str> {
        self.torpedo_types.get(&id).map(|s| s.as_str())
    }

    pub fn set_consumable_types(&mut self, map: HashMap<i32, crate::game_types::Consumable>) {
        self.consumable_types = Some(map);
    }

    pub fn consumable_type(&self, id: i32) -> Option<&crate::game_types::Consumable> {
        self.consumable_types.as_ref()?.get(&id)
    }

    pub fn plane_ammo_types(&self) -> &HashMap<i32, String> {
        &self.plane_ammo_types
    }

    pub fn torpedo_types(&self) -> &HashMap<i32, String> {
        &self.torpedo_types
    }

    pub fn consumable_types(&self) -> Option<&HashMap<i32, crate::game_types::Consumable>> {
        self.consumable_types.as_ref()
    }

    pub fn plane_ammo_types_mut(&mut self) -> &mut HashMap<i32, String> {
        &mut self.plane_ammo_types
    }

    pub fn torpedo_types_mut(&mut self) -> &mut HashMap<i32, String> {
        &mut self.torpedo_types
    }

    pub fn consumable_types_mut(
        &mut self,
    ) -> &mut Option<HashMap<i32, crate::game_types::Consumable>> {
        &mut self.consumable_types
    }
}

/// Constants parsed from `gui/data/constants/channel.xml`.
#[derive(Clone)]
pub struct ChannelConstants {
    battle_chat_channel_types: HashMap<i32, String>,
    channel_type_idents: HashMap<i32, String>,
}

impl ChannelConstants {
    /// Load from game files, falling back to defaults if the file can't be read.
    pub fn load(file_tree: &FileNode, pkg_loader: &PkgFileLoader) -> Self {
        let mut buf = Vec::new();
        if file_tree
            .read_file_at_path(Path::new(CHANNEL_CONSTANTS_PATH), pkg_loader, &mut buf)
            .is_ok()
        {
            Self::from_xml(&buf)
        } else {
            Self::defaults()
        }
    }

    /// Parse from raw XML bytes. Falls back to defaults on parse failure.
    pub fn from_xml(xml: &[u8]) -> Self {
        let xml_str = match std::str::from_utf8(xml) {
            Ok(s) => s,
            Err(_) => return Self::defaults(),
        };

        let defaults = Self::defaults();
        Self {
            battle_chat_channel_types: parse_integer_enum(xml_str, "BATTLE_CHAT_CHANNEL_TYPE")
                .unwrap_or(defaults.battle_chat_channel_types),
            channel_type_idents: parse_positional_enum(xml_str, "CHANNEL_TYPE_IDENT_VALUE")
                .unwrap_or(defaults.channel_type_idents),
        }
    }

    /// Hardcoded defaults matching known game versions (v15.0/v15.1).
    pub fn defaults() -> Self {
        Self {
            battle_chat_channel_types: HashMap::from([
                (0, "GENERAL".into()),
                (1, "TEAM".into()),
                (2, "DIVISION".into()),
                (3, "SYSTEM".into()),
            ]),
            channel_type_idents: HashMap::from([
                (0, "UNKNOWN".into()),
                (1, "GROUP_OPEN".into()),
                (2, "GROUP_CLOSED".into()),
                (3, "PREBATTLE".into()),
                (4, "COMMON".into()),
                (5, "PRIVATE".into()),
                (6, "CLAN".into()),
                (7, "TRAINING_ROOM".into()),
            ]),
        }
    }

    pub fn battle_chat_channel_type(&self, id: i32) -> Option<&str> {
        self.battle_chat_channel_types.get(&id).map(|s| s.as_str())
    }

    pub fn channel_type_ident(&self, id: i32) -> Option<&str> {
        self.channel_type_idents.get(&id).map(|s| s.as_str())
    }

    pub fn battle_chat_channel_types(&self) -> &HashMap<i32, String> {
        &self.battle_chat_channel_types
    }

    pub fn channel_type_idents(&self) -> &HashMap<i32, String> {
        &self.channel_type_idents
    }

    pub fn battle_chat_channel_types_mut(&mut self) -> &mut HashMap<i32, String> {
        &mut self.battle_chat_channel_types
    }

    pub fn channel_type_idents_mut(&mut self) -> &mut HashMap<i32, String> {
        &mut self.channel_type_idents
    }
}

/// Parse an `<enum type="Integer">` block from XML.
fn parse_integer_enum(xml: &str, enum_name: &str) -> Option<HashMap<i32, String>> {
    let doc = roxmltree::Document::parse(xml).ok()?;
    let enum_node = doc
        .descendants()
        .find(|n| n.has_tag_name("enum") && n.attribute("name") == Some(enum_name))?;

    let mut map = HashMap::new();
    for child in enum_node.children() {
        if child.has_tag_name("const") {
            if let (Some(name), Some(value_str)) =
                (child.attribute("name"), child.attribute("value"))
            {
                if let Ok(value) = value_str.trim().parse::<i32>() {
                    map.insert(value, name.to_string());
                }
            }
        }
    }

    if map.is_empty() { None } else { Some(map) }
}

/// Parse an `<enum type="String">` block from XML (positional indexing).
fn parse_positional_enum(xml: &str, enum_name: &str) -> Option<HashMap<i32, String>> {
    let doc = roxmltree::Document::parse(xml).ok()?;
    let enum_node = doc
        .descendants()
        .find(|n| n.has_tag_name("enum") && n.attribute("name") == Some(enum_name))?;

    let mut map = HashMap::new();
    let mut index = 0i32;
    for child in enum_node.children() {
        if child.has_tag_name("const") {
            if let Some(name) = child.attribute("name") {
                map.insert(index, name.to_string());
            }
            index += 1;
        }
    }

    if map.is_empty() { None } else { Some(map) }
}

/// The file path within the game's `res/` directory for battle constants.
pub const BATTLE_CONSTANTS_PATH: &str = "gui/data/constants/battle.xml";

/// The file path within the game's `res/` directory for ship constants.
pub const SHIPS_CONSTANTS_PATH: &str = "gui/data/constants/ships.xml";

/// The file path within the game's `res/` directory for weapons constants.
pub const WEAPONS_CONSTANTS_PATH: &str = "gui/data/constants/weapons.xml";

/// The file path within the game's `res/` directory for common constants.
pub const COMMON_CONSTANTS_PATH: &str = "gui/data/constants/common.xml";

/// The file path within the game's `res/` directory for channel constants.
pub const CHANNEL_CONSTANTS_PATH: &str = "gui/data/constants/channel.xml";
