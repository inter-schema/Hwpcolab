use std::io::{Cursor, Write};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

#[cfg(test)]
mod tests {
    use super::*;

    fn build_archive(entries: &[(&str, &str, CompressionMethod)]) -> Vec<u8> {
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        for (name, content, method) in entries {
            let options = SimpleFileOptions::default().compression_method(*method);
            writer.start_file(*name, options).unwrap();
            writer.write_all(content.as_bytes()).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    fn valid_entries() -> Vec<(&'static str, &'static str, CompressionMethod)> {
        vec![
            ("mimetype", "application/hwp+zip", CompressionMethod::Stored),
            ("version.xml", "<HCFVersion/>", CompressionMethod::Deflated),
            ("Contents/content.hpf", "<opf:package/>", CompressionMethod::Deflated),
            ("Contents/header.xml", "<hh:head/>", CompressionMethod::Deflated),
            ("Contents/section0.xml", "<hs:sec/>", CompressionMethod::Deflated),
        ]
    }

    #[test]
    fn accepts_a_valid_hwpx_package() {
        let bytes = build_archive(&valid_entries());
        assert!(crate::hwpx_package::is_valid_hwpx_package(&bytes));
    }

    #[test]
    fn rejects_generic_zip_archives() {
        let bytes = build_archive(&[("notes.txt", "not HWPX", CompressionMethod::Stored)]);
        assert!(!crate::hwpx_package::is_valid_hwpx_package(&bytes));
    }

    #[test]
    fn rejects_mimetype_that_is_not_first_or_stored() {
        let wrong_order = build_archive(&[
            ("version.xml", "<HCFVersion/>", CompressionMethod::Stored),
            ("mimetype", "application/hwp+zip", CompressionMethod::Stored),
            ("Contents/content.hpf", "<opf:package/>", CompressionMethod::Stored),
            ("Contents/header.xml", "<hh:head/>", CompressionMethod::Stored),
            ("Contents/section0.xml", "<hs:sec/>", CompressionMethod::Stored),
        ]);
        assert!(!crate::hwpx_package::is_valid_hwpx_package(&wrong_order));

        let compressed = build_archive(&[
            ("mimetype", "application/hwp+zip", CompressionMethod::Deflated),
            ("Contents/content.hpf", "<opf:package/>", CompressionMethod::Stored),
            ("Contents/header.xml", "<hh:head/>", CompressionMethod::Stored),
            ("Contents/section0.xml", "<hs:sec/>", CompressionMethod::Stored),
        ]);
        assert!(!crate::hwpx_package::is_valid_hwpx_package(&compressed));
    }

    #[test]
    fn rejects_packages_missing_required_document_parts() {
        let missing_content = build_archive(&[
            ("mimetype", "application/hwp+zip", CompressionMethod::Stored),
            ("Contents/header.xml", "<hh:head/>", CompressionMethod::Stored),
            ("Contents/section0.xml", "<hs:sec/>", CompressionMethod::Stored),
        ]);
        assert!(!crate::hwpx_package::is_valid_hwpx_package(&missing_content));

        let missing_header = build_archive(&[
            ("mimetype", "application/hwp+zip", CompressionMethod::Stored),
            ("Contents/content.hpf", "<opf:package/>", CompressionMethod::Stored),
            ("Contents/section0.xml", "<hs:sec/>", CompressionMethod::Stored),
        ]);
        assert!(!crate::hwpx_package::is_valid_hwpx_package(&missing_header));

        let missing_section = build_archive(&[
            ("mimetype", "application/hwp+zip", CompressionMethod::Stored),
            ("Contents/content.hpf", "<opf:package/>", CompressionMethod::Stored),
            ("Contents/header.xml", "<hh:head/>", CompressionMethod::Stored),
        ]);
        assert!(!crate::hwpx_package::is_valid_hwpx_package(&missing_section));
    }
}
