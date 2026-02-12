/// Utilities for interacting with the game's data files
pub mod data;
/// Error definitions
pub mod error;
/// Utilities for interacting with the `content/GameParams.data` file
pub mod game_params;
/// Utilities involving the game's RPC functions -- useful for parsing entity defs and RPC definitions.
pub mod rpc;
/// Constants parsed from the game's XML files in `res/gui/data/constants/`
pub mod game_constants;
/// Utilities for loading game resources from a WoWS installation directory.
pub mod game_data;

#[cfg(feature = "arc")]
pub type Rc<T> = std::sync::Arc<T>;

#[cfg(not(feature = "arc"))]
pub type Rc<T> = std::rc::Rc<T>;
