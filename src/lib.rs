/// Utilities for interacting with the game's data files
pub mod data;
/// Error definitions
pub mod error;
/// Utilities for interacting with the `content/GameParams.data` file
pub mod game_params;
/// Utilities involving the game's RPC functions -- useful for parsing entity defs and RPC definitions.
pub mod rpc;

#[cfg(feature = "arc")]
pub type Rc<T> = std::sync::Arc<T>;

#[cfg(not(feature = "arc"))]
pub type Rc<T> = std::rc::Rc<T>;
