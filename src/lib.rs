pub mod core;
pub mod ports;
pub mod adapters;
pub mod web;
pub mod cli;
pub mod sys;

/// Version shown everywhere. Development builds published by the CI carry
/// their commit (`AURION_BUILD=1.8.0-edge.1a2b3c4`), releases the plain
/// Cargo version.
pub const VERSION: &str = match option_env!("AURION_BUILD") {
    Some(v) => v,
    None => env!("CARGO_PKG_VERSION"),
};
