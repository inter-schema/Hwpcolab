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
        let mut owned = bytes.to_vec();
        sanitize_fat(&mut owned);
        let cursor = Cursor::new(owned);
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

/// Clean real-world CFB FAT quirks before passing bytes to `cfb::CompoundFile::open`:
/// 1. SheetJS / older Hancom exporters fill trailing unused FAT slots (`>= num_sectors`)
///    with `ENDOFCHAIN` (`0xFFFFFFFE`) rather than `FREESECT` (`0xFFFFFFFF`).
/// 2. Legacy exporters occasionally zero-fill unallocated FAT slots (`0x00000000`) when
///    sector 0 is itself free or the zero slot is unreferenced by any directory/stream chain,
///    which trips `cfb`'s `sector 0 pointed to twice` check.
fn sanitize_fat(buf: &mut [u8]) {
    const FREESECT: u32 = 0xFFFF_FFFF;
    const ENDOFCHAIN: u32 = 0xFFFF_FFFE;
    const FAT_SECTOR: u32 = 0xFFFF_FFFD;
    const DIFAT_SECTOR: u32 = 0xFFFF_FFFC;

    if buf.len() < 512 {
        return;
    }
    let sector_shift = u16::from_le_bytes([buf[30], buf[31]]);
    if sector_shift != 9 && sector_shift != 12 {
        return;
    }
    let sector_len = 1usize << sector_shift;
    if buf.len() <= sector_len {
        return;
    }
    let num_sectors = (buf.len() - sector_len) / sector_len;
    let num_fat_sectors = u32::from_le_bytes(buf[44..48].try_into().unwrap()) as usize;
    let entries_per_sec = sector_len / 4;

    let mut fat_sectors = Vec::with_capacity(num_fat_sectors);
    for i in 0..109usize {
        if fat_sectors.len() >= num_fat_sectors {
            break;
        }
        let off = 76 + i * 4;
        let sid = u32::from_le_bytes(buf[off..off + 4].try_into().unwrap());
        if sid == FREESECT {
            break;
        }
        fat_sectors.push(sid);
    }

    let mut next_difat = u32::from_le_bytes(buf[68..72].try_into().unwrap());
    let num_difat_sectors = u32::from_le_bytes(buf[72..76].try_into().unwrap()) as usize;
    let mut difat_hops = 0usize;
    while fat_sectors.len() < num_fat_sectors
        && next_difat < num_sectors as u32
        && difat_hops < num_difat_sectors
    {
        let sec_off = sector_len + (next_difat as usize) * sector_len;
        if sec_off + sector_len > buf.len() {
            break;
        }
        let slots = entries_per_sec.saturating_sub(1);
        for i in 0..slots {
            if fat_sectors.len() >= num_fat_sectors {
                break;
            }
            let off = sec_off + i * 4;
            let sid = u32::from_le_bytes(buf[off..off + 4].try_into().unwrap());
            if sid == FREESECT {
                break;
            }
            fat_sectors.push(sid);
        }
        let tail_off = sec_off + slots * 4;
        next_difat = u32::from_le_bytes(buf[tail_off..tail_off + 4].try_into().unwrap());
        difat_hops += 1;
    }

    // Read current FAT entries into a flat vector so we can detect unreferenced zero slots cleanly.
    let mut fat_vals = vec![FREESECT; fat_sectors.len() * entries_per_sec];
    for (fat_idx, &fat_sid) in fat_sectors.iter().enumerate() {
        if (fat_sid as usize) >= num_sectors {
            continue;
        }
        let sec_off = sector_len + (fat_sid as usize) * sector_len;
        if sec_off + sector_len > buf.len() {
            continue;
        }
        for local_i in 0..entries_per_sec {
            let global_sec = fat_idx * entries_per_sec + local_i;
            let off = sec_off + local_i * 4;
            let val = u32::from_le_bytes(buf[off..off + 4].try_into().unwrap());
            fat_vals[global_sec] = val;
        }
    }

    // Count how many times sector 0 is referenced as a chain target; if sector 0 itself is
    // free or pointed to more than once, walk chains from Directory + MiniFAT + Root to clear
    // unreferenced 0 slots safely without touching legitimate `k -> 0` links.
    let zero_targets = fat_vals
        .iter()
        .take(num_sectors)
        .filter(|&&v| v == 0)
        .count();
    let sec0_free = fat_vals.first().copied().unwrap_or(FREESECT) == FREESECT;
    let mut referenced = vec![false; num_sectors];
    let mut dir_chain = Vec::new();
    let mut minifat_chain = Vec::new();
    let mut mini_starts: Vec<u32> = Vec::new();

    {
        let mut dir_sid = u32::from_le_bytes(buf[48..52].try_into().unwrap());
        while (dir_sid as usize) < num_sectors && !referenced[dir_sid as usize] {
            referenced[dir_sid as usize] = true;
            dir_chain.push(dir_sid);
            dir_sid = fat_vals[dir_sid as usize];
        }
        let mut mf_sid = u32::from_le_bytes(buf[60..64].try_into().unwrap());
        while (mf_sid as usize) < num_sectors && !referenced[mf_sid as usize] {
            referenced[mf_sid as usize] = true;
            minifat_chain.push(mf_sid);
            mf_sid = fat_vals[mf_sid as usize];
        }
        let mut mark_chain = |mut sid: u32| {
            while (sid as usize) < num_sectors && !referenced[sid as usize] {
                referenced[sid as usize] = true;
                sid = fat_vals[sid as usize];
            }
        };
        for &ds in &dir_chain {
            let sec_off = sector_len + (ds as usize) * sector_len;
            if sec_off + sector_len > buf.len() {
                continue;
            }
            for e in 0..(sector_len / 128) {
                let eo = sec_off + e * 128;
                let etype = buf[eo + 0x42];
                let start = u32::from_le_bytes(buf[eo + 0x74..eo + 0x78].try_into().unwrap());
                let size = u32::from_le_bytes(buf[eo + 0x78..eo + 0x7C].try_into().unwrap());
                if etype == 5 || (etype == 2 && size >= 4096) {
                    mark_chain(start);
                } else if etype == 2 && size > 0 && size < 4096 {
                    mini_starts.push(start);
                }
            }
        }
    }

    for (fat_idx, &fat_sid) in fat_sectors.iter().enumerate() {
        if (fat_sid as usize) >= num_sectors {
            continue;
        }
        let sec_off = sector_len + (fat_sid as usize) * sector_len;
        if sec_off + sector_len > buf.len() {
            continue;
        }
        for local_i in 0..entries_per_sec {
            let global_sec = fat_idx * entries_per_sec + local_i;
            let off = sec_off + local_i * 4;
            let val = fat_vals[global_sec];
            if global_sec >= num_sectors {
                if matches!(val, 0 | ENDOFCHAIN | FAT_SECTOR | DIFAT_SECTOR) {
                    buf[off..off + 4].copy_from_slice(&FREESECT.to_le_bytes());
                }
            } else if val == 0 && !referenced[global_sec] && (sec0_free || zero_targets > 1) {
                buf[off..off + 4].copy_from_slice(&FREESECT.to_le_bytes());
            }
        }
    }

    // Same zero-slot normalization for MiniFAT sectors when legacy writers zero-fill unused entries.
    if !minifat_chain.is_empty() {
        let mut mf_vals = Vec::with_capacity(minifat_chain.len() * entries_per_sec);
        for &mf_sid in &minifat_chain {
            let sec_off = sector_len + (mf_sid as usize) * sector_len;
            if sec_off + sector_len > buf.len() {
                break;
            }
            for i in 0..entries_per_sec {
                let off = sec_off + i * 4;
                mf_vals.push(u32::from_le_bytes(buf[off..off + 4].try_into().unwrap()));
            }
        }
        let mf_zeros = mf_vals.iter().filter(|&&v| v == 0).count();
        let mf0_free = mf_vals.first().copied().unwrap_or(FREESECT) == FREESECT;
        if mf_zeros > 1 || (mf_zeros == 1 && mf0_free) {
            let mut mf_ref = vec![false; mf_vals.len()];
            for mut msid in mini_starts {
                while (msid as usize) < mf_vals.len() && !mf_ref[msid as usize] {
                    mf_ref[msid as usize] = true;
                    msid = mf_vals[msid as usize];
                }
            }
            for (c_idx, &mf_sid) in minifat_chain.iter().enumerate() {
                let sec_off = sector_len + (mf_sid as usize) * sector_len;
                if sec_off + sector_len > buf.len() {
                    break;
                }
                for i in 0..entries_per_sec {
                    let g = c_idx * entries_per_sec + i;
                    if g < mf_vals.len() && mf_vals[g] == 0 && !mf_ref[g] {
                        let off = sec_off + i * 4;
                        buf[off..off + 4].copy_from_slice(&FREESECT.to_le_bytes());
                    }
                }
            }
        }
    }
}
