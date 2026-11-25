pub mod archive;
pub mod error;
pub mod executor;
pub mod model;
pub mod ops;
pub mod package;
pub mod xml_util;

pub use archive::HwpxArchive;
pub use error::HwpxError;
pub use executor::{execute_hwpx_edit_plan, execute_hwpx_edit_plan_model};
pub use model::{HwpxDocument, HANCOM_NATIVE_STUB};
pub use package::is_valid_hwpx_package;

/// Embedded Hancom-verified blank HWPX template (D-06).
pub const BLANK_HWPX_TEMPLATE: &[u8] = include_bytes!("../../../assets/templates/blank.hwpx");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blank_template_is_valid_hwpx_package() {
        assert!(
            is_valid_hwpx_package(BLANK_HWPX_TEMPLATE),
            "embedded assets/templates/blank.hwpx must pass is_valid_hwpx_package"
        );
    }
}
