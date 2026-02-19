// RPi adapters — only compiled with `--features rpi`

#[cfg(feature = "rpi")]
pub mod camera_rpi;
#[cfg(feature = "rpi")]
pub mod storage_rpi;
#[cfg(feature = "rpi")]
pub mod network_rpi;
#[cfg(feature = "rpi")]
pub mod system_rpi;

#[cfg(feature = "rpi")]
pub use camera_rpi::CameraRpi;
#[cfg(feature = "rpi")]
pub use storage_rpi::StorageRpi;
#[cfg(feature = "rpi")]
pub use network_rpi::NetworkRpi;
#[cfg(feature = "rpi")]
pub use system_rpi::SystemRpi;
