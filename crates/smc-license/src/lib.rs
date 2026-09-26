pub mod license;
pub mod trial;

pub use license::{LicenseFile, LicenseStatus, verify_license};
pub use trial::TrialManager;
