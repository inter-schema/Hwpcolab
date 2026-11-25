use std::io::{Cursor, Read, Write};

use flate2::read::DeflateDecoder;
use flate2::write::DeflateEncoder;
use flate2::Compression;

use crate::error::{Hwp5Error, Result};

/// Fixed 256-byte size of the HWP 5.0 `FileHeader` stream.
pub const FILE_HEADER_LEN: usize = 256;

/// Magic ASCII prefix at `FileHeader[0..17]`.
pub const FILE_HEADER_SIGNATURE: &[u8] = b"HWP Document File";

/// Parsed HWP 5.0 `FileHeader` stream metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileHeader {
    /// Raw 256-byte header buffer preserved on write.
    pub raw: [u8; FILE_HEADER_LEN],
    /// Packed HWP version (`MMnnPPrr` little-endian `u32` at offset 32).
    pub version: u32,
    /// Document flags `u32` at offset 36.
    pub flags: u32,
}

impl FileHeader {
    /// Parse and validate a 256-byte `FileHeader` stream.
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < FILE_HEADER_LEN {
            return Err(Hwp5Error::InvalidHeader(format!(
                "FileHeader expected {FILE_HEADER_LEN} bytes, got {}",
                bytes.len()
            )));
        }
        if !bytes.starts_with(FILE_HEADER_SIGNATURE) {
            return Err(Hwp5Error::InvalidHeader(
                "missing 'HWP Document File' signature".to_string(),
            ));
        }
        let mut raw = [0u8; FILE_HEADER_LEN];
        raw.copy_from_slice(&bytes[..FILE_HEADER_LEN]);
        let version = u32::from_le_bytes(raw[32..36].try_into().unwrap());
        let flags = u32::from_le_bytes(raw[36..40].try_into().unwrap());
        Ok(Self {
            raw,
            version,
            flags,
        })
    }

    /// True if `DocInfo` and `BodyText/SectionN` streams are raw-deflate compressed (bit 0).
    pub fn is_compressed(&self) -> bool {
        (self.flags & 0x01) != 0
    }

    /// True if the document is encrypted (bit 1).
    pub fn is_encrypted(&self) -> bool {
        (self.flags & 0x02) != 0
    }

    /// True if the document is a distribution (`ViewText`) document (bit 2).
    pub fn is_distribution(&self) -> bool {
        (self.flags & 0x04) != 0
    }
}

/// Inflate a raw-deflate HWP 5.0 stream (`DocInfo`, `BodyText/SectionN`, etc.).
pub fn inflate_raw(compressed: &[u8]) -> Result<Vec<u8>> {
    let mut decoder = DeflateDecoder::new(compressed);
    let mut out = Vec::new();
    decoder.read_to_end(&mut out).map_err(|err| {
        Hwp5Error::Compression(format!("raw deflate decompression failed: {err}"))
    })?;
    Ok(out)
}

/// Deflate a stream using raw deflate at compression level 9 (required for Hancom Docs compatibility).
pub fn deflate_raw_level9(plain: &[u8]) -> Result<Vec<u8>> {
    let mut encoder = DeflateEncoder::new(Vec::new(), Compression::best());
    encoder.write_all(plain).map_err(|err| {
        Hwp5Error::Compression(format!("raw deflate write failed: {err}"))
    })?;
    encoder.finish().map_err(|err| {
        Hwp5Error::Compression(format!("raw deflate finish failed: {err}"))
    })
}

/// Normalize CFB stream path into canonical `/Parent/Name` form.
pub fn normalize_cfb_path(path: &str) -> String {
    let clean = path.replace('\\', "/");
    let trimmed = clean.trim_start_matches('/');
    if trimmed.is_empty() {
        "/".to_string()
    } else {
        format!("/{trimmed}")
    }
}

/// Pure-Rust CFB container wrapper for HWP 5.0 documents (`cfb` 0.14.0).
pub struct Hwp5Container {
    cfb: cfb::CompoundFile<Cursor<Vec<u8>>>,
    file_header: FileHeader,
}

impl Hwp5Container {
    /// Open an HWP 5.0 document from raw CFB bytes in memory.
    pub fn open_bytes(bytes: &[u8]) -> Result<Self> {
        let cursor = Cursor::new(bytes.to_vec());
        let mut comp = cfb::CompoundFile::open(cursor).map_err(|err| {
            Hwp5Error::InvalidContainer(format!("failed to open CFB container: {err}"))
        })?;
        let fh_bytes = read_stream_from(&mut comp, "/FileHeader")?;
        let file_header = FileHeader::parse(&fh_bytes)?;
        if file_header.is_encrypted() {
            return Err(Hwp5Error::UnsupportedFormat(
                "encrypted HWP 5.0 documents are not supported".to_string(),
            ));
        }
        Ok(Self {
            cfb: comp,
            file_header,
        })
    }

    /// Return the parsed `FileHeader` metadata.
    pub fn file_header(&self) -> &FileHeader {
        &self.file_header
    }

    /// List all stream paths (excluding storages), sorted lexicographically.
    pub fn stream_paths(&self) -> Vec<String> {
        let mut out: Vec<String> = self
            .cfb
            .walk()
            .filter(|entry| entry.is_stream())
            .map(|entry| normalize_cfb_path(&entry.path().to_string_lossy()))
            .collect();
        out.sort();
        out
    }

    /// Check whether a stream exists at `path`.
    pub fn has_stream(&self, path: &str) -> bool {
        let norm = normalize_cfb_path(path);
        self.cfb.exists(&norm) && self.cfb.is_stream(&norm)
    }

    /// Read raw (un-inflated) bytes of a CFB stream.
    pub fn read_raw_stream(&mut self, path: &str) -> Result<Vec<u8>> {
        read_stream_from(&mut self.cfb, path)
    }

    /// Write raw bytes to a CFB stream, creating parent storages if missing and truncating any prior content.
    pub fn write_raw_stream(&mut self, path: &str, data: &[u8]) -> Result<()> {
        let norm = normalize_cfb_path(path);
        if let Some((parent, _)) = norm.rsplit_once('/') {
            if !parent.is_empty() && !self.cfb.exists(parent) {
                self.cfb.create_storage_all(parent).map_err(|err| {
                    Hwp5Error::InvalidContainer(format!(
                        "failed to create CFB storage '{parent}': {err}"
                    ))
                })?;
            }
        }
        let mut stream = self.cfb.create_stream(&norm).map_err(|err| {
            Hwp5Error::InvalidContainer(format!("failed to create CFB stream '{norm}': {err}"))
        })?;
        stream.write_all(data)?;
        stream.flush()?;
        Ok(())
    }

    /// Remove a stream if it exists; returns `Ok(true)` if removed, `Ok(false)` if absent.
    pub fn remove_stream(&mut self, path: &str) -> Result<bool> {
        let norm = normalize_cfb_path(path);
        if !self.cfb.exists(&norm) {
            return Ok(false);
        }
        self.cfb.remove_stream(&norm).map_err(|err| {
            Hwp5Error::InvalidContainer(format!("failed to remove CFB stream '{norm}': {err}"))
        })?;
        Ok(true)
    }

    /// Read and inflate (when `FileHeader.is_compressed()`) a record stream such as `DocInfo` or `BodyText/Section0`.
    pub fn read_record_stream(&mut self, path: &str) -> Result<Vec<u8>> {
        let raw = self.read_raw_stream(path)?;
        if self.file_header.is_compressed() {
            inflate_raw(&raw)
        } else {
            Ok(raw)
        }
    }

    /// Deflate (at level 9 when compressed) and write back a record stream such as `DocInfo` or `BodyText/SectionN`.
    pub fn write_record_stream(&mut self, path: &str, plain: &[u8]) -> Result<()> {
        if self.file_header.is_compressed() {
            let compressed = deflate_raw_level9(plain)?;
            self.write_raw_stream(path, &compressed)
        } else {
            self.write_raw_stream(path, plain)
        }
    }

    /// Discover sorted section stream paths (`/BodyText/Section0`, `/BodyText/Section1`, ...).
    pub fn section_paths(&self) -> Vec<String> {
        let mut indexed: Vec<(usize, String)> = self
            .stream_paths()
            .into_iter()
            .filter_map(|p| {
                let idx_str = p.strip_prefix("/BodyText/Section")?;
                let idx = idx_str.parse::<usize>().ok()?;
                Some((idx, p))
            })
            .collect();
        indexed.sort_by_key(|(idx, _)| *idx);
        indexed.into_iter().map(|(_, p)| p).collect()
    }

    /// Read and inflate `DocInfo` bytes.
    pub fn read_docinfo(&mut self) -> Result<Vec<u8>> {
        self.read_record_stream("/DocInfo")
    }

    /// Deflate and write `DocInfo` bytes.
    pub fn write_docinfo(&mut self, plain: &[u8]) -> Result<()> {
        self.write_record_stream("/DocInfo", plain)
    }

    /// Read and inflate `BodyText/Section{index}`.
    pub fn read_section(&mut self, section_idx: usize) -> Result<Vec<u8>> {
        self.read_record_stream(&format!("/BodyText/Section{section_idx}"))
    }

    /// Deflate and write `BodyText/Section{index}`.
    pub fn write_section(&mut self, section_idx: usize, plain: &[u8]) -> Result<()> {
        self.write_record_stream(&format!("/BodyText/Section{section_idx}"), plain)
    }

    /// Flush CFB allocation tables/directory and return serialized `.hwp` container bytes.
    pub fn into_bytes(mut self) -> Result<Vec<u8>> {
        self.cfb.flush()?;
        Ok(self.cfb.into_inner().into_inner())
    }
}

fn read_stream_from(
    comp: &mut cfb::CompoundFile<Cursor<Vec<u8>>>,
    path: &str,
) -> Result<Vec<u8>> {
    let norm = normalize_cfb_path(path);
    if !comp.exists(&norm) || !comp.is_stream(&norm) {
        return Err(Hwp5Error::MissingStream(norm));
    }
    let mut stream = comp.open_stream(&norm).map_err(|err| {
        Hwp5Error::InvalidContainer(format!("failed to open CFB stream '{norm}': {err}"))
    })?;
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf)?;
    Ok(buf)
}
