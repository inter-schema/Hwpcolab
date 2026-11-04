import { caseFold } from 'unicode-case-folding';

const LOCAL_FILE_HEADER = 0x04034b50;
const CENTRAL_DIRECTORY_HEADER = 0x02014b50;
const END_OF_CENTRAL_DIRECTORY = 0x06054b50;
const DATA_DESCRIPTOR_SIGNATURE = 0x08074b50;
const ZIP64_END_OF_CENTRAL_DIRECTORY = 0x06064b50;
const ZIP64_END_OF_CENTRAL_DIRECTORY_LOCATOR = 0x07064b50;
const HWPX_MIME_TYPE = 'application/hwp+zip';
const REQUIRED_PARTS = [
  'Contents/content.hpf',
  'Contents/header.xml',
] as const;
const MAX_ZIP_COMMENT_LENGTH = 0xffff;

function readUint16(view: DataView, offset: number): number {
  return view.getUint16(offset, true);
}

function readUint32(view: DataView, offset: number): number {
  return view.getUint32(offset, true);
}

function calculateCrc32(bytes: Uint8Array): number {
  let checksum = 0xffff_ffff;
  for (const byte of bytes) {
    checksum ^= byte;
    for (let bit = 0; bit < 8; bit += 1) {
      checksum = (checksum >>> 1) ^ ((checksum & 1) === 0 ? 0 : 0xedb8_8320);
    }
  }
  return (checksum ^ 0xffff_ffff) >>> 0;
}

function canonicalCaseFold(name: string): string {
  return caseFold(name.normalize('NFD')).normalize('NFD');
}

/** @internal Exposed so the ZIP descriptor ambiguity rule can be regression-tested. */
export function resolveZipDataDescriptorEnd(
  view: DataView,
  offset: number,
  crc32: number,
  compressedSize: number,
  uncompressedSize: number,
  centralDirectoryOffset: number,
): number | null {
  const descriptorMatchesAt = (candidateOffset: number): boolean => (
    candidateOffset + 12 <= centralDirectoryOffset
    && readUint32(view, candidateOffset) === crc32
    && readUint32(view, candidateOffset + 4) === compressedSize
    && readUint32(view, candidateOffset + 8) === uncompressedSize
  );

  const signedDescriptorMatches = readUint32(view, offset) === DATA_DESCRIPTOR_SIGNATURE
    && descriptorMatchesAt(offset + 4);
  if (signedDescriptorMatches) return offset + 16;
  if (descriptorMatchesAt(offset)) return offset + 12;
  return null;
}

type CentralDirectoryEntry = {
  name: string;
  nameBytes: Uint8Array;
  flags: number;
  method: number;
  crc32: number;
  compressedSize: number;
  uncompressedSize: number;
  localHeaderOffset: number;
};

function consistentLocalRecordEnd(
  bytes: Uint8Array,
  view: DataView,
  entry: CentralDirectoryEntry,
  centralDirectoryOffset: number,
): number | null {
  const localOffset = entry.localHeaderOffset;
  if (
    localOffset + 30 > centralDirectoryOffset
    || readUint32(view, localOffset) !== LOCAL_FILE_HEADER
  ) {
    return null;
  }

  const localFlags = readUint16(view, localOffset + 6);
  const localMethod = readUint16(view, localOffset + 8);
  const localCrc32 = readUint32(view, localOffset + 14);
  const localCompressedSize = readUint32(view, localOffset + 18);
  const localUncompressedSize = readUint32(view, localOffset + 22);
  const localNameLength = readUint16(view, localOffset + 26);
  const localExtraLength = readUint16(view, localOffset + 28);
  const localNameStart = localOffset + 30;
  const localNameEnd = localNameStart + localNameLength;
  const localDataStart = localNameEnd + localExtraLength;
  const localDataEnd = localDataStart + entry.compressedSize;
  if (
    localDataEnd > centralDirectoryOffset
    || localNameEnd > bytes.byteLength
    || localDataStart > bytes.byteLength
    || localDataEnd > bytes.byteLength
  ) {
    return null;
  }

  const localName = bytes.subarray(localNameStart, localNameEnd);
  if (
    localName.byteLength !== entry.nameBytes.byteLength
    || !localName.every((byte, index) => byte === entry.nameBytes[index])
    || localFlags !== entry.flags
    || localMethod !== entry.method
  ) {
    return null;
  }

  const usesDataDescriptor = (entry.flags & 0x0008) !== 0;
  const crcMatches = usesDataDescriptor
    ? localCrc32 === 0 || localCrc32 === entry.crc32
    : localCrc32 === entry.crc32;
  const compressedSizeMatches = usesDataDescriptor
    ? localCompressedSize === 0 || localCompressedSize === entry.compressedSize
    : localCompressedSize === entry.compressedSize;
  const uncompressedSizeMatches = usesDataDescriptor
    ? localUncompressedSize === 0 || localUncompressedSize === entry.uncompressedSize
    : localUncompressedSize === entry.uncompressedSize;

  if (!crcMatches || !compressedSizeMatches || !uncompressedSizeMatches) return null;
  if (!usesDataDescriptor) return localDataEnd;

  return resolveZipDataDescriptorEnd(
    view,
    localDataEnd,
    entry.crc32,
    entry.compressedSize,
    entry.uncompressedSize,
    centralDirectoryOffset,
  );
}

function hasNonOverlappingLocalFileRecords(
  bytes: Uint8Array,
  view: DataView,
  entries: CentralDirectoryEntry[],
  centralDirectoryOffset: number,
): boolean {
  const ranges: Array<{ start: number; end: number }> = [];
  for (const entry of entries) {
    const end = consistentLocalRecordEnd(bytes, view, entry, centralDirectoryOffset);
    if (end === null) return false;
    ranges.push({ start: entry.localHeaderOffset, end });
  }
  ranges.sort((left, right) => left.start - right.start);
  return ranges.every((range, index) => index === 0 || ranges[index - 1].end <= range.start);
}

function findEndOfCentralDirectory(view: DataView, bytesLength: number): number {
  const minimumOffset = Math.max(0, bytesLength - 22 - MAX_ZIP_COMMENT_LENGTH);
  for (let offset = bytesLength - 22; offset >= minimumOffset; offset -= 1) {
    if (readUint32(view, offset) !== END_OF_CENTRAL_DIRECTORY) continue;
    const commentLength = readUint16(view, offset + 20);
    if (offset + 22 + commentLength === bytesLength) return offset;
  }
  return -1;
}

function hasZip64EndOfCentralDirectoryLocator(
  view: DataView,
  eocdOffset: number,
  centralDirectoryEnd: number,
): boolean {
  const locatorOffset = eocdOffset - 20;
  if (
    locatorOffset < centralDirectoryEnd
    || readUint32(view, locatorOffset) !== ZIP64_END_OF_CENTRAL_DIRECTORY_LOCATOR
  ) {
    return false;
  }

  const diskWithZip64End = readUint32(view, locatorOffset + 4);
  const zip64EndOffsetLow = readUint32(view, locatorOffset + 8);
  const zip64EndOffsetHigh = readUint32(view, locatorOffset + 12);
  const totalDisks = readUint32(view, locatorOffset + 16);
  if (diskWithZip64End !== 0 || totalDisks !== 1) return false;
  if (zip64EndOffsetHigh !== 0) return true;

  const zip64EndOffset = zip64EndOffsetLow;
  if (
    zip64EndOffset < centralDirectoryEnd
    || zip64EndOffset + 56 > locatorOffset
    || readUint32(view, zip64EndOffset) !== ZIP64_END_OF_CENTRAL_DIRECTORY
  ) {
    return false;
  }

  const recordSizeLow = readUint32(view, zip64EndOffset + 4);
  const recordSizeHigh = readUint32(view, zip64EndOffset + 8);
  if (recordSizeHigh !== 0) return true;
  if (recordSizeLow < 44) return false;
  return zip64EndOffset + 12 + recordSizeLow === locatorOffset;
}

/**
 * Validates the OWPML package envelope without decompressing arbitrary ZIP
 * entries. It checks the first local/central entry, stored mimetype payload,
 * single-disk classic ZIP directory bounds, and essential document parts.
 * ZIP64, encrypted entries and malformed archives fail closed. Data descriptors
 * remain valid for ordinary entries; the mimetype entry must still be stored
 * with complete metadata in its first local header.
 */
export function isValidHwpxPackage(bytes: Uint8Array): boolean {
  try {
    if (bytes.byteLength < 22) return false;
    const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
    const eocdOffset = findEndOfCentralDirectory(view, bytes.byteLength);
    if (eocdOffset < 0) return false;

    const diskNumber = readUint16(view, eocdOffset + 4);
    const centralDirectoryDisk = readUint16(view, eocdOffset + 6);
    const entriesOnDisk = readUint16(view, eocdOffset + 8);
    const totalEntries = readUint16(view, eocdOffset + 10);
    const centralDirectorySize = readUint32(view, eocdOffset + 12);
    const centralDirectoryOffset = readUint32(view, eocdOffset + 16);

    if (
      diskNumber !== 0
      || centralDirectoryDisk !== 0
      || entriesOnDisk !== totalEntries
      || totalEntries === 0
      || totalEntries === 0xffff
      || centralDirectorySize === 0xffffffff
      || centralDirectoryOffset === 0xffffffff
      || centralDirectoryOffset + centralDirectorySize > eocdOffset
    ) {
      return false;
    }

    // Preserve a leading U+FEFF as part of the ZIP filename. The default
    // TextDecoder behavior strips it, which can alias a BOM-prefixed member
    // to a required OWPML part name and disagree with the Rust validator.
    // A ZIP64 locator/record must live after the complete central directory.
    // Central-directory comments are opaque bytes and can contain signatures
    // and record-shaped data that must not be mistaken for ZIP64 structures.
    const centralDirectoryEnd = centralDirectoryOffset + centralDirectorySize;
    if (hasZip64EndOfCentralDirectoryLocator(view, eocdOffset, centralDirectoryEnd)) return false;

    const decoder = new TextDecoder('utf-8', { fatal: true, ignoreBOM: true });
    const names: string[] = [];
    const seenNames = new Set<string>();
    const centralEntries: CentralDirectoryEntry[] = [];
    let firstCentralEntry: CentralDirectoryEntry | null = null;
    let cursor = centralDirectoryOffset;

    for (let index = 0; index < totalEntries; index += 1) {
      if (cursor + 46 > centralDirectoryOffset + centralDirectorySize) return false;
      if (readUint32(view, cursor) !== CENTRAL_DIRECTORY_HEADER) return false;

      const flags = readUint16(view, cursor + 8);
      const method = readUint16(view, cursor + 10);
      const crc32 = readUint32(view, cursor + 16);
      const compressedSize = readUint32(view, cursor + 20);
      const uncompressedSize = readUint32(view, cursor + 24);
      const nameLength = readUint16(view, cursor + 28);
      const extraLength = readUint16(view, cursor + 30);
      const commentLength = readUint16(view, cursor + 32);
      const startDisk = readUint16(view, cursor + 34);
      const localHeaderOffset = readUint32(view, cursor + 42);
      const recordEnd = cursor + 46 + nameLength + extraLength + commentLength;

      if (
        recordEnd > centralDirectoryOffset + centralDirectorySize
        || nameLength === 0
        || startDisk !== 0
        || (flags & 0x0041) !== 0
        || compressedSize === 0xffffffff
        || uncompressedSize === 0xffffffff
        || localHeaderOffset === 0xffffffff
      ) {
        return false;
      }

      const nameBytes = bytes.subarray(cursor + 46, cursor + 46 + nameLength);
      if (nameBytes.some((byte) => byte > 0x7f) && (flags & 0x0800) === 0) return false;
      const name = decoder.decode(nameBytes);
      const normalizedName = canonicalCaseFold(name);
      if (seenNames.has(normalizedName)) return false;
      seenNames.add(normalizedName);
      names.push(name);
      const entry = {
        name,
        nameBytes: nameBytes.slice(),
        flags,
        method,
        crc32,
        compressedSize,
        uncompressedSize,
        localHeaderOffset,
      };
      centralEntries.push(entry);
      if (index === 0) firstCentralEntry = entry;
      cursor = recordEnd;
    }

    if (
      cursor !== centralDirectoryOffset + centralDirectorySize
      || !hasNonOverlappingLocalFileRecords(bytes, view, centralEntries, centralDirectoryOffset)
      || !firstCentralEntry
      || firstCentralEntry.name !== 'mimetype'
      || firstCentralEntry.method !== 0
      || firstCentralEntry.localHeaderOffset !== 0
      || firstCentralEntry.compressedSize !== HWPX_MIME_TYPE.length
      || firstCentralEntry.uncompressedSize !== HWPX_MIME_TYPE.length
      || !REQUIRED_PARTS.every((part) => names.includes(part))
      || !names.some((name) => /^Contents\/section\d+\.xml$/.test(name))
    ) {
      return false;
    }

    const localOffset = firstCentralEntry.localHeaderOffset;
    if (localOffset + 30 > bytes.byteLength || readUint32(view, localOffset) !== LOCAL_FILE_HEADER) {
      return false;
    }

    const localFlags = readUint16(view, localOffset + 6);
    const localMethod = readUint16(view, localOffset + 8);
    const localCrc32 = readUint32(view, localOffset + 14);
    const localCompressedSize = readUint32(view, localOffset + 18);
    const localUncompressedSize = readUint32(view, localOffset + 22);
    const localNameLength = readUint16(view, localOffset + 26);
    const localExtraLength = readUint16(view, localOffset + 28);
    const localNameStart = localOffset + 30;
    const localDataStart = localNameStart + localNameLength + localExtraLength;
    const localDataEnd = localDataStart + firstCentralEntry.compressedSize;

    if (
      localDataEnd > bytes.byteLength
      || (localFlags & 0x0049) !== 0
      || localFlags !== firstCentralEntry.flags
      || localMethod !== 0
      || localMethod !== firstCentralEntry.method
      || localCrc32 !== firstCentralEntry.crc32
      || localCompressedSize !== firstCentralEntry.compressedSize
      || localUncompressedSize !== firstCentralEntry.uncompressedSize
      || decoder.decode(bytes.subarray(localNameStart, localNameStart + localNameLength)) !== 'mimetype'
    ) {
      return false;
    }

    const mimeBytes = bytes.subarray(localDataStart, localDataEnd);
    if (
      mimeBytes.byteLength !== HWPX_MIME_TYPE.length
      || calculateCrc32(mimeBytes) !== firstCentralEntry.crc32
    ) {
      return false;
    }
    for (let index = 0; index < mimeBytes.length; index += 1) {
      if (mimeBytes[index] !== HWPX_MIME_TYPE.charCodeAt(index)) return false;
    }
    return true;
  } catch {
    return false;
  }
}
