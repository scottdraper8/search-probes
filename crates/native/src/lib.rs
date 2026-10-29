#![deny(unsafe_op_in_unsafe_fn)]

pub mod arena;
pub mod arena_inputs;
pub mod batch;
pub mod bioluminescence;
pub mod capture;
pub mod capture_session;
mod colour_appearance;
pub mod colour_lut;
#[cfg(windows)]
mod control_worker;
#[cfg(windows)]
mod destination;
#[cfg(windows)]
mod dispatch_gate;
pub mod fauna;
pub mod fauna_model_scalars;
mod flora_appearance;
mod flora_geometry;
mod game_assets;
#[cfg(windows)]
pub mod game_state;
#[cfg(windows)]
pub mod gameplay_hook;
pub mod grass_appearance;
#[cfg(windows)]
mod guide_hook;
pub mod hook_code;
#[cfg(windows)]
mod hook_threads;
#[cfg(windows)]
pub mod identity;
#[cfg(windows)]
pub mod lifecycle_hook;
#[cfg(windows)]
pub mod navigation;
#[cfg(windows)]
mod navigation_hook;
pub mod object_cache;
mod object_flags;
#[cfg(windows)]
pub mod overlay;
#[cfg(windows)]
pub mod planet_hook;
mod planet_query_context;
#[cfg(windows)]
pub mod platform;
pub mod query;
#[cfg(windows)]
pub mod query_hooks;
pub mod region;
#[cfg(windows)]
mod registration_hook;
#[cfg(windows)]
pub mod resident_hook;
pub mod resource_hints;
#[cfg(windows)]
pub mod runtime;
pub mod scheduler;
pub mod search;
pub mod snapshot;
mod solar;
#[cfg(windows)]
mod source_shutdown_hook;
#[cfg(windows)]
mod system_name;
pub mod terrain;
pub mod workers;
