use std::io::{Cursor, Read};

const HWPX_MIME_TYPE: &str = "application/hwp+zip";

/// Validate the OWPML ZIP envelope without extracting document payloads.
/// A generic ZIP, malformed archive, or package with missing core parts is
/// not treated as HWPX.
pub fn is_valid_hwpx_package(bytes: &[u8]) -> bool {
    let cursor = Cursor::new(bytes);
    let Ok(mut archive) = zip::ZipArchive::new(cursor) else {
        return false;
    };

    if archive.len() == 0 {
        return false;
    }

    let mime_type = {
        let Ok(mut entry) = archive.by_index(0) else {
            return false;
        };
        if entry.name() != "mimetype"
            || entry.name_raw() != b"mimetype"
            || entry.header_start() != 0
            || entry.compression() != zip::CompressionMethod::Stored
            || entry.encrypted()
            || entry.size() != HWPX_MIME_TYPE.len() as u64
        {
            return false;
        }
        let mut value = String::new();
        if entry.read_to_string(&mut value).is_err() {
            return false;
        }
        value
    };

    if mime_type != HWPX_MIME_TYPE {
        return false;
    }

    let mut has_content_package = false;
    let mut has_header = false;
    let mut has_section = false;

    for index in 0..archive.len() {
        let Ok(entry) = archive.by_index(index) else {
            return false;
        };
        let name = entry.name();
        has_content_package |= name == "Contents/content.hpf";
        has_header |= name == "Contents/header.xml";
        has_section |= is_section_part(name);
    }

    has_content_package && has_header && has_section
}

fn is_section_part(name: &str) -> bool {
    let Some(number) = name
        .strip_prefix("Contents/section")
        .and_then(|value| value.strip_suffix(".xml"))
    else {
        return false;
    };
    !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::write::SimpleFileOptions;
    use zip::{CompressionMethod, ZipWriter};

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
            ("mimetype", HWPX_MIME_TYPE, CompressionMethod::Stored),
            ("version.xml", "<HCFVersion/>", CompressionMethod::Deflated),
            ("Contents/content.hpf", "<opf:package/>", CompressionMethod::Deflated),
            ("Contents/header.xml", "<hh:head/>", CompressionMethod::Deflated),
            ("Contents/section0.xml", "<hs:sec/>", CompressionMethod::Deflated),
        ]
    }

    #[test]
    fn accepts_a_valid_hwpx_package() {
        assert!(is_valid_hwpx_package(&build_archive(&valid_entries())));
    }

    #[test]
    fn rejects_generic_zip_archives() {
        let bytes = build_archive(&[("notes.txt", "not HWPX", CompressionMethod::Stored)]);
        assert!(!is_valid_hwpx_package(&bytes));
    }

    #[test]
    fn rejects_mimetype_that_is_not_first_or_stored() {
        let wrong_order = build_archive(&[
            ("version.xml", "<HCFVersion/>", CompressionMethod::Stored),
            ("mimetype", HWPX_MIME_TYPE, CompressionMethod::Stored),
            ("Contents/content.hpf", "<opf:package/>", CompressionMethod::Stored),
            ("Contents/header.xml", "<hh:head/>", CompressionMethod::Stored),
            ("Contents/section0.xml", "<hs:sec/>", CompressionMethod::Stored),
        ]);
        assert!(!is_valid_hwpx_package(&wrong_order));

        let compressed = build_archive(&[
            ("mimetype", HWPX_MIME_TYPE, CompressionMethod::Deflated),
            ("Contents/content.hpf", "<opf:package/>", CompressionMethod::Stored),
            ("Contents/header.xml", "<hh:head/>", CompressionMethod::Stored),
            ("Contents/section0.xml", "<hs:sec/>", CompressionMethod::Stored),
        ]);
        assert!(!is_valid_hwpx_package(&compressed));
    }

    #[test]
    fn rejects_packages_missing_required_document_parts() {
        let missing_content = build_archive(&[
            ("mimetype", HWPX_MIME_TYPE, CompressionMethod::Stored),
            ("Contents/header.xml", "<hh:head/>", CompressionMethod::Stored),
            ("Contents/section0.xml", "<hs:sec/>", CompressionMethod::Stored),
        ]);
        assert!(!is_valid_hwpx_package(&missing_content));

        let missing_header = build_archive(&[
            ("mimetype", HWPX_MIME_TYPE, CompressionMethod::Stored),
            ("Contents/content.hpf", "<opf:package/>", CompressionMethod::Stored),
            ("Contents/section0.xml", "<hs:sec/>", CompressionMethod::Stored),
        ]);
        assert!(!is_valid_hwpx_package(&missing_header));

        let missing_section = build_archive(&[
            ("mimetype", HWPX_MIME_TYPE, CompressionMethod::Stored),
            ("Contents/content.hpf", "<opf:package/>", CompressionMethod::Stored),
            ("Contents/header.xml", "<hh:head/>", CompressionMethod::Stored),
        ]);
        assert!(!is_valid_hwpx_package(&missing_section));
    }
}
