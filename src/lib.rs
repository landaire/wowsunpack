/// Utilities for interacting with the `content/GameParams.data` file
pub mod game_params;
/// Main logic for parsing the game's resource index files
pub mod idx;
/// Utilities for helping load and maintain `.pkg` files
pub mod pkg;
/// Utilities for assisting with serializing game resource metadata
pub mod serialization;

#[cfg(feature = "arc")]
pub type Rc<T> = std::sync::Arc<T>;
#[cfg(not(feature = "arc"))]
pub type Rc<T> = std::rc::Rc<T>;
