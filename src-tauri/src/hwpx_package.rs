use std::collections::HashSet;
use std::io::{Cursor, Read};

const LOCAL_FILE_HEADER_SIGNATURE: &[u8; 4] = b"PK\x03\x04";
const CENTRAL_DIRECTORY_HEADER_SIGNATURE: &[u8; 4] = b"PK\x01\x02";
const END_OF_CENTRAL_DIRECTORY_SIGNATURE: &[u8; 4] = b"PK\x05\x06";
const ZIP64_END_OF_CENTRAL_DIRECTORY_SIGNATURE: &[u8; 4] = b"PK\x06\x06";
const ZIP64_END_OF_CENTRAL_DIRECTORY_LOCATOR_SIGNATURE: &[u8; 4] = b"PK\x06\x07";
const HWPX_MIME_TYPE: &str = "application/hwp+zip";
const ZIP_EOCD_MIN_LENGTH: usize = 22;
const ZIP_MAX_COMMENT_LENGTH: usize = u16::MAX as usize;

#[derive(Debug)]
struct CentralDirectoryEntry {
    name: Vec<u8>,
    flags: u16,
    method: u16,
    crc32: u32,
    compressed_size: u32,
    uncompressed_size: u32,
    local_header_offset: u32,
}

struct ClassicZipDirectory {
    entries: Vec<CentralDirectoryEntry>,
    central_directory_offset: usize,
}

fn read_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    let end = offset.checked_add(2)?;
    Some(u16::from_le_bytes(bytes.get(offset..end)?.try_into().ok()?))
}

fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    let end = offset.checked_add(4)?;
    Some(u32::from_le_bytes(bytes.get(offset..end)?.try_into().ok()?))
}

fn find_end_of_central_directory(bytes: &[u8]) -> Option<usize> {
    let last_possible_offset = bytes.len().checked_sub(ZIP_EOCD_MIN_LENGTH)?;
    let first_possible_offset = bytes
        .len()
        .saturating_sub(ZIP_EOCD_MIN_LENGTH + ZIP_MAX_COMMENT_LENGTH);

    for offset in (first_possible_offset..=last_possible_offset).rev() {
        if bytes.get(offset..offset + 4) != Some(END_OF_CENTRAL_DIRECTORY_SIGNATURE.as_slice()) {
            continue;
        }
        let comment_length = read_u16(bytes, offset + 20)? as usize;
        if offset
            .checked_add(ZIP_EOCD_MIN_LENGTH)?
            .checked_add(comment_length)?
            == bytes.len()
        {
            return Some(offset);
        }
    }
    None
}

fn has_zip64_locator(bytes: &[u8], eocd_offset: usize) -> bool {
    let Some(locator_offset) = eocd_offset.checked_sub(20) else {
        return false;
    };
    if bytes.get(locator_offset..locator_offset + 4)
        != Some(ZIP64_END_OF_CENTRAL_DIRECTORY_LOCATOR_SIGNATURE.as_slice())
    {
        return false;
    }

    let Some(disk_with_zip64_eocd) = read_u32(bytes, locator_offset + 4) else {
        return false;
    };
    let Some(zip64_eocd_offset_low) = read_u32(bytes, locator_offset + 8) else {
        return false;
    };
    let Some(zip64_eocd_offset_high) = read_u32(bytes, locator_offset + 12) else {
        return false;
    };
    let Some(total_disks) = read_u32(bytes, locator_offset + 16) else {
        return false;
    };

    if disk_with_zip64_eocd != 0 || total_disks != 1 {
        return false;
    }
    if zip64_eocd_offset_high != 0 {
        return true;
    }

    let zip64_eocd_offset = zip64_eocd_offset_low as usize;
    if zip64_eocd_offset
        .checked_add(56)
        .is_none_or(|end| end > locator_offset)
        || bytes.get(zip64_eocd_offset..zip64_eocd_offset + 4)
            != Some(ZIP64_END_OF_CENTRAL_DIRECTORY_SIGNATURE.as_slice())
    {
        return false;
    }

    let Some(record_size_low) = read_u32(bytes, zip64_eocd_offset + 4) else {
        return false;
    };
    let Some(record_size_high) = read_u32(bytes, zip64_eocd_offset + 8) else {
        return false;
    };
    if record_size_high != 0 || record_size_low < 44 {
        return false;
    }

    zip64_eocd_offset
        .checked_add(12)
        .and_then(|start| start.checked_add(record_size_low as usize))
        .is_some_and(|end| end <= locator_offset)
}

/// Inspect classic ZIP central-directory records before using ZipArchive.
/// zip 8.x indexes entries by name, so duplicate names can disappear from its
/// public index. Reject ZIP64 and malformed directory layouts rather than
/// silently falling back to that lossy view.
fn inspect_classic_zip_directory(bytes: &[u8]) -> Option<ClassicZipDirectory> {
    let eocd_offset = find_end_of_central_directory(bytes)?;
    // A valid ZIP64 locator is placed immediately before the classic EOCD,
    // even when the classic fields happen to fit. Verify its structure so a
    // coincidental signature in a ZIP comment is not mistaken for a locator.
    if has_zip64_locator(bytes, eocd_offset) {
        return None;
    }
    let disk_number = read_u16(bytes, eocd_offset + 4)?;
    let central_directory_disk = read_u16(bytes, eocd_offset + 6)?;
    let entries_on_disk = read_u16(bytes, eocd_offset + 8)?;
    let total_entries = read_u16(bytes, eocd_offset + 10)?;
    let central_directory_size = read_u32(bytes, eocd_offset + 12)?;
    let central_directory_offset = read_u32(bytes, eocd_offset + 16)?;

    if disk_number != 0
        || central_directory_disk != 0
        || entries_on_disk != total_entries
        || total_entries == 0
        || total_entries == u16::MAX
        || central_directory_size == u32::MAX
        || central_directory_offset == u32::MAX
    {
        return None;
    }

    let central_directory_offset = central_directory_offset as usize;
    let central_directory_end =
        central_directory_offset.checked_add(central_directory_size as usize)?;
    if central_directory_end > eocd_offset {
        return None;
    }

    let mut entries = Vec::with_capacity(total_entries as usize);
    let mut seen_names = HashSet::with_capacity(total_entries as usize);
    let mut cursor = central_directory_offset;

    for _ in 0..total_entries {
        let fixed_header_end = cursor.checked_add(46)?;
        if fixed_header_end > central_directory_end
            || bytes.get(cursor..cursor + 4) != Some(CENTRAL_DIRECTORY_HEADER_SIGNATURE.as_slice())
        {
            return None;
        }

        let flags = read_u16(bytes, cursor + 8)?;
        let method = read_u16(bytes, cursor + 10)?;
        let crc32 = read_u32(bytes, cursor + 16)?;
        let compressed_size = read_u32(bytes, cursor + 20)?;
        let uncompressed_size = read_u32(bytes, cursor + 24)?;
        let name_length = read_u16(bytes, cursor + 28)? as usize;
        let extra_length = read_u16(bytes, cursor + 30)? as usize;
        let comment_length = read_u16(bytes, cursor + 32)? as usize;
        let start_disk = read_u16(bytes, cursor + 34)?;
        let local_header_offset = read_u32(bytes, cursor + 42)?;
        let record_end = fixed_header_end
            .checked_add(name_length)?
            .checked_add(extra_length)?
            .checked_add(comment_length)?;

        if record_end > central_directory_end
            || name_length == 0
            || start_disk != 0
            || (flags & 0x0041) != 0
            || compressed_size == u32::MAX
            || uncompressed_size == u32::MAX
            || local_header_offset == u32::MAX
        {
            return None;
        }

        let name_start = fixed_header_end;
        let name_end = name_start.checked_add(name_length)?;
        let name = bytes.get(name_start..name_end)?.to_vec();
        if !name.is_ascii() && (flags & 0x0800) == 0 {
            return None;
        }
        std::str::from_utf8(&name).ok()?;
        let normalized_name: Vec<u8> = name.iter().map(|byte| byte.to_ascii_lowercase()).collect();
        if !seen_names.insert(normalized_name) {
            return None;
        }

        entries.push(CentralDirectoryEntry {
            name,
            flags,
            method,
            crc32,
            compressed_size,
            uncompressed_size,
            local_header_offset,
        });
        cursor = record_end;
    }

    if cursor != central_directory_end {
        return None;
    }

    Some(ClassicZipDirectory {
        entries,
        central_directory_offset,
    })
}

fn has_valid_first_mimetype_local_header(
    bytes: &[u8],
    entry: &CentralDirectoryEntry,
    central_directory_offset: usize,
) -> bool {
    if entry.name.as_slice() != b"mimetype".as_slice()
        || entry.local_header_offset != 0
        || entry.method != 0
        || (entry.flags & 0x0049) != 0
        || entry.compressed_size != HWPX_MIME_TYPE.len() as u32
        || entry.uncompressed_size != HWPX_MIME_TYPE.len() as u32
    {
        return false;
    }

    let local_offset = entry.local_header_offset as usize;
    if bytes.get(local_offset..local_offset + 4) != Some(LOCAL_FILE_HEADER_SIGNATURE.as_slice()) {
        return false;
    }

    let Some(local_flags) = read_u16(bytes, local_offset + 6) else {
        return false;
    };
    let Some(local_method) = read_u16(bytes, local_offset + 8) else {
        return false;
    };
    let Some(local_crc32) = read_u32(bytes, local_offset + 14) else {
        return false;
    };
    let Some(local_compressed_size) = read_u32(bytes, local_offset + 18) else {
        return false;
    };
    let Some(local_uncompressed_size) = read_u32(bytes, local_offset + 22) else {
        return false;
    };
    let Some(local_name_length) = read_u16(bytes, local_offset + 26) else {
        return false;
    };
    let Some(local_extra_length) = read_u16(bytes, local_offset + 28) else {
        return false;
    };

    let local_name_start = local_offset + 30;
    let Some(local_name_end) = local_name_start.checked_add(local_name_length as usize) else {
        return false;
    };
    let Some(local_data_start) = local_name_end.checked_add(local_extra_length as usize) else {
        return false;
    };
    let Some(local_data_end) = local_data_start.checked_add(entry.compressed_size as usize) else {
        return false;
    };

    local_data_end <= bytes.len()
        && local_data_end <= central_directory_offset
        && local_flags == entry.flags
        && local_method == entry.method
        && local_crc32 == entry.crc32
        && local_compressed_size == entry.compressed_size
        && local_uncompressed_size == entry.uncompressed_size
        && bytes.get(local_name_start..local_name_end) == Some(b"mimetype".as_slice())
        && bytes.get(local_data_start..local_data_end) == Some(HWPX_MIME_TYPE.as_bytes())
}

/// Validate the OWPML ZIP envelope without extracting document payloads.
/// A generic ZIP, malformed archive, or package with missing core parts is
/// not treated as HWPX.
pub fn is_valid_hwpx_package(bytes: &[u8]) -> bool {
    let Some(directory) = inspect_classic_zip_directory(bytes) else {
        return false;
    };
    let Some(first_entry) = directory.entries.first() else {
        return false;
    };
    if !has_valid_first_mimetype_local_header(
        bytes,
        first_entry,
        directory.central_directory_offset,
    ) {
        return false;
    }

    let cursor = Cursor::new(bytes);
    let Ok(mut archive) = zip::ZipArchive::new(cursor) else {
        return false;
    };

    if archive.is_empty() || archive.len() != directory.entries.len() {
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
    let mut seen_names = HashSet::with_capacity(directory.entries.len());

    for index in 0..archive.len() {
        let Ok(entry) = archive.by_index(index) else {
            return false;
        };
        let name = entry.name().to_owned();
        if !seen_names.insert(name.to_lowercase()) {
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

    fn find_eocd_offset(bytes: &[u8]) -> usize {
        assert!(bytes.len() >= 22, "ZIP is too short for EOCD");
        let start = bytes.len().saturating_sub(22 + u16::MAX as usize);
        let end = bytes.len() - 22;
        (start..=end)
            .rev()
            .find(|offset| {
                if bytes[*offset..*offset + 4] != [0x50, 0x4b, 0x05, 0x06] {
                    return false;
                }
                let comment_length =
                    u16::from_le_bytes(bytes[*offset + 20..*offset + 22].try_into().unwrap())
                        as usize;
                *offset + 22 + comment_length == bytes.len()
            })
            .expect("EOCD signature")
    }

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
        let eocd = find_eocd_offset(&bytes);
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

    fn mark_entry_utf8(mut bytes: Vec<u8>, target: &str) -> Vec<u8> {
        let eocd = find_eocd_offset(&bytes);
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
            if &bytes[cursor + 46..cursor + 46 + name_length] == target.as_bytes() {
                let flags = u16::from_le_bytes(bytes[cursor + 8..cursor + 10].try_into().unwrap());
                bytes[cursor + 8..cursor + 10].copy_from_slice(&(flags | 0x0800).to_le_bytes());
                let local_offset =
                    u32::from_le_bytes(bytes[cursor + 42..cursor + 46].try_into().unwrap())
                        as usize;
                let local_flags = u16::from_le_bytes(
                    bytes[local_offset + 6..local_offset + 8]
                        .try_into()
                        .unwrap(),
                );
                bytes[local_offset + 6..local_offset + 8]
                    .copy_from_slice(&(local_flags | 0x0800).to_le_bytes());
                return bytes;
            }
            cursor += 46 + name_length + extra_length + comment_length;
        }
        panic!("ZIP entry not found: {target}");
    }

    fn add_data_descriptor_to_mimetype(mut bytes: Vec<u8>) -> Vec<u8> {
        let eocd = find_eocd_offset(&bytes);
        let entry_count = u16::from_le_bytes(bytes[eocd + 10..eocd + 12].try_into().unwrap());
        let central_offset =
            u32::from_le_bytes(bytes[eocd + 16..eocd + 20].try_into().unwrap()) as usize;
        assert_eq!(
            &bytes[central_offset..central_offset + 4],
            &[0x50, 0x4b, 0x01, 0x02]
        );
        let local_offset = u32::from_le_bytes(
            bytes[central_offset + 42..central_offset + 46]
                .try_into()
                .unwrap(),
        ) as usize;
        let crc32 = u32::from_le_bytes(
            bytes[central_offset + 16..central_offset + 20]
                .try_into()
                .unwrap(),
        );
        let compressed_size = u32::from_le_bytes(
            bytes[central_offset + 20..central_offset + 24]
                .try_into()
                .unwrap(),
        );
        let uncompressed_size = u32::from_le_bytes(
            bytes[central_offset + 24..central_offset + 28]
                .try_into()
                .unwrap(),
        );
        let name_length = u16::from_le_bytes(
            bytes[local_offset + 26..local_offset + 28]
                .try_into()
                .unwrap(),
        ) as usize;
        let extra_length = u16::from_le_bytes(
            bytes[local_offset + 28..local_offset + 30]
                .try_into()
                .unwrap(),
        ) as usize;
        let data_start = local_offset + 30 + name_length + extra_length;
        let data_end = data_start + compressed_size as usize;

        let mut descriptor = Vec::with_capacity(16);
        descriptor.extend_from_slice(&0x0807_4b50_u32.to_le_bytes());
        descriptor.extend_from_slice(&crc32.to_le_bytes());
        descriptor.extend_from_slice(&compressed_size.to_le_bytes());
        descriptor.extend_from_slice(&uncompressed_size.to_le_bytes());
        bytes.splice(data_end..data_end, descriptor);

        let new_central_offset = central_offset + 16;
        let mut cursor = new_central_offset;
        for index in 0..entry_count {
            assert_eq!(&bytes[cursor..cursor + 4], &[0x50, 0x4b, 0x01, 0x02]);
            if index == 0 {
                let flags = u16::from_le_bytes(bytes[cursor + 8..cursor + 10].try_into().unwrap());
                bytes[cursor + 8..cursor + 10].copy_from_slice(&(flags | 0x0008).to_le_bytes());
            }
            let entry_local_offset =
                u32::from_le_bytes(bytes[cursor + 42..cursor + 46].try_into().unwrap()) as usize;
            if entry_local_offset >= data_end {
                bytes[cursor + 42..cursor + 46]
                    .copy_from_slice(&((entry_local_offset + 16) as u32).to_le_bytes());
            }
            let name_length =
                u16::from_le_bytes(bytes[cursor + 28..cursor + 30].try_into().unwrap()) as usize;
            let extra_length =
                u16::from_le_bytes(bytes[cursor + 30..cursor + 32].try_into().unwrap()) as usize;
            let comment_length =
                u16::from_le_bytes(bytes[cursor + 32..cursor + 34].try_into().unwrap()) as usize;
            cursor += 46 + name_length + extra_length + comment_length;
        }

        let local_flags = u16::from_le_bytes(
            bytes[local_offset + 6..local_offset + 8]
                .try_into()
                .unwrap(),
        );
        bytes[local_offset + 6..local_offset + 8]
            .copy_from_slice(&(local_flags | 0x0008).to_le_bytes());
        bytes[local_offset + 14..local_offset + 26].fill(0);

        let new_eocd_offset = eocd + 16;
        bytes[new_eocd_offset + 16..new_eocd_offset + 20]
            .copy_from_slice(&(new_central_offset as u32).to_le_bytes());
        bytes
    }

    fn add_zip64_end_records(mut bytes: Vec<u8>) -> Vec<u8> {
        let eocd = find_eocd_offset(&bytes);
        let entries = u16::from_le_bytes(bytes[eocd + 10..eocd + 12].try_into().unwrap()) as u64;
        let directory_size =
            u32::from_le_bytes(bytes[eocd + 12..eocd + 16].try_into().unwrap()) as u64;
        let directory_offset =
            u32::from_le_bytes(bytes[eocd + 16..eocd + 20].try_into().unwrap()) as u64;

        let mut records = Vec::with_capacity(76);
        records.extend_from_slice(&0x0606_4b50_u32.to_le_bytes());
        records.extend_from_slice(&44_u64.to_le_bytes());
        records.extend_from_slice(&45_u16.to_le_bytes());
        records.extend_from_slice(&45_u16.to_le_bytes());
        records.extend_from_slice(&0_u32.to_le_bytes());
        records.extend_from_slice(&0_u32.to_le_bytes());
        records.extend_from_slice(&entries.to_le_bytes());
        records.extend_from_slice(&entries.to_le_bytes());
        records.extend_from_slice(&directory_size.to_le_bytes());
        records.extend_from_slice(&directory_offset.to_le_bytes());
        assert_eq!(records.len(), 56);

        records.extend_from_slice(&0x0706_4b50_u32.to_le_bytes());
        records.extend_from_slice(&0_u32.to_le_bytes());
        records.extend_from_slice(&(eocd as u64).to_le_bytes());
        records.extend_from_slice(&1_u32.to_le_bytes());
        assert_eq!(records.len(), 76);
        bytes.splice(eocd..eocd, records);
        bytes
    }

    fn add_central_directory_comment_that_resembles_zip64_locator(mut bytes: Vec<u8>) -> Vec<u8> {
        let eocd = find_eocd_offset(&bytes);
        let entry_count = u16::from_le_bytes(bytes[eocd + 10..eocd + 12].try_into().unwrap());
        let central_directory_offset =
            u32::from_le_bytes(bytes[eocd + 16..eocd + 20].try_into().unwrap()) as usize;
        let mut cursor = central_directory_offset;
        let mut last_central_entry = 0;

        for _ in 0..entry_count {
            assert_eq!(&bytes[cursor..cursor + 4], &[0x50, 0x4b, 0x01, 0x02]);
            last_central_entry = cursor;
            let name_length =
                u16::from_le_bytes(bytes[cursor + 28..cursor + 30].try_into().unwrap()) as usize;
            let extra_length =
                u16::from_le_bytes(bytes[cursor + 30..cursor + 32].try_into().unwrap()) as usize;
            let comment_length =
                u16::from_le_bytes(bytes[cursor + 32..cursor + 34].try_into().unwrap()) as usize;
            cursor += 46 + name_length + extra_length + comment_length;
        }

        let comment = [
            0x50, 0x4b, 0x06, 0x07, // ZIP64 locator signature
            0, 0, 0, 0, // disk with ZIP64 EOCD
            0, 0, 0, 0, // ZIP64 EOCD offset low
            0, 0, 0, 0, // ZIP64 EOCD offset high
            1, 0, 0, 0, // total disks
        ];
        let old_directory_size =
            u32::from_le_bytes(bytes[eocd + 12..eocd + 16].try_into().unwrap());
        bytes.splice(eocd..eocd, comment);
        bytes[last_central_entry + 32..last_central_entry + 34]
            .copy_from_slice(&(comment.len() as u16).to_le_bytes());
        let new_eocd = eocd + comment.len();
        bytes[new_eocd + 12..new_eocd + 16]
            .copy_from_slice(&(old_directory_size + comment.len() as u32).to_le_bytes());
        bytes
    }

    fn rename_mimetype_local_header_only(mut bytes: Vec<u8>) -> Vec<u8> {
        let eocd = find_eocd_offset(&bytes);
        let central_offset =
            u32::from_le_bytes(bytes[eocd + 16..eocd + 20].try_into().unwrap()) as usize;
        assert_eq!(
            &bytes[central_offset..central_offset + 4],
            &[0x50, 0x4b, 0x01, 0x02]
        );
        let local_offset = u32::from_le_bytes(
            bytes[central_offset + 42..central_offset + 46]
                .try_into()
                .unwrap(),
        ) as usize;
        let name_length = u16::from_le_bytes(
            bytes[local_offset + 26..local_offset + 28]
                .try_into()
                .unwrap(),
        ) as usize;
        assert_eq!(name_length, b"mimetype".len());
        bytes[local_offset + 30..local_offset + 30 + name_length].copy_from_slice(b"mimetyqe");
        bytes
    }

    fn corrupt_mimetype_crc(mut bytes: Vec<u8>) -> Vec<u8> {
        let eocd = find_eocd_offset(&bytes);
        let central_offset =
            u32::from_le_bytes(bytes[eocd + 16..eocd + 20].try_into().unwrap()) as usize;
        assert_eq!(
            &bytes[central_offset..central_offset + 4],
            &[0x50, 0x4b, 0x01, 0x02]
        );
        let local_offset = u32::from_le_bytes(
            bytes[central_offset + 42..central_offset + 46]
                .try_into()
                .unwrap(),
        ) as usize;
        let incorrect_crc = u32::from_le_bytes(
            bytes[central_offset + 16..central_offset + 20]
                .try_into()
                .unwrap(),
        ) ^ 1;
        bytes[central_offset + 16..central_offset + 20]
            .copy_from_slice(&incorrect_crc.to_le_bytes());
        bytes[local_offset + 14..local_offset + 18].copy_from_slice(&incorrect_crc.to_le_bytes());
        bytes
    }

    fn add_data_descriptor_to_last_entry(mut bytes: Vec<u8>) -> Vec<u8> {
        let eocd = find_eocd_offset(&bytes);
        let entry_count = u16::from_le_bytes(bytes[eocd + 10..eocd + 12].try_into().unwrap());
        let central_directory_offset =
            u32::from_le_bytes(bytes[eocd + 16..eocd + 20].try_into().unwrap()) as usize;
        let mut cursor = central_directory_offset;
        let mut last_central_offset = 0;
        let mut last_local_offset = 0;
        let mut last_flags = 0;
        let mut last_crc32 = 0;
        let mut last_compressed_size = 0;
        let mut last_uncompressed_size = 0;

        for _ in 0..entry_count {
            assert_eq!(&bytes[cursor..cursor + 4], &[0x50, 0x4b, 0x01, 0x02]);
            last_central_offset = cursor;
            last_flags = u16::from_le_bytes(bytes[cursor + 8..cursor + 10].try_into().unwrap());
            last_crc32 = u32::from_le_bytes(bytes[cursor + 16..cursor + 20].try_into().unwrap());
            last_compressed_size =
                u32::from_le_bytes(bytes[cursor + 20..cursor + 24].try_into().unwrap());
            last_uncompressed_size =
                u32::from_le_bytes(bytes[cursor + 24..cursor + 28].try_into().unwrap());
            last_local_offset =
                u32::from_le_bytes(bytes[cursor + 42..cursor + 46].try_into().unwrap()) as usize;
            let name_length =
                u16::from_le_bytes(bytes[cursor + 28..cursor + 30].try_into().unwrap()) as usize;
            let extra_length =
                u16::from_le_bytes(bytes[cursor + 30..cursor + 32].try_into().unwrap()) as usize;
            let comment_length =
                u16::from_le_bytes(bytes[cursor + 32..cursor + 34].try_into().unwrap()) as usize;
            cursor += 46 + name_length + extra_length + comment_length;
        }

        let local_name_length = u16::from_le_bytes(
            bytes[last_local_offset + 26..last_local_offset + 28]
                .try_into()
                .unwrap(),
        ) as usize;
        let local_extra_length = u16::from_le_bytes(
            bytes[last_local_offset + 28..last_local_offset + 30]
                .try_into()
                .unwrap(),
        ) as usize;
        let data_start = last_local_offset + 30 + local_name_length + local_extra_length;
        let data_end = data_start + last_compressed_size as usize;
        assert_eq!(
            data_end, central_directory_offset,
            "last entry must precede central directory"
        );

        let mut descriptor = Vec::with_capacity(16);
        descriptor.extend_from_slice(&0x0807_4b50_u32.to_le_bytes());
        descriptor.extend_from_slice(&last_crc32.to_le_bytes());
        descriptor.extend_from_slice(&last_compressed_size.to_le_bytes());
        descriptor.extend_from_slice(&last_uncompressed_size.to_le_bytes());
        bytes.splice(data_end..data_end, descriptor);

        let new_central_offset = central_directory_offset + 16;
        let new_last_central_offset = last_central_offset + 16;
        let new_central_flags = last_flags | 0x0008;
        bytes[new_last_central_offset + 8..new_last_central_offset + 10]
            .copy_from_slice(&new_central_flags.to_le_bytes());
        let local_flags = u16::from_le_bytes(
            bytes[last_local_offset + 6..last_local_offset + 8]
                .try_into()
                .unwrap(),
        );
        bytes[last_local_offset + 6..last_local_offset + 8]
            .copy_from_slice(&(local_flags | 0x0008).to_le_bytes());
        bytes[last_local_offset + 14..last_local_offset + 26].fill(0);

        let new_eocd_offset = eocd + 16;
        bytes[new_eocd_offset + 16..new_eocd_offset + 20]
            .copy_from_slice(&(new_central_offset as u32).to_le_bytes());
        bytes
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
        let eocd = find_eocd_offset(&duplicated);
        let entry_count = u16::from_le_bytes(duplicated[eocd + 10..eocd + 12].try_into().unwrap());
        let mut cursor =
            u32::from_le_bytes(duplicated[eocd + 16..eocd + 20].try_into().unwrap()) as usize;
        let mut names = Vec::new();
        for _ in 0..entry_count {
            assert_eq!(&duplicated[cursor..cursor + 4], &[0x50, 0x4b, 0x01, 0x02]);
            let name_length =
                u16::from_le_bytes(duplicated[cursor + 28..cursor + 30].try_into().unwrap())
                    as usize;
            let extra_length =
                u16::from_le_bytes(duplicated[cursor + 30..cursor + 32].try_into().unwrap())
                    as usize;
            let comment_length =
                u16::from_le_bytes(duplicated[cursor + 32..cursor + 34].try_into().unwrap())
                    as usize;
            names.push(
                String::from_utf8(duplicated[cursor + 46..cursor + 46 + name_length].to_vec())
                    .unwrap(),
            );
            cursor += 46 + name_length + extra_length + comment_length;
        }
        assert_eq!(
            names
                .iter()
                .filter(|name| name.as_str() == "Contents/section0.xml")
                .count(),
            2,
            "fixture must contain duplicate package part names: {names:?}",
        );
        assert!(!is_valid_hwpx_package(&duplicated));
    }

    #[test]
    fn rejects_case_insensitive_duplicate_package_part_names() {
        let bytes = build_archive(&valid_entries());
        let duplicated = rename_entry(bytes, "Contents/metadata.xml", "contents/SECTION0.xml");
        assert!(!is_valid_hwpx_package(&duplicated));
    }

    #[test]
    fn rejects_unicode_case_insensitive_duplicate_package_part_names() {
        let mut entries = valid_entries();
        entries.push(("Contents/Ä.xml", "<x/>", CompressionMethod::Stored));
        entries.push(("Contents/ä.xml", "<x/>", CompressionMethod::Stored));
        let bytes = build_archive(&entries);
        let bytes = mark_entry_utf8(bytes, "Contents/Ä.xml");
        let bytes = mark_entry_utf8(bytes, "Contents/ä.xml");
        assert!(!is_valid_hwpx_package(&bytes));
    }

    #[test]
    fn rejects_mimetype_using_a_data_descriptor() {
        let bytes = build_archive(&valid_entries());
        let descriptor = add_data_descriptor_to_mimetype(bytes);
        let mut archive = zip::ZipArchive::new(Cursor::new(&descriptor)).unwrap();
        let mut mimetype = archive.by_index(0).unwrap();
        let mut value = String::new();
        mimetype.read_to_string(&mut value).unwrap();
        assert_eq!(value, HWPX_MIME_TYPE, "fixture must be a readable ZIP");
        assert!(!is_valid_hwpx_package(&descriptor));
    }

    #[test]
    fn rejects_mimetype_local_name_that_differs_from_the_central_directory() {
        let bytes = build_archive(&valid_entries());
        let mismatched = rename_mimetype_local_header_only(bytes);
        assert!(!is_valid_hwpx_package(&mismatched));
    }

    #[test]
    fn rejects_mimetype_when_crc_does_not_match_payload() {
        let bytes = build_archive(&valid_entries());
        let corrupted = corrupt_mimetype_crc(bytes);
        assert!(!is_valid_hwpx_package(&corrupted));
    }

    #[test]
    fn rejects_zip64_even_when_classic_eocd_fields_still_fit() {
        let bytes = build_archive(&valid_entries());
        let zip64 = add_zip64_end_records(bytes);
        assert!(!is_valid_hwpx_package(&zip64));
    }

    #[test]
    fn accepts_a_central_directory_comment_that_resembles_a_zip64_locator() {
        let bytes = build_archive(&valid_entries());
        let commented = add_central_directory_comment_that_resembles_zip64_locator(bytes);
        assert!(is_valid_hwpx_package(&commented));
    }

    #[test]
    fn accepts_data_descriptors_on_ordinary_package_parts() {
        let bytes = build_archive(&valid_entries());
        let descriptor = add_data_descriptor_to_last_entry(bytes);
        assert!(is_valid_hwpx_package(&descriptor));
    }

    #[test]
    fn rejects_empty_central_directory_entry_names() {
        let mut bytes = Vec::with_capacity(46 + ZIP_EOCD_MIN_LENGTH);
        bytes.extend_from_slice(CENTRAL_DIRECTORY_HEADER_SIGNATURE);
        bytes.extend_from_slice(&[0; 42]);
        bytes.extend_from_slice(END_OF_CENTRAL_DIRECTORY_SIGNATURE);
        bytes.extend_from_slice(&[0; 4]); // Disk number and central directory disk.
        bytes.extend_from_slice(&1_u16.to_le_bytes()); // Entries on this disk.
        bytes.extend_from_slice(&1_u16.to_le_bytes()); // Total entries.
        bytes.extend_from_slice(&46_u32.to_le_bytes()); // Central directory size.
        bytes.extend_from_slice(&0_u32.to_le_bytes()); // Central directory offset.
        bytes.extend_from_slice(&0_u16.to_le_bytes()); // EOCD comment length.

        assert!(inspect_classic_zip_directory(&bytes).is_none());
    }
}
