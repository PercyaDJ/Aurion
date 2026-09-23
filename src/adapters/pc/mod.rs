pub mod camera_mock;
pub mod storage_mock;
pub mod clock_mock;
pub mod network_mock;
pub mod system_mock;

pub use camera_mock::{CameraMock, SkyPattern};
pub use storage_mock::StorageMock;
pub use clock_mock::{ClockMock, ClockReal, TokioClock};
pub use network_mock::NetworkMock;
pub use system_mock::SystemMock;
