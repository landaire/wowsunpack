use std::{
    borrow::Cow,
    collections::{BTreeMap, HashMap},
    path::Path,
    str::FromStr,
    sync::Arc,
};

use gettext::Catalog;
use itertools::Itertools;

use pickled::{HashableValue, Value};
use tracing::debug;

use crate::{
    Rc,
    data::{DataFileWithCallback, ResourceLoader, idx::FileNode, pkg::PkgFileLoader},
    error::ErrorKind,
    game_params::convert::game_params_to_pickle,
    rpc::entitydefs::{EntitySpec, parse_scripts},
};

use super::types::*;

pub struct GameMetadataProvider {
    params: GameParams,
    param_id_to_translation_id: HashMap<u32, String>,
    translations: Option<Catalog>,
    specs: Arc<Vec<EntitySpec>>,
}

impl GameParamProvider for GameMetadataProvider {
    fn game_param_by_id(&self, id: u32) -> Option<Rc<Param>> {
        self.params.game_param_by_id(id)
    }

    fn game_param_by_index(&self, index: &str) -> Option<Rc<Param>> {
        self.params.game_param_by_index(index)
    }

    fn game_param_by_name(&self, name: &str) -> Option<Rc<Param>> {
        self.params.game_param_by_name(name)
    }

    fn params(&self) -> &[Rc<Param>] {
        self.params.params()
    }
}

impl ResourceLoader for GameMetadataProvider {
    fn localized_name_from_param(&self, param: &Param) -> Option<&str> {
        self.param_localization_id(param.id()).and_then(|id| {
            self.translations
                .as_ref()
                .map(|catalog| catalog.gettext(id))
        })
    }

    fn localized_name_from_id(&self, id: &str) -> Option<String> {
        self.translations
            .as_ref()
            .map(move |catalog| catalog.gettext(id).to_string())
    }

    fn game_param_by_id(&self, id: u32) -> Option<Rc<Param>> {
        self.params.game_param_by_id(id)
    }

    fn entity_specs(&self) -> &[EntitySpec] {
        self.specs.as_slice()
    }
}

macro_rules! game_param_to_type {
    ($params:ident, $key:expr, String) => {
        game_param_to_type!($params, $key, string_ref, String).inner().to_string()
    };
    ($params:ident, $key:expr, i8) => {
        game_param_to_type!($params, $key, i64) as i8
    };
    ($params:ident, $key:expr, i16) => {
        game_param_to_type!($params, $key, i64) as i16
    };
    ($params:ident, $key:expr, i32) => {
        game_param_to_type!($params, $key, i64) as i32
    };
    ($params:ident, $key:expr, u8) => {
        game_param_to_type!($params, $key, i64) as u8
    };
    ($params:ident, $key:expr, u16) => {
        game_param_to_type!($params, $key, i64) as u16
    };
    ($params:ident, $key:expr, u32) => {
        game_param_to_type!($params, $key, i64) as u32
    };
    ($params:ident, $key:expr, u64) => {
        game_param_to_type!($params, $key, i64) as u64
    };
    ($params:ident, $key:expr, usize) => {
        game_param_to_type!($params, $key, i64) as usize
    };
    ($params:ident, $key:expr, isize) => {
        game_param_to_type!($params, $key, i64) as isize
    };
    ($params:ident, $key:expr, f32) => {
        game_param_to_type!($params, $key, f64) as f32
    };

    // The above matches in this macro will either expand to
    // game_param_to_type!($params, $key, f64) as f32
    // game_param_to_type!($params, $key, i64) as <PRIMITIVE_TYPE>
    // game_param_to_type!($params, $key, bool)
    //
    // But the primitive types do not have an inner shared value,
    // so we need to handle those specially here.
    ($params:ident, $key:expr, i64) => {
        *$params
            .get(&HashableValue::String($key.to_string().into()))
            .unwrap_or_else(|| panic!("could not get {}", $key))
            .i64_ref()
            .unwrap_or_else(|| panic!("{} is not an i64", $key))
    };
    ($params:ident, $key:expr, f64) => {
        *$params
            .get(&HashableValue::String($key.to_string().into()))
            .unwrap_or_else(|| panic!("could not get {}", $key))
            .f64_ref()
            .unwrap_or_else(|| panic!("{} is not a f64", $key))
    };
    ($params:ident, $key:expr, bool) => {
        *$params
            .get(&HashableValue::String($key.to_string().into()))
            .unwrap_or_else(|| panic!("could not get {}", $key))
            .bool_ref()
            .unwrap_or_else(|| panic!("{} is not a bool", $key))
    };

    // Hashmaps that may fail to resolve
    ($params:ident, $key:expr, Option<HashMap<(), ()>>) => {
        if
        $params
            .get(&HashableValue::String($key.to_string().into()))
            .unwrap_or_else(|| panic!("could not get {}", $key))
            .is_none() {
                None
            } else {
                Some(game_param_to_type!($params, $key, HashMap<(), ()>))
            }
    };
    ($params:ident, $key:expr, Option<$ty:tt>) => {
        $params
            .get(&HashableValue::String($key.to_string().into()))
            .and_then(|value| {
                if value.is_none() {
                    None
                } else {
                    Some(game_param_to_type!($params, $key, $ty))
                }
            })
    };
    ($params:ident, $key:expr, HashMap<(), ()>) => {
        game_param_to_type!($params, $key, dict_ref, HashMap<(), ()>)
    };
    ($params:ident, $key:expr, &[()]) => {
        game_param_to_type!($params, $key, list_ref, &[()])
    };
    ($args:ident, $key:expr, $conversion_func:ident, $ty:ty) => {
        $args
            .get(&HashableValue::String($key.to_string().into()))
            .unwrap_or_else(|| panic!("could not get {}", $key))
            .$conversion_func()
            .unwrap_or_else(|| panic!("{} is not a {}", $key, stringify!($ty)))
    };
}

/// TODO: Too many unpredictable schema differences >:(
/// Need to just create structs for everything.
fn build_skill_modifiers(
    modifiers: &BTreeMap<HashableValue, Value>,
) -> Result<Vec<CrewSkillModifier>, CrewSkillModifierBuilderError> {
    modifiers
        .iter()
        .filter_map(|(modifier_name, modifier_data)| {
            let modifier_name = modifier_name
                .string_ref()
                .expect("modifier name is not a string")
                .to_owned();

            let modifier_name = modifier_name.inner();

            // TODO
            if matches!(
                modifier_name.as_ref(),
                "excludedConsumables"
                    | "fireResistanceEnabled"
                    | "priorityTargetEnabled"
                    | "artilleryAlertEnabled"
                    | "nearEnemyIntuitionEnabled"
                    | "GMRotationSpeed"
            ) {
                return None;
            }
            let modifier = if let Some(common_value) = modifier_data.i64_ref().cloned() {
                let common_value = common_value as f32;
                CrewSkillModifierBuilder::default()
                    .aircraft_carrier(common_value)
                    .auxiliary(common_value)
                    .battleship(common_value)
                    .cruiser(common_value)
                    .destroyer(common_value)
                    .submarine(common_value)
                    .name(modifier_name.to_owned())
                    .build()
            } else if let Some(common_value) = modifier_data.f64_ref().cloned() {
                let common_value = common_value as f32;
                CrewSkillModifierBuilder::default()
                    .aircraft_carrier(common_value)
                    .auxiliary(common_value)
                    .battleship(common_value)
                    .cruiser(common_value)
                    .destroyer(common_value)
                    .submarine(common_value)
                    .name(modifier_name.to_owned())
                    .build()
            } else {
                let modifier_data = modifier_data
                    .dict_ref()
                    .expect("skill modifier data is not a dict");

                let modifier_data = modifier_data.inner();

                if modifier_data
                    .get(&HashableValue::String("AirCarrier".to_owned().into()))
                    .expect("could not get AirCarrier")
                    .is_i64()
                {
                    CrewSkillModifierBuilder::default()
                        .aircraft_carrier(
                            game_param_to_type!(modifier_data, "AirCarrier", i64) as f32
                        )
                        .auxiliary(game_param_to_type!(modifier_data, "Auxiliary", i64) as f32)
                        .battleship(game_param_to_type!(modifier_data, "Battleship", i64) as f32)
                        .cruiser(game_param_to_type!(modifier_data, "Cruiser", i64) as f32)
                        .destroyer(game_param_to_type!(modifier_data, "Destroyer", i64) as f32)
                        .submarine(game_param_to_type!(modifier_data, "Submarine", i64) as f32)
                        .name(modifier_name.to_owned())
                        .build()
                } else {
                    CrewSkillModifierBuilder::default()
                        .aircraft_carrier(game_param_to_type!(modifier_data, "AirCarrier", f32))
                        .auxiliary(game_param_to_type!(modifier_data, "Auxiliary", f32))
                        .battleship(game_param_to_type!(modifier_data, "Battleship", f32))
                        .cruiser(game_param_to_type!(modifier_data, "Cruiser", f32))
                        .destroyer(game_param_to_type!(modifier_data, "Destroyer", f32))
                        .submarine(game_param_to_type!(modifier_data, "Submarine", f32))
                        .name(modifier_name.to_owned())
                        .build()
                }
            };

            Some(modifier)
        })
        .collect()
}

fn build_crew_skills(
    skills: &BTreeMap<HashableValue, Value>,
) -> Result<Vec<CrewSkill>, CrewSkillBuilderError> {
    skills
        .iter()
        .filter_map(|(hashable_skill_name, skill_data)| {
            let skill_name = hashable_skill_name
                .string_ref()
                .expect("hashable_skill_name is not a String")
                .to_owned();

            let skill_name = skill_name.inner();

            if skill_data.is_none() {
                return None;
            }

            let skill_data = skill_data.dict_ref().expect("skill data is not dictionary");
            let skill_data = skill_data.inner();

            let _logic_modifiers =
                game_param_to_type!(skill_data, "modifiers", Option<HashMap<(), ()>>);

            let logic_modifiers = None;
            // logic_modifiers.map(|modifiers| {
            //     build_skill_modifiers(modifiers).expect("failed to build logic modifiers")
            // });

            let logic_trigger_data =
                game_param_to_type!(skill_data, "LogicTrigger", Option<HashMap<(), ()>>);

            let logic_trigger = logic_trigger_data.map(|logic_trigger_data| {
                let logic_trigger_data = logic_trigger_data.inner();
                CrewSkillLogicTriggerBuilder::default()
                    .burn_count(game_param_to_type!(
                        logic_trigger_data,
                        "burnCount",
                        Option<usize>
                    ))
                    .change_priority_target_penalty(game_param_to_type!(
                        logic_trigger_data,
                        "changePriorityTargetPenalty",
                        f32
                    ))
                    .consumable_type(game_param_to_type!(
                        logic_trigger_data,
                        "consumableType",
                        String
                    ))
                    .cooling_delay(game_param_to_type!(logic_trigger_data, "coolingDelay", f32))
                    .cooling_interpolator(Vec::default())
                    .divider_type(game_param_to_type!(
                        logic_trigger_data,
                        "dividerType",
                        Option<String>
                    ))
                    .divider_value(game_param_to_type!(
                        logic_trigger_data,
                        "dividerValue",
                        Option<f32>
                    ))
                    .duration(game_param_to_type!(logic_trigger_data, "duration", f32))
                    .energy_coeff(game_param_to_type!(logic_trigger_data, "energyCoeff", f32))
                    .flood_count(game_param_to_type!(
                        logic_trigger_data,
                        "floodCount",
                        Option<usize>
                    ))
                    .health_factor(game_param_to_type!(
                        logic_trigger_data,
                        "healthFactor",
                        Option<f32>
                    ))
                    .heat_interpolator(Vec::default())
                    .modifiers(logic_modifiers)
                    .trigger_desc_ids(game_param_to_type!(
                        logic_trigger_data,
                        "triggerDescIds",
                        String
                    ))
                    .trigger_type(game_param_to_type!(
                        logic_trigger_data,
                        "triggerType",
                        String
                    ))
                    .build()
                    .expect("failed to build logic trigger")
            });

            let _modifiers = game_param_to_type!(skill_data, "modifiers", Option<HashMap<(), ()>>);
            let modifiers = None;

            // modifiers.map(|modifiers| {
            //     build_skill_modifiers(modifiers).expect("failed to build skill modifiers")
            // });

            let tier_data = game_param_to_type!(skill_data, "tier", HashMap<(), ()>);
            let tier_data = tier_data.inner();
            let tier = CrewSkillTiersBuilder::default()
                .aircraft_carrier(game_param_to_type!(tier_data, "AirCarrier", usize))
                .auxiliary(game_param_to_type!(tier_data, "Auxiliary", usize))
                .battleship(game_param_to_type!(tier_data, "Battleship", usize))
                .cruiser(game_param_to_type!(tier_data, "Cruiser", usize))
                .destroyer(game_param_to_type!(tier_data, "Destroyer", usize))
                .submarine(game_param_to_type!(tier_data, "Submarine", usize))
                .build()
                .expect("failed to build skill tiers");

            Some(
                CrewSkillBuilder::default()
                    .internal_name(skill_name.to_owned())
                    .can_be_learned(game_param_to_type!(skill_data, "canBeLearned", bool))
                    .is_epic(game_param_to_type!(skill_data, "isEpic", bool))
                    .skill_type(game_param_to_type!(skill_data, "skillType", usize))
                    .ui_treat_as_trigger(game_param_to_type!(skill_data, "uiTreatAsTrigger", bool))
                    .tier(tier)
                    .modifiers(modifiers)
                    .logic_trigger(logic_trigger)
                    .build(),
            )
        })
        .collect()
}

fn build_crew_personality(
    personality: &BTreeMap<HashableValue, Value>,
) -> Result<CrewPersonality, CrewPersonalityBuilderError> {
    let ships = game_param_to_type!(personality, "ships", HashMap<(), ()>);
    let ships = ships.inner();
    let ships = CrewPersonalityShipsBuilder::default()
        .groups(
            game_param_to_type!(ships, "groups", &[()])
                .inner()
                .iter()
                .map(|value| {
                    value
                        .string_ref()
                        .expect("group entry is not a string")
                        .inner()
                        .to_owned()
                })
                .collect(),
        )
        .nation(
            game_param_to_type!(ships, "nation", &[()])
                .inner()
                .iter()
                .map(|value| {
                    value
                        .string_ref()
                        .expect("nation entry is not a string")
                        .inner()
                        .to_owned()
                })
                .collect(),
        )
        .peculiarity(
            game_param_to_type!(ships, "peculiarity", &[()])
                .inner()
                .iter()
                .map(|value| {
                    value
                        .string_ref()
                        .expect("peculiarity entry is not a string")
                        .inner()
                        .to_owned()
                })
                .collect(),
        )
        .ships(
            game_param_to_type!(ships, "ships", &[()])
                .inner()
                .iter()
                .map(|value| {
                    value
                        .string_ref()
                        .expect("ships entry is not a string")
                        .inner()
                        .to_owned()
                })
                .collect(),
        )
        .build()
        .expect("failed to build CrewPersonalityShips");

    CrewPersonalityBuilder::default()
        .can_reset_skills_for_free(game_param_to_type!(
            personality,
            "canResetSkillsForFree",
            bool
        ))
        .cost_credits(game_param_to_type!(personality, "costCR", usize))
        .cost_elite_xp(game_param_to_type!(personality, "costELXP", usize))
        .cost_gold(game_param_to_type!(personality, "costGold", usize))
        .cost_xp(game_param_to_type!(personality, "costXP", usize))
        .has_custom_background(game_param_to_type!(
            personality,
            "hasCustomBackground",
            bool
        ))
        .has_overlay(game_param_to_type!(personality, "hasOverlay", bool))
        .has_rank(game_param_to_type!(personality, "hasRank", bool))
        .has_sample_voiceover(game_param_to_type!(personality, "hasSampleVO", bool))
        .is_animated(game_param_to_type!(personality, "isAnimated", bool))
        .is_person(game_param_to_type!(personality, "isPerson", bool))
        .is_retrainable(game_param_to_type!(personality, "isRetrainable", bool))
        .is_unique(game_param_to_type!(personality, "isUnique", bool))
        .peculiarity(game_param_to_type!(personality, "peculiarity", String))
        .permissions(game_param_to_type!(personality, "permissions", u32))
        .person_name(game_param_to_type!(personality, "personName", String))
        .subnation(game_param_to_type!(personality, "subnation", String))
        .tags(
            game_param_to_type!(personality, "tags", &[()])
                .inner()
                .iter()
                .map(|value| {
                    value
                        .string_ref()
                        .expect("peculiarity entry is not a string")
                        .inner()
                        .to_owned()
                })
                .collect(),
        )
        .ships(ships)
        .build()
}

fn build_ability_category(
    category_data: &BTreeMap<HashableValue, Value>,
) -> Result<AbilityCategory, AbilityCategoryBuilderError> {
    let reload_time = if let Some(reload_time) =
        category_data.get(&HashableValue::String("reloadTime".to_owned().into()))
    {
        if let Some(reload_time) = reload_time.i64_ref() {
            *reload_time as f32
        } else {
            *reload_time.f64_ref().expect("workTime is not a f64") as f32
        }
    } else {
        panic!("could not get reloadTime");
    };

    let work_time = if let Some(work_time) =
        category_data.get(&HashableValue::String("workTime".to_owned().into()))
    {
        if let Some(work_time) = work_time.i64_ref() {
            *work_time as f32
        } else {
            *work_time.f64_ref().expect("workTime is not a f64") as f32
        }
    } else {
        panic!("could not get reloadTime");
    };

    AbilityCategoryBuilder::default()
        .special_sound_id(game_param_to_type!(
            category_data,
            "SpecialSoundID",
            Option<String>
        ))
        .consumable_type(game_param_to_type!(category_data, "consumableType", String))
        .description_id(game_param_to_type!(category_data, "descIDs", String))
        .group(game_param_to_type!(category_data, "group", String))
        .icon_id(game_param_to_type!(category_data, "iconIDs", String))
        .num_consumables(game_param_to_type!(category_data, "numConsumables", isize))
        .preparation_time(game_param_to_type!(category_data, "preparationTime", f32))
        .reload_time(reload_time)
        .title_id(game_param_to_type!(category_data, "titleIDs", String))
        .work_time(work_time)
        .build()
}

fn build_ability(
    ability_data: &BTreeMap<HashableValue, Value>,
) -> Result<Ability, AbilityBuilderError> {
    let test_key = HashableValue::String("numConsumables".to_string().into());
    let categories: HashMap<String, AbilityCategory> =
        HashMap::from_iter(ability_data.iter().filter_map(|(key, value)| {
            if value.is_not_dict() {
                return None;
            }

            let value = value.dict_ref().unwrap().inner();
            if value.contains_key(&test_key) {
                Some((
                    key.string_ref().unwrap().inner().to_owned(),
                    build_ability_category(&value).expect("failed to build ability category"),
                ))
            } else {
                None
            }
        }));

    AbilityBuilder::default()
        .can_buy(game_param_to_type!(ability_data, "canBuy", bool))
        .cost_credits(game_param_to_type!(ability_data, "costCR", isize))
        .cost_gold(game_param_to_type!(ability_data, "costGold", isize))
        .is_free(game_param_to_type!(ability_data, "freeOfCharge", bool))
        .categories(categories)
        .build()
}

fn build_ship(ship_data: &BTreeMap<HashableValue, Value>) -> Result<Vehicle, VehicleBuilderError> {
    let ability_data = game_param_to_type!(ship_data, "ShipAbilities", Option<HashMap<(), ()>>);
    let abilities: Option<Vec<Vec<(String, String)>>> = ability_data.map(|abilities_data| {
        abilities_data
            .inner()
            .iter()
            .filter_map(|(slot_name, slot_data)| {
                let _slot_name = slot_name
                    .string_ref()
                    .expect("ship ability slot name is not a string");
                if slot_data.is_none() {
                    return None;
                }

                let slot_data = slot_data.dict_ref().expect("slot data is not a dictionary");
                let slot_data = slot_data.inner();

                let slot = game_param_to_type!(slot_data, "slot", usize);
                let abils = game_param_to_type!(slot_data, "abils", &[()]).inner();
                let abils: Vec<(String, String)> = abils
                    .iter()
                    .map(|abil| {
                        let map_abil = |abil: &Vec<Value>| {
                            (
                                abil[0]
                                    .string_ref()
                                    .expect("abil[0] is not a string")
                                    .inner()
                                    .clone(),
                                abil[1]
                                    .string_ref()
                                    .expect("abil[1] is not a string")
                                    .inner()
                                    .clone(),
                            )
                        };
                        match abil {
                            Value::Tuple(inner) => map_abil(inner.inner()),
                            Value::List(inner) => map_abil(&inner.inner()),
                            _ => panic!("abil is not a list/tuple"),
                        }
                    })
                    .collect();

                Some((slot, abils))
            })
            .sorted_by(|a, b| a.0.cmp(&b.0))
            // drop the slot
            .map(|abil| abil.1)
            .collect()
    });

    let upgrade_data = game_param_to_type!(ship_data, "ShipUpgradeInfo", HashMap<(), ()>);
    let upgrades: Vec<String> = upgrade_data
        .inner()
        .iter()
        .map(|(upgrade_name, upgrade_data)| {
            upgrade_name
                .string_ref()
                .expect("upgrade name is not a string")
                .inner()
                .to_owned()
        })
        .collect();

    let level = game_param_to_type!(ship_data, "level", u32);
    let group = game_param_to_type!(ship_data, "group", String);

    VehicleBuilder::default()
        .level(level)
        .group(group)
        .abilities(abilities)
        .upgrades(upgrades)
        .build()
}

impl GameMetadataProvider {
    /// Loads game metadata directly from game files. This operation is fairly expensive
    /// considering `GameParams.data` must be deserialized and converted to a strongly-typed
    /// representation.
    ///
    /// See [`GameMetadataProvider::from_params`] if you wish to use caching.
    pub fn from_pkg(
        file_tree: &FileNode,
        pkg_loader: &PkgFileLoader,
    ) -> Result<GameMetadataProvider, ErrorKind> {
        debug!("deserializing gameparams");

        let game_params = file_tree.find("content/GameParams.data")?;
        let mut game_params_data = Vec::new();
        game_params.read_file(pkg_loader, &mut game_params_data)?;

        let pickled_params: Value = game_params_to_pickle(game_params_data)?;

        let params_dict = if let Some(params_dict) = pickled_params.dict_ref() {
            let params_dict = params_dict.inner();
            params_dict
                .get(&HashableValue::String("".to_string().into()))
                .expect("failed to get default game_params")
                .dict_ref()
                .expect("game params is not a dict")
                .clone()
        } else {
            let params_list = pickled_params
                .list_ref()
                .expect("Root game params is not a list")
                .inner();

            let params = &params_list[0];
            params
                .dict_ref()
                .expect("First element of GameParams is not a dictionary")
                .clone()
        };

        let new_params = params_dict
        .inner()
                .values()
                .filter_map(|param| {
                    if param.is_none() {
                        return None;
                    }

                    let param_data = param.dict_ref().expect("Params root level dictionary values are not dictionaries");
                    let param_data = param_data.inner();

                    param_data
                        .get(&HashableValue::String("typeinfo".to_string().into()))
                        .and_then(|type_info| {
                            type_info.dict_ref().and_then(|type_info_main| {
                                let type_info = type_info_main.inner();
                                let (nation, species, ty) = (
                                    type_info.get(&HashableValue::String("nation".to_string().into()))?,
                                    type_info.get(&HashableValue::String("species".to_string().into()))?,
                                    type_info.get(&HashableValue::String("type".to_string().into()))?,
                                );

                                let (Value::String(nation), Value::String(ty)) = (nation, ty) else {
                                    return None;
                                };


                                Some((nation.clone(), species.clone(), ty.clone()))
                            })
                        })
                        .and_then(|(nation, species, typ)| {
                            let param_type = ParamType::from_str(typ.inner().as_str()).ok()?;
                            let nation = nation.inner().clone();
                            let species = species.string_ref().and_then(|s| Species::from_str(s.inner().as_str()).ok());

                            let parsed_param_data = match param_type {
                                ParamType::Ship => Some(build_ship(&param_data).map(ParamData::Vehicle).expect("failed to build Vehicle")),
                                ParamType::Crew => {
                                    let money_training_level = game_param_to_type!(param_data, "moneyTrainingLevel", usize);

                                    let personality = game_param_to_type!(param_data, "CrewPersonality", HashMap<(), ()>);
                                    let personality = personality.inner();
                                    let crew_personality = build_crew_personality(&personality).expect("failed to build crew personality");

                                    let skills = game_param_to_type!(param_data, "Skills", Option<HashMap<(), ()>>);
                                    let skills = skills.map(|skills| build_crew_skills(&skills.inner()).expect("failed to build crew skills"));

                                    CrewBuilder::default()
                                        .money_training_level(money_training_level)
                                        .personality(crew_personality)
                                        .skills(skills)
                                        .build()
                                        .ok()
                                        .map(ParamData::Crew)
                                }
                                ParamType::Achievement => {
                                    let is_group = game_param_to_type!(param_data, "group", bool);
                                    let one_per_battle = game_param_to_type!(param_data, "onePerBattle", bool);
                                    let ui_type = game_param_to_type!(param_data, "uiType", String);
                                    let ui_name = game_param_to_type!(param_data, "uiName", String);

                                    AchievementBuilder::default()
                                        .is_group(is_group)
                                        .one_per_battle(one_per_battle)
                                        .ui_type(ui_type)
                                        .ui_name(ui_name)
                                        .build()
                                        .ok()
                                        .map(ParamData::Achievement)
                                }
                                ParamType::Ability => Some(build_ability(&param_data).map(ParamData::Ability).expect("failed to build Ability")),
                                ParamType::Exterior => Some(ParamData::Exterior),
                                ParamType::Modernization => Some(ParamData::Modernization),
                                ParamType::Unit => Some(ParamData::Unit),
                                ParamType::Aircraft => {
                                    let subtypes: Vec<String> = param_data
                                        .get(&HashableValue::String("planeSubtype".to_string().into()))
                                        .and_then(|v| v.list_ref())
                                        .map(|list| {
                                            list.inner().iter().filter_map(|item| {
                                                item.string_ref().map(|s| s.inner().to_string())
                                            }).collect()
                                        })
                                        .unwrap_or_default();
                                    let category = if subtypes.iter().any(|s| s == "airsupport") {
                                        PlaneCategory::Airsupport
                                    } else if subtypes.iter().any(|s| s == "consumable") {
                                        PlaneCategory::Consumable
                                    } else {
                                        PlaneCategory::Controllable
                                    };
                                    // Resolve ammo type: bombName -> projectile dict -> ammoType
                                    let ammo_type = param_data
                                        .get(&HashableValue::String("bombName".to_string().into()))
                                        .and_then(|v| v.string_ref())
                                        .filter(|s| !s.inner().is_empty())
                                        .and_then(|bomb_name| {
                                            params_dict.inner()
                                                .get(&HashableValue::String(bomb_name.clone()))
                                                .and_then(|proj| proj.dict_ref())
                                                .and_then(|proj_dict| {
                                                    proj_dict.inner()
                                                        .get(&HashableValue::String("ammoType".to_string().into()))
                                                        .and_then(|v| v.string_ref())
                                                        .map(|s| s.inner().to_string())
                                                })
                                        })
                                        .unwrap_or_default();
                                    AircraftBuilder::default()
                                        .category(category)
                                        .ammo_type(ammo_type)
                                        .build()
                                        .ok()
                                        .map(ParamData::Aircraft)
                                },
                                _ => None,
                            }?;

                            let id = *param_data
                                .get(&HashableValue::String("id".to_string().into()))
                                .expect("param has no id field")
                                .i64_ref()
                                .expect("param id is not an i64") as u32;

                            let index = param_data
                                .get(&HashableValue::String("index".to_string().into()))
                                .expect("param has no index field")
                                .string_ref()
                                .expect("param index is not a string")
                                .inner()
                                .clone();

                            let name = param_data
                                .get(&HashableValue::String("name".to_string().into()))
                                .expect("param has no name field")
                                .string_ref()
                                .expect("param name is not a string")
                                .inner()
                                .clone();

                            ParamBuilder::default()
                                .id(id)
                                .index(index)
                                .name(name)
                                .species(species)
                                .nation(nation)
                                .data(parsed_param_data)
                                .build()
                                .ok()
                        })
                })
                .collect::<Vec<Param>>();

        let params = new_params;

        Self::from_params(params, file_tree, pkg_loader)
    }

    /// Constructs a GameMetadataProvider from a pre-built list of GameParams. This may be useful for callers
    /// who wish to cache GameParams and avoid the cost of loading and converting directly from game PKG files.
    pub fn from_params(
        params: Vec<Param>,
        file_tree: &FileNode,
        pkg_loader: &PkgFileLoader,
    ) -> Result<GameMetadataProvider, ErrorKind> {
        let param_id_to_translation_id = HashMap::from_iter(
            params
                .iter()
                .map(|param| (param.id(), format!("IDS_{}", param.index()))),
        );

        let data_file_loader = DataFileWithCallback::new(|path| {
            debug!("requesting file: {path}");

            let path = Path::new(path);

            let mut file_data = Vec::new();
            file_tree
                .read_file_at_path(path, pkg_loader, &mut file_data)
                .expect("failed to read file");

            Ok(Cow::Owned(file_data))
        });

        let specs = Arc::new(parse_scripts(&data_file_loader).unwrap());

        Ok(GameMetadataProvider {
            params: params.into(),
            param_id_to_translation_id,
            translations: None,
            specs,
        })
    }

    /// Similar to [`Self::from_params`], but does not allow looking up specs. Useful for scenarios where you
    /// want to use utility functions for only game params.
    pub fn from_params_no_specs(params: Vec<Param>) -> Result<GameMetadataProvider, ErrorKind> {
        let param_id_to_translation_id = HashMap::from_iter(
            params
                .iter()
                .map(|param| (param.id(), format!("IDS_{}", param.index()))),
        );

        let specs = Arc::new(Vec::new());

        Ok(GameMetadataProvider {
            params: params.into(),
            param_id_to_translation_id,
            translations: None,
            specs,
        })
    }

    pub fn set_translations(&mut self, catalog: Catalog) {
        self.translations = Some(catalog);
    }

    pub fn param_localization_id(&self, ship_id: u32) -> Option<&str> {
        self.param_id_to_translation_id
            .get(&ship_id)
            .map(|s| s.as_str())
    }

    // pub fn get(&self, path: &str) -> Option<&pickled::Value> {
    //     let path_parts = path.split("/");
    //     let mut current = Some(&self.0);
    //     while let Some(pickled::Value::Dict(dict)) = current {

    //     }
    //     None
    // }
}
