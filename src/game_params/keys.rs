//! Constants for GameParams pickle dictionary keys.
//!
//! These replace the hardcoded `HashableValue::String("...".to_string().into())`
//! patterns scattered throughout `provider.rs` and `main.rs`.

// Ship top-level keys
pub const SHIP_UPGRADE_INFO: &str = "ShipUpgradeInfo";
pub const SHIP_ABILITIES: &str = "ShipAbilities";
pub const A_HULL: &str = "A_Hull";

// Upgrade dict keys
pub const UC_TYPE: &str = "ucType";
pub const COMPONENTS: &str = "components";

// ucType values
pub const UC_TYPE_HULL: &str = "_Hull";
pub const UC_TYPE_ARTILLERY: &str = "_Artillery";
pub const UC_TYPE_TORPEDOES: &str = "_Torpedoes";

// Component type keys (inside "components" dict)
pub const COMP_HULL: &str = "hull";
pub const COMP_ARTILLERY: &str = "artillery";
pub const COMP_ATBA: &str = "atba";
pub const COMP_AIR_DEFENSE: &str = "airDefense";
pub const COMP_DIRECTORS: &str = "directors";
pub const COMP_FINDERS: &str = "finders";
pub const COMP_RADARS: &str = "radars";
pub const COMP_TORPEDOES: &str = "torpedoes";

/// All component type keys.
pub const ALL_COMPONENT_TYPES: &[&str] = &[
    COMP_HULL,
    COMP_ARTILLERY,
    COMP_ATBA,
    COMP_AIR_DEFENSE,
    COMP_DIRECTORS,
    COMP_FINDERS,
    COMP_RADARS,
    COMP_TORPEDOES,
];

/// Component types that have 3D models (excludes hull — hull is the parent model).
pub const MODEL_COMPONENT_TYPES: &[&str] = &[
    COMP_ARTILLERY,
    COMP_ATBA,
    COMP_AIR_DEFENSE,
    COMP_DIRECTORS,
    COMP_FINDERS,
    COMP_RADARS,
    COMP_TORPEDOES,
];

// Data field keys
pub const MODEL: &str = "model";
pub const ARMOR: &str = "armor";
pub const HIT_LOCATION_GROUPS: &str = "hitLocationGroups";
pub const HL_TYPE: &str = "hlType";
pub const MAX_HP: &str = "maxHP";
pub const REGENERATED_HP_PART: &str = "regeneratedHPPart";
pub const SPLASH_BOXES: &str = "splashBoxes";
pub const THICKNESS: &str = "thickness";
pub const DRAFT: &str = "draft";
pub const VISIBILITY_FACTOR: &str = "visibilityFactor";
pub const VISIBILITY_FACTOR_BY_PLANE: &str = "visibilityFactorByPlane";
pub const MAX_DIST: &str = "maxDist";
pub const AMMO_LIST: &str = "ammoList";
pub const CAMOUFLAGE: &str = "camouflage";
pub const PERMOFLAGES: &str = "permoflages";
pub const TITLE: &str = "title";

// HP_ mount prefix
pub const HP_PREFIX: &str = "HP_";

// typeinfo keys
pub const TYPEINFO: &str = "typeinfo";
pub const TYPEINFO_TYPE: &str = "type";
pub const TYPEINFO_NATION: &str = "nation";
pub const TYPEINFO_SPECIES: &str = "species";

// Param identity keys
pub const PARAM_ID: &str = "id";
pub const PARAM_INDEX: &str = "index";
pub const PARAM_NAME: &str = "name";
