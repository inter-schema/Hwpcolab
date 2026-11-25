//! Unit conversion utilities for HWPUNIT (`1 pt = 100 HWPUNIT`, `1 mm = 7200/25.4 ≈ 283.465 HWPUNIT`).

/// Integer HWPUNIT used across HWP 5.0 records and OWPML XML attributes.
pub type HwpUnit = i32;

/// Exact HWPUNIT per millimeter (`7200 / 25.4 ≈ 283.4645669`).
pub const HWPUNIT_PER_MM: f64 = 7200.0 / 25.4;

/// Approximate HWPUNIT per millimeter constant used by JS layout inspector (`283.465`).
pub const HWPUNIT_PER_MM_APPROX: f64 = 283.465;

/// Exact HWPUNIT per typographic point (`1 pt = 100 HWPUNIT`).
pub const HWPUNIT_PER_PT: f64 = 100.0;

/// Exact HWPUNIT per CSS pixel (`96 dpi`, `7200 / 96 = 75`).
pub const HWPUNIT_PER_PX: f64 = 75.0;

/// Convert millimeters to rounded `HwpUnit`.
#[inline]
pub fn mm_to_hwpunit(mm: f64) -> HwpUnit {
    (mm * HWPUNIT_PER_MM).round() as HwpUnit
}

/// Convert typographic points to rounded `HwpUnit`.
#[inline]
pub fn pt_to_hwpunit(pt: f64) -> HwpUnit {
    (pt * HWPUNIT_PER_PT).round() as HwpUnit
}

/// Convert `HwpUnit` back to millimeters.
#[inline]
pub fn hwpunit_to_mm(hu: HwpUnit) -> f64 {
    (hu as f64) / HWPUNIT_PER_MM
}

/// Convert `HwpUnit` back to typographic points.
#[inline]
pub fn hwpunit_to_pt(hu: HwpUnit) -> f64 {
    (hu as f64) / HWPUNIT_PER_PT
}
