/// Main logic for parsing the game's resource index files
pub mod idx;
/// Utilities for helping load and maintain `.pkg` files
pub mod pkg;
// File tree serialization utilities
pub mod serialization;

use std::borrow::Cow;

use crate::Rc;
use crate::error::ErrorKind;
use crate::game_params::types::Param;
use crate::rpc::entitydefs::EntitySpec;

pub trait ResourceLoader {
    fn localized_name_from_param(&self, param: &Param) -> Option<&str>;
    fn localized_name_from_id(&self, id: &str) -> Option<String>;
    fn game_param_by_id(&self, id: u32) -> Option<Rc<Param>>;
    fn entity_specs(&self) -> &[EntitySpec];
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
    pub build: u32,
}

impl Version {
    pub fn from_client_exe(version: &str) -> Version {
        let parts: Vec<_> = version.split(",").collect();
        assert!(parts.len() == 4);
        Version {
            major: parts[0].parse::<u32>().unwrap(),
            minor: parts[1].parse::<u32>().unwrap(),
            patch: parts[2].parse::<u32>().unwrap(),
            build: parts[3].parse::<u32>().unwrap(),
        }
    }

    pub fn to_path(&self) -> String {
        format!("{}.{}.{}", self.major, self.minor, self.patch)
    }

    pub fn is_at_least(&self, other: &Version) -> bool {
        if self.major > other.major {
            true
        } else if self.major < other.major {
            false
        } else if self.minor > other.minor {
            true
        } else if self.minor < other.minor {
            false
        } else {
            self.patch >= other.patch
        }
    }
}

pub trait DataFileLoader {
    fn get(&self, path: &str) -> Result<Cow<'static, [u8]>, ErrorKind>;
}

pub struct DataFileWithCallback<F> {
    callback: F,
}

impl<F> DataFileWithCallback<F>
where
    F: Fn(&str) -> Result<Cow<'static, [u8]>, ErrorKind>,
{
    pub fn new(callback: F) -> Self {
        Self { callback }
    }
}

impl<F> DataFileLoader for DataFileWithCallback<F>
where
    F: Fn(&str) -> Result<Cow<'static, [u8]>, ErrorKind>,
{
    fn get(&self, path: &str) -> Result<Cow<'static, [u8]>, ErrorKind> {
        (self.callback)(path)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    fn assert_older_newer(older: Version, newer: Version) {
        assert!(newer.is_at_least(&older));
        assert!(newer.is_at_least(&newer));
        assert!(!older.is_at_least(&newer));
    }

    #[test]
    fn different_patch() {
        let older = Version::from_client_exe("0,10,9,0");
        let newer = Version::from_client_exe("0,10,10,0");
        assert_older_newer(older, newer);
    }

    #[test]
    fn different_minor() {
        let older = Version::from_client_exe("0,10,9,0");
        let newer = Version::from_client_exe("0,11,0,0");
        assert_older_newer(older, newer);
    }

    #[test]
    fn different_major() {
        let older = Version::from_client_exe("0,11,5,0");
        let newer = Version::from_client_exe("1,0,0,0");
        assert_older_newer(older, newer);
    }
}
