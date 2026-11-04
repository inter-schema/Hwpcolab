const LOCAL_FILE_HEADER = 0x04034b50;
const CENTRAL_DIRECTORY_HEADER = 0x02014b50;
const END_OF_CENTRAL_DIRECTORY = 0x06054b50;
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

function findEndOfCentralDirectory(view: DataView, bytesLength: number): number {
  const minimumOffset = Math.max(0, bytesLength - 22 - MAX_ZIP_COMMENT_LENGTH);
  for (let offset = bytesLength - 22; offset >= minimumOffset; offset -= 1) {
    if (readUint32(view, offset) !== END_OF_CENTRAL_DIRECTORY) continue;
    const commentLength = readUint16(view, offset + 20);
    if (offset + 22 + commentLength === bytesLength) return offset;
  }
  return -1;
}

function hasZip64EndOfCentralDirectoryLocator(view: DataView, eocdOffset: number): boolean {
  const locatorOffset = eocdOffset - 20;
  if (locatorOffset < 0 || readUint32(view, locatorOffset) !== ZIP64_END_OF_CENTRAL_DIRECTORY_LOCATOR) {
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
    zip64EndOffset + 56 > locatorOffset
    || readUint32(view, zip64EndOffset) !== ZIP64_END_OF_CENTRAL_DIRECTORY
  ) {
    return false;
  }

  const recordSizeLow = readUint32(view, zip64EndOffset + 4);
  const recordSizeHigh = readUint32(view, zip64EndOffset + 8);
  if (recordSizeHigh !== 0) return true;
  if (recordSizeLow < 44) return false;
  return zip64EndOffset + 12 + recordSizeLow <= locatorOffset;
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

    // Validate the ZIP64 locator structure so a coincidental signature in a
    // central-directory comment is not mistaken for a ZIP64 record.
    if (hasZip64EndOfCentralDirectoryLocator(view, eocdOffset)) return false;

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

    const decoder = new TextDecoder('utf-8', { fatal: true });
    const names: string[] = [];
    const seenNames = new Set<string>();
    let firstCentralEntry: {
      name: string;
      flags: number;
      method: number;
      crc32: number;
      compressedSize: number;
      uncompressedSize: number;
      localHeaderOffset: number;
    } | null = null;
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
      const normalizedName = name.toLowerCase();
      if (seenNames.has(normalizedName)) return false;
      seenNames.add(normalizedName);
      names.push(name);
      if (index === 0) {
        firstCentralEntry = {
          name,
          flags,
          method,
          crc32,
          compressedSize,
          uncompressedSize,
          localHeaderOffset,
        };
      }
      cursor = recordEnd;
    }

    if (
      cursor !== centralDirectoryOffset + centralDirectorySize
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
