#![deny(unsafe_op_in_unsafe_fn)]

pub mod bioluminescence;
pub mod capture;
pub mod capture_session;
#[cfg(any(windows, test))]
mod destination;
pub mod fauna;
#[cfg(feature = "investigation")]
pub mod flora_appearance;
#[cfg(not(feature = "investigation"))]
mod flora_appearance;
#[cfg(feature = "investigation")]
pub mod flora_geometry;
#[cfg(not(feature = "investigation"))]
mod flora_geometry;
mod game_assets;
pub mod grass_appearance;
pub mod hook_code;
pub mod object_cache;
mod object_flags;
pub mod query;
pub mod region;
pub mod resource_hints;
pub mod snapshot;
pub mod terrain;
pub mod workers;

#[cfg(windows)]
mod hook_threads;
#[cfg(windows)]
pub mod planet_hook;
#[cfg(windows)]
pub mod resident_hook;

#[cfg(windows)]
pub mod query_hooks;
#[cfg(windows)]
mod registration_hook;

mod solar;

mod planet_query_context;

pub mod search;
#[cfg(any(windows, test))]
mod system_name;

#[cfg(all(windows, feature = "investigation"))]
mod investigation;

#[cfg(all(windows, feature = "investigation"))]
mod hook_probe;

#[cfg(all(windows, feature = "investigation"))]
mod persistent_probe;

pub mod batch;
#[cfg(all(windows, feature = "investigation"))]
mod filter_probe;
pub mod scheduler;
#[cfg(all(windows, feature = "investigation"))]
mod session_probe;

#[cfg(windows)]
pub mod game_state;
#[cfg(windows)]
pub mod gameplay_hook;
#[cfg(windows)]
mod guide_hook;
#[cfg(windows)]
pub mod identity;
#[cfg(windows)]
pub mod lifecycle_hook;
#[cfg(windows)]
pub mod platform;

#[cfg(windows)]
mod control_worker;
#[cfg(windows)]
pub mod overlay;

#[cfg(windows)]
pub mod navigation;
#[cfg(windows)]
mod navigation_hook;

#[cfg(windows)]
pub mod runtime;

#[cfg(all(windows, feature = "investigation"))]
mod lifecycle_probe;

#[cfg(all(windows, feature = "investigation"))]
mod runtime_probe;

mod colour_appearance;
pub mod colour_lut;
