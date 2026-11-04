use std::collections::HashSet;
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

    if archive.is_empty() {
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
    let mut seen_names = HashSet::with_capacity(archive.len());

    for index in 0..archive.len() {
        let Ok(entry) = archive.by_index(index) else {
            return false;
        };
        let name = entry.name().to_owned();
        if !seen_names.insert(name.to_ascii_lowercase()) {
            return false;
        }
        has_content_package |= name == "Contents/content.hpf";
        has_header |= name == "Contents/header.xml";
        has_section |= is_section_part(&name);
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

    fn mark_entry_encrypted(mut bytes: Vec<u8>, target: &str) -> Vec<u8> {
        let eocd = bytes
            .windows(4)
            .rposition(|window| window == [0x50, 0x4b, 0x05, 0x06])
            .expect("EOCD signature");
        let entry_count = u16::from_le_bytes(bytes[eocd + 10..eocd + 12].try_into().unwrap());
        let mut cursor =
            u32::from_le_bytes(bytes[eocd + 16..eocd + 20].try_into().unwrap()) as usize;

        for _ in 0..entry_count {
            assert_eq!(&bytes[cursor..cursor + 4], &[0x50, 0x4b, 0x01, 0x02]);
            let name_length =
                u16::from_le_bytes(bytes[cursor + 28..cursor + 30].try_into().unwrap()) as usize;
            let extra_length =
                u16::from_le_bytes(bytes[cursor + 30..cursor + 32].try_into().unwrap()) as usize;
            let comment_length =
                u16::from_le_bytes(bytes[cursor + 32..cursor + 34].try_into().unwrap()) as usize;
            let name = &bytes[cursor + 46..cursor + 46 + name_length];

            if name == target.as_bytes() {
                let central_flags =
                    u16::from_le_bytes(bytes[cursor + 8..cursor + 10].try_into().unwrap());
                bytes[cursor + 8..cursor + 10]
                    .copy_from_slice(&(central_flags | 0x0001).to_le_bytes());
                let local_offset =
                    u32::from_le_bytes(bytes[cursor + 42..cursor + 46].try_into().unwrap())
                        as usize;
                let local_flags = u16::from_le_bytes(
                    bytes[local_offset + 6..local_offset + 8]
                        .try_into()
                        .unwrap(),
                );
                bytes[local_offset + 6..local_offset + 8]
                    .copy_from_slice(&(local_flags | 0x0001).to_le_bytes());
                return bytes;
            }

            cursor += 46 + name_length + extra_length + comment_length;
        }
        panic!("ZIP entry not found: {target}");
    }

    fn rename_entry(mut bytes: Vec<u8>, from: &str, to: &str) -> Vec<u8> {
        assert_eq!(from.len(), to.len());
        let eocd = bytes
            .windows(4)
            .rposition(|window| window == [0x50, 0x4b, 0x05, 0x06])
            .expect("EOCD signature");
        let entry_count = u16::from_le_bytes(bytes[eocd + 10..eocd + 12].try_into().unwrap());
        let mut cursor =
            u32::from_le_bytes(bytes[eocd + 16..eocd + 20].try_into().unwrap()) as usize;

        for _ in 0..entry_count {
            assert_eq!(&bytes[cursor..cursor + 4], &[0x50, 0x4b, 0x01, 0x02]);
            let name_length =
                u16::from_le_bytes(bytes[cursor + 28..cursor + 30].try_into().unwrap()) as usize;
            let extra_length =
                u16::from_le_bytes(bytes[cursor + 30..cursor + 32].try_into().unwrap()) as usize;
            let comment_length =
                u16::from_le_bytes(bytes[cursor + 32..cursor + 34].try_into().unwrap()) as usize;
            let matches = &bytes[cursor + 46..cursor + 46 + name_length] == from.as_bytes();

            if matches {
                let local_offset =
                    u32::from_le_bytes(bytes[cursor + 42..cursor + 46].try_into().unwrap())
                        as usize;
                let local_name_length = u16::from_le_bytes(
                    bytes[local_offset + 26..local_offset + 28]
                        .try_into()
                        .unwrap(),
                ) as usize;
                assert_eq!(local_name_length, to.len());
                bytes[cursor + 46..cursor + 46 + name_length].copy_from_slice(to.as_bytes());
                bytes[local_offset + 30..local_offset + 30 + local_name_length]
                    .copy_from_slice(to.as_bytes());
                return bytes;
            }

            cursor += 46 + name_length + extra_length + comment_length;
        }
        panic!("ZIP entry not found: {from}");
    }

    fn valid_entries() -> Vec<(&'static str, &'static str, CompressionMethod)> {
        vec![
            ("mimetype", HWPX_MIME_TYPE, CompressionMethod::Stored),
            ("version.xml", "<HCFVersion/>", CompressionMethod::Deflated),
            (
                "Contents/content.hpf",
                "<opf:package/>",
                CompressionMethod::Deflated,
            ),
            (
                "Contents/header.xml",
                "<hh:head/>",
                CompressionMethod::Deflated,
            ),
            (
                "Contents/section0.xml",
                "<hs:sec/>",
                CompressionMethod::Deflated,
            ),
            (
                "Contents/metadata.xml",
                "<meta/>",
                CompressionMethod::Deflated,
            ),
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
            (
                "Contents/content.hpf",
                "<opf:package/>",
                CompressionMethod::Stored,
            ),
            (
                "Contents/header.xml",
                "<hh:head/>",
                CompressionMethod::Stored,
            ),
            (
                "Contents/section0.xml",
                "<hs:sec/>",
                CompressionMethod::Stored,
            ),
        ]);
        assert!(!is_valid_hwpx_package(&wrong_order));

        let compressed = build_archive(&[
            ("mimetype", HWPX_MIME_TYPE, CompressionMethod::Deflated),
            (
                "Contents/content.hpf",
                "<opf:package/>",
                CompressionMethod::Stored,
            ),
            (
                "Contents/header.xml",
                "<hh:head/>",
                CompressionMethod::Stored,
            ),
            (
                "Contents/section0.xml",
                "<hs:sec/>",
                CompressionMethod::Stored,
            ),
        ]);
        assert!(!is_valid_hwpx_package(&compressed));
    }

    #[test]
    fn rejects_packages_missing_required_document_parts() {
        let missing_content = build_archive(&[
            ("mimetype", HWPX_MIME_TYPE, CompressionMethod::Stored),
            (
                "Contents/header.xml",
                "<hh:head/>",
                CompressionMethod::Stored,
            ),
            (
                "Contents/section0.xml",
                "<hs:sec/>",
                CompressionMethod::Stored,
            ),
        ]);
        assert!(!is_valid_hwpx_package(&missing_content));

        let missing_header = build_archive(&[
            ("mimetype", HWPX_MIME_TYPE, CompressionMethod::Stored),
            (
                "Contents/content.hpf",
                "<opf:package/>",
                CompressionMethod::Stored,
            ),
            (
                "Contents/section0.xml",
                "<hs:sec/>",
                CompressionMethod::Stored,
            ),
        ]);
        assert!(!is_valid_hwpx_package(&missing_header));

        let missing_section = build_archive(&[
            ("mimetype", HWPX_MIME_TYPE, CompressionMethod::Stored),
            (
                "Contents/content.hpf",
                "<opf:package/>",
                CompressionMethod::Stored,
            ),
            (
                "Contents/header.xml",
                "<hh:head/>",
                CompressionMethod::Stored,
            ),
        ]);
        assert!(!is_valid_hwpx_package(&missing_section));
    }
    #[test]
    fn accepts_repository_hwpx_samples() {
        let samples_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../samples");
        for sample in [
            "(날짜)(교시)(과목명)교수.학습과정안(이름).hwpx",
            "demo_official_report.hwpx",
        ] {
            let path = samples_dir.join(sample);
            let bytes = std::fs::read(&path)
                .unwrap_or_else(|error| panic!("could not read {}: {}", path.display(), error));
            assert!(
                is_valid_hwpx_package(&bytes),
                "repository HWPX sample was rejected: {}",
                path.display()
            );
        }
    }

    #[test]
    fn rejects_encrypted_document_parts() {
        let bytes = build_archive(&valid_entries());
        let encrypted = mark_entry_encrypted(bytes, "Contents/section0.xml");
        assert!(!is_valid_hwpx_package(&encrypted));
    }
    #[test]
    fn rejects_duplicate_package_part_names() {
        let bytes = build_archive(&valid_entries());
        let duplicated = rename_entry(bytes, "Contents/metadata.xml", "Contents/section0.xml");
        assert!(!is_valid_hwpx_package(&duplicated));
    }
}
