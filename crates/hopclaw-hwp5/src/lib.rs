//! Pure-Rust HWP 5.0 binary container, record codec, and structure mutator.

pub mod cfb_container;
pub mod docinfo;
pub mod error;
pub mod executor;
pub mod layout;
pub mod objects;
pub mod record;
pub mod section;
pub mod style;
pub mod table;
pub mod text;

pub use cfb_container::{deflate_raw_level9, inflate_raw, FileHeader, Hwp5Container};
pub use docinfo::{
    id_slot, parse_hex_color, rgb_to_bgr, BorderFillBuilder, BorderLineType, BorderSide,
    CharShapePatch, DiagonalDirection, DocInfoDoc, LineSpacingType, ParaAlignment, ParaShapePatch,
};
pub use error::{Hwp5Error, Result};
pub use executor::{
    error_to_model, execute_hwp5_plan, inspect_hwp5, reject_unsupported_unified_op,
    Hwp5ExecOutput, Hwp5Inspector, BLANK_HWP_TEMPLATE,
};
pub use record::{parse_records, serialize_records, subtree_end, tag, RawRecord};
pub use section::{
    decode_para_text, encode_para_text, is_extended_ctrl_wchar, parse_para_char_shapes,
    serialize_para_char_shapes, CharShapeRun, DecodedParaText, PageDef, ParaHeaderMut,
    ParaHeaderRef, SectionDoc, SectionTableCellInfo, SectionTableInfo, TableCellAttr,
    HWPUNIT_PER_MM, PARA_LAST_FLAG,
};
