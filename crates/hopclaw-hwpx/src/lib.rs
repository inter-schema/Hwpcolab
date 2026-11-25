pub mod package;

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
