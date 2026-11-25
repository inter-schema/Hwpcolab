use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Read, Write};

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::error::HwpxError;
use crate::package::is_valid_hwpx_package;

const HWPX_MIME_TYPE: &[u8] = b"application/hwp+zip";

/// In-memory HWPX ZIP container supporting lazy UTF-8 reads, selective dirty
/// tracking, and `raw_copy_file` preservation of untouched entries.
pub struct HwpxArchive {
    original_bytes: Vec<u8>,
    entry_order: Vec<String>,
    text_cache: BTreeMap<String, String>,
    dirty_text: BTreeMap<String, String>,
    dirty_bytes: BTreeMap<String, Vec<u8>>,
    deleted: BTreeSet<String>,
}

impl HwpxArchive {
    /// Open and validate an HWPX package from raw bytes.
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, HwpxError> {
        if !is_valid_hwpx_package(&bytes) {
            return Err(HwpxError::InvalidPackage(
                "failed is_valid_hwpx_package check".to_string(),
            ));
        }

        let archive = ZipArchive::new(Cursor::new(&bytes))?;
        let mut entry_order = Vec::with_capacity(archive.len());
        for idx in 0..archive.len() {
            if let Some(name) = archive.name_for_index(idx) {
                entry_order.push(name.to_string());
            }
        }

        Ok(Self {
            original_bytes: bytes,
            entry_order,
            text_cache: BTreeMap::new(),
            dirty_text: BTreeMap::new(),
            dirty_bytes: BTreeMap::new(),
            deleted: BTreeSet::new(),
        })
    }

    /// Return all active entry names in package order.
    pub fn entry_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .entry_order
            .iter()
            .filter(|n| !self.deleted.contains(*n))
            .cloned()
            .collect();
        for n in self.dirty_text.keys() {
            if !self.deleted.contains(n) && !names.contains(n) {
                names.push(n.clone());
            }
        }
        for n in self.dirty_bytes.keys() {
            if !self.deleted.contains(n) && !names.contains(n) {
                names.push(n.clone());
            }
        }
        names
    }

    /// Return `Contents/sectionN.xml` entries sorted numerically by `N`.
    pub fn section_names(&self) -> Vec<String> {
        let mut secs: Vec<(usize, String)> = self
            .entry_names()
            .into_iter()
            .filter_map(|name| parse_section_index(&name).map(|idx| (idx, name)))
            .collect();
        secs.sort_by_key(|(idx, _)| *idx);
        secs.into_iter().map(|(_, name)| name).collect()
    }

    /// Return `Contents/header.xml` entry name if present.
    pub fn header_name(&self) -> Option<String> {
        self.entry_names()
            .into_iter()
            .find(|n| n.eq_ignore_ascii_case("Contents/header.xml"))
    }

    /// Return `Contents/content.hpf` entry name if present.
    pub fn hpf_name(&self) -> Option<String> {
        self.entry_names()
            .into_iter()
            .find(|n| n.eq_ignore_ascii_case("Contents/content.hpf"))
    }

    /// Check if an active entry exists.
    pub fn contains(&self, name: &str) -> bool {
        if self.deleted.contains(name) {
            return false;
        }
        self.dirty_text.contains_key(name)
            || self.dirty_bytes.contains_key(name)
            || self.entry_order.iter().any(|n| n == name)
    }

    /// Read an entry as UTF-8 text (from dirty buffer, cache, or original ZIP).
    pub fn read_text(&mut self, name: &str) -> Result<String, HwpxError> {
        if self.deleted.contains(name) {
            return Err(HwpxError::MissingPart(name.to_string()));
        }
        if let Some(s) = self.dirty_text.get(name) {
            return Ok(s.clone());
        }
        if let Some(s) = self.text_cache.get(name) {
            return Ok(s.clone());
        }
        let raw = self.read_raw_bytes(name)?;
        let text = String::from_utf8(raw)?;
        self.text_cache.insert(name.to_string(), text.clone());
        Ok(text)
    }

    /// Read an entry as raw bytes.
    pub fn read_raw_bytes(&self, name: &str) -> Result<Vec<u8>, HwpxError> {
        if self.deleted.contains(name) {
            return Err(HwpxError::MissingPart(name.to_string()));
        }
        if let Some(b) = self.dirty_bytes.get(name) {
            return Ok(b.clone());
        }
        if let Some(s) = self.dirty_text.get(name) {
            return Ok(s.as_bytes().to_vec());
        }
        let mut archive = ZipArchive::new(Cursor::new(&self.original_bytes))?;
        let mut entry = archive.by_name(name)?;
        let mut buf = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut buf)?;
        Ok(buf)
    }

    /// Mark a text entry dirty with new XML/text contents.
    pub fn write_text(&mut self, name: impl Into<String>, content: String) {
        let name = name.into();
        self.deleted.remove(&name);
        self.dirty_bytes.remove(&name);
        self.text_cache.insert(name.clone(), content.clone());
        self.dirty_text.insert(name, content);
    }

    /// Mark a binary entry dirty (new or replaced).
    pub fn write_bytes(&mut self, name: impl Into<String>, bytes: Vec<u8>) {
        let name = name.into();
        self.deleted.remove(&name);
        self.dirty_text.remove(&name);
        self.text_cache.remove(&name);
        self.dirty_bytes.insert(name, bytes);
    }

    /// Delete an entry from the package.
    pub fn delete_entry(&mut self, name: &str) {
        self.dirty_text.remove(name);
        self.dirty_bytes.remove(name);
        self.text_cache.remove(name);
        self.deleted.insert(name.to_string());
    }

    /// Repackage into valid HWPX bytes:
    /// - `mimetype` written first as `CompressionMethod::Stored`
    /// - untouched entries copied verbatim via `ZipWriter::raw_copy_file`
    /// - dirty and newly added entries written deflated
    /// - output validated with `is_valid_hwpx_package`
    pub fn save(&self) -> Result<Vec<u8>, HwpxError> {
        let mut archive = ZipArchive::new(Cursor::new(&self.original_bytes))?;
        let mut writer = ZipWriter::new(Cursor::new(Vec::with_capacity(self.original_bytes.len())));

        // 1. `mimetype` must be first and STORED.
        let stored_opts = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        writer.start_file("mimetype", stored_opts)?;
        writer.write_all(HWPX_MIME_TYPE)?;

        let deflated_opts =
            SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

        // 2. Existing entries in original order.
        let mut written = BTreeSet::new();
        written.insert("mimetype".to_string());

        for idx in 0..archive.len() {
            let name = archive
                .name_for_index(idx)
                .map(ToOwned::to_owned)
                .ok_or_else(|| HwpxError::InvalidPackage("unnamed zip entry".to_string()))?;
            if name == "mimetype" {
                continue;
            }
            written.insert(name.clone());
            if self.deleted.contains(&name) {
                continue;
            }
            if let Some(text) = self.dirty_text.get(&name) {
                writer.start_file(&name, deflated_opts)?;
                writer.write_all(text.as_bytes())?;
            } else if let Some(bytes) = self.dirty_bytes.get(&name) {
                writer.start_file(&name, deflated_opts)?;
                writer.write_all(bytes)?;
            } else {
                let entry = archive.by_index(idx)?;
                writer.raw_copy_file(entry)?;
            }
        }

        // 3. Newly added entries.
        for (name, text) in &self.dirty_text {
            if !written.contains(name) && !self.deleted.contains(name) {
                writer.start_file(name, deflated_opts)?;
                writer.write_all(text.as_bytes())?;
                written.insert(name.clone());
            }
        }
        for (name, bytes) in &self.dirty_bytes {
            if !written.contains(name) && !self.deleted.contains(name) {
                writer.start_file(name, deflated_opts)?;
                writer.write_all(bytes)?;
                written.insert(name.clone());
            }
        }

        let out = writer.finish()?.into_inner();
        if !is_valid_hwpx_package(&out) {
            return Err(HwpxError::InvalidPackage(
                "repackaged HWPX failed validation".to_string(),
            ));
        }
        Ok(out)
    }
}

fn parse_section_index(name: &str) -> Option<usize> {
    let rest = name
        .strip_prefix("Contents/section")
        .or_else(|| {
            // Case-insensitive match for `Contents/sectionN.xml`
            let lower = name.to_ascii_lowercase();
            if lower.starts_with("contents/section") && lower.ends_with(".xml") {
                Some(&name["Contents/section".len()..])
            } else {
                None
            }
        })?;
    let num = rest
        .strip_suffix(".xml")
        .or_else(|| rest.strip_suffix(".XML"))?;
    if num.is_empty() {
        return None;
    }
    num.parse::<usize>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn crc_map(bytes: &[u8]) -> HashMap<String, u32> {
        let mut arc = ZipArchive::new(Cursor::new(bytes)).unwrap();
        let mut map = HashMap::new();
        for i in 0..arc.len() {
            let e = arc.by_index(i).unwrap();
            map.insert(e.name().to_string(), e.crc32());
        }
        map
    }

    #[test]
    fn untouched_entries_preserve_crc32_on_save() {
        let before_bytes = crate::BLANK_HWPX_TEMPLATE.to_vec();
        let before_crcs = crc_map(&before_bytes);

        let mut hwpx = HwpxArchive::from_bytes(before_bytes).unwrap();
        let sec_name = hwpx.section_names()[0].clone();
        let sec = hwpx.read_text(&sec_name).unwrap();
        let modified = sec.replace("<hp:t/>", "<hp:t>CRC Preservation Test</hp:t>");
        hwpx.write_text(&sec_name, modified);

        let after_bytes = hwpx.save().unwrap();
        assert!(is_valid_hwpx_package(&after_bytes));

        let after_crcs = crc_map(&after_bytes);
        for (name, crc) in &before_crcs {
            if name == &sec_name {
                assert_ne!(after_crcs.get(name), Some(crc), "modified section CRC should change");
            } else {
                assert_eq!(
                    after_crcs.get(name),
                    Some(crc),
                    "untouched entry {name} CRC32 changed"
                );
            }
        }
    }
}
