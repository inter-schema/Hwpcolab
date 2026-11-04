import { describe, expect, it } from 'vitest';
import { readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { strToU8, unzipSync, zipSync, type Zippable } from 'fflate';
import { isValidHwpxPackage, resolveZipDataDescriptorEnd } from './hwpx-package';

const MIME_TYPE = 'application/hwp+zip';


function addDataDescriptorToLastEntry(zip: Uint8Array): Uint8Array {
  const view = new DataView(zip.buffer, zip.byteOffset, zip.byteLength);
  let eocdOffset = -1;
  for (let offset = zip.byteLength - 22; offset >= Math.max(0, zip.byteLength - 22 - 0xffff); offset -= 1) {
    if (view.getUint32(offset, true) === 0x06054b50) {
      eocdOffset = offset;
      break;
    }
  }
  if (eocdOffset < 0) throw new Error('ZIP EOCD not found');

  const entryCount = view.getUint16(eocdOffset + 10, true);
  const centralDirectoryOffset = view.getUint32(eocdOffset + 16, true);
  let cursor = centralDirectoryOffset;
  let lastCentralOffset = -1;
  let lastLocalOffset = -1;
  let lastFlags = 0;
  let lastCrc32 = 0;
  let lastCompressedSize = 0;
  let lastUncompressedSize = 0;

  for (let index = 0; index < entryCount; index += 1) {
    if (view.getUint32(cursor, true) !== 0x02014b50) throw new Error('Invalid central directory');
    lastCentralOffset = cursor;
    lastFlags = view.getUint16(cursor + 8, true);
    lastCrc32 = view.getUint32(cursor + 16, true);
    lastCompressedSize = view.getUint32(cursor + 20, true);
    lastUncompressedSize = view.getUint32(cursor + 24, true);
    lastLocalOffset = view.getUint32(cursor + 42, true);
    cursor += 46
      + view.getUint16(cursor + 28, true)
      + view.getUint16(cursor + 30, true)
      + view.getUint16(cursor + 32, true);
  }

  const localNameLength = view.getUint16(lastLocalOffset + 26, true);
  const localExtraLength = view.getUint16(lastLocalOffset + 28, true);
  const dataEnd = lastLocalOffset + 30 + localNameLength + localExtraLength + lastCompressedSize;
  const descriptor = new Uint8Array(16);
  const descriptorView = new DataView(descriptor.buffer);
  descriptorView.setUint32(0, 0x08074b50, true);
  descriptorView.setUint32(4, lastCrc32, true);
  descriptorView.setUint32(8, lastCompressedSize, true);
  descriptorView.setUint32(12, lastUncompressedSize, true);

  const output = new Uint8Array(zip.byteLength + descriptor.byteLength);
  output.set(zip.subarray(0, dataEnd), 0);
  output.set(descriptor, dataEnd);
  output.set(zip.subarray(dataEnd), dataEnd + descriptor.byteLength);

  const outputView = new DataView(output.buffer);
  outputView.setUint16(lastLocalOffset + 6, lastFlags | 0x0008, true);
  outputView.setUint32(lastLocalOffset + 14, 0, true);
  outputView.setUint32(lastLocalOffset + 18, 0, true);
  outputView.setUint32(lastLocalOffset + 22, 0, true);
  outputView.setUint16(lastCentralOffset + descriptor.byteLength + 8, lastFlags | 0x0008, true);
  outputView.setUint32(eocdOffset + descriptor.byteLength + 16, centralDirectoryOffset + descriptor.byteLength, true);
  return output;
}


function renameZipEntry(zip: Uint8Array, from: string, to: string): Uint8Array {
  const fromBytes = new TextEncoder().encode(from);
  const toBytes = new TextEncoder().encode(to);
  if (fromBytes.byteLength !== toBytes.byteLength) throw new Error('ZIP entry names must have equal byte length');

  const view = new DataView(zip.buffer, zip.byteOffset, zip.byteLength);
  let eocdOffset = -1;
  for (let offset = zip.byteLength - 22; offset >= Math.max(0, zip.byteLength - 22 - 0xffff); offset -= 1) {
    if (view.getUint32(offset, true) === 0x06054b50) {
      eocdOffset = offset;
      break;
    }
  }
  if (eocdOffset < 0) throw new Error('ZIP EOCD not found');

  const entryCount = view.getUint16(eocdOffset + 10, true);
  let cursor = view.getUint32(eocdOffset + 16, true);
  const output = zip.slice();
  for (let index = 0; index < entryCount; index += 1) {
    if (view.getUint32(cursor, true) !== 0x02014b50) throw new Error('Invalid central directory');
    const nameLength = view.getUint16(cursor + 28, true);
    const extraLength = view.getUint16(cursor + 30, true);
    const commentLength = view.getUint16(cursor + 32, true);
    const name = new TextDecoder().decode(zip.subarray(cursor + 46, cursor + 46 + nameLength));
    if (name === from) {
      const localOffset = view.getUint32(cursor + 42, true);
      const localNameLength = view.getUint16(localOffset + 26, true);
      if (localNameLength !== toBytes.byteLength) throw new Error('Unexpected local entry name length');
      output.set(toBytes, cursor + 46);
      output.set(toBytes, localOffset + 30);
      return output;
    }
    cursor += 46 + nameLength + extraLength + commentLength;
  }
  throw new Error(`ZIP entry not found: ${from}`);
}

function addDataDescriptorToMimetype(zip: Uint8Array): Uint8Array {
  const sourceView = new DataView(zip.buffer, zip.byteOffset, zip.byteLength);
  let eocdOffset = -1;
  for (let offset = zip.byteLength - 22; offset >= Math.max(0, zip.byteLength - 22 - 0xffff); offset -= 1) {
    if (sourceView.getUint32(offset, true) === 0x06054b50) {
      eocdOffset = offset;
      break;
    }
  }
  if (eocdOffset < 0) throw new Error('ZIP EOCD not found');

  const entryCount = sourceView.getUint16(eocdOffset + 10, true);
  const centralOffset = sourceView.getUint32(eocdOffset + 16, true);
  if (sourceView.getUint32(centralOffset, true) !== 0x02014b50) throw new Error('Invalid first central entry');
  const crc32 = sourceView.getUint32(centralOffset + 16, true);
  const compressedSize = sourceView.getUint32(centralOffset + 20, true);
  const uncompressedSize = sourceView.getUint32(centralOffset + 24, true);
  const localOffset = sourceView.getUint32(centralOffset + 42, true);
  const localNameLength = sourceView.getUint16(localOffset + 26, true);
  const localExtraLength = sourceView.getUint16(localOffset + 28, true);
  const dataEnd = localOffset + 30 + localNameLength + localExtraLength + compressedSize;

  const descriptor = new Uint8Array(16);
  const descriptorView = new DataView(descriptor.buffer);
  descriptorView.setUint32(0, 0x08074b50, true);
  descriptorView.setUint32(4, crc32, true);
  descriptorView.setUint32(8, compressedSize, true);
  descriptorView.setUint32(12, uncompressedSize, true);

  const output = new Uint8Array(zip.byteLength + descriptor.byteLength);
  output.set(zip.subarray(0, dataEnd), 0);
  output.set(descriptor, dataEnd);
  output.set(zip.subarray(dataEnd), dataEnd + descriptor.byteLength);

  const view = new DataView(output.buffer);
  const newCentralOffset = centralOffset + descriptor.byteLength;
  let cursor = newCentralOffset;
  for (let index = 0; index < entryCount; index += 1) {
    if (view.getUint32(cursor, true) !== 0x02014b50) throw new Error('Invalid central directory');
    if (index === 0) view.setUint16(cursor + 8, view.getUint16(cursor + 8, true) | 0x0008, true);
    const entryLocalOffset = view.getUint32(cursor + 42, true);
    if (entryLocalOffset >= dataEnd) view.setUint32(cursor + 42, entryLocalOffset + descriptor.byteLength, true);
    cursor += 46
      + view.getUint16(cursor + 28, true)
      + view.getUint16(cursor + 30, true)
      + view.getUint16(cursor + 32, true);
  }

  const localFlags = view.getUint16(localOffset + 6, true);
  view.setUint16(localOffset + 6, localFlags | 0x0008, true);
  view.setUint32(localOffset + 14, 0, true);
  view.setUint32(localOffset + 18, 0, true);
  view.setUint32(localOffset + 22, 0, true);
  view.setUint32(eocdOffset + descriptor.byteLength + 16, newCentralOffset, true);
  return output;
}

function corruptLastDataDescriptorField(zip: Uint8Array, fieldOffset: number): Uint8Array {
  const output = addDataDescriptorToLastEntry(zip).slice();
  const view = new DataView(output.buffer, output.byteOffset, output.byteLength);
  let eocdOffset = -1;
  for (let offset = output.byteLength - 22; offset >= Math.max(0, output.byteLength - 22 - 0xffff); offset -= 1) {
    if (view.getUint32(offset, true) === 0x06054b50) {
      eocdOffset = offset;
      break;
    }
  }
  if (eocdOffset < 0) throw new Error('ZIP EOCD not found');

  const entryCount = view.getUint16(eocdOffset + 10, true);
  let cursor = view.getUint32(eocdOffset + 16, true);
  let lastCentralOffset = -1;
  for (let index = 0; index < entryCount; index += 1) {
    if (view.getUint32(cursor, true) !== 0x02014b50) throw new Error('Invalid central directory');
    lastCentralOffset = cursor;
    cursor += 46
      + view.getUint16(cursor + 28, true)
      + view.getUint16(cursor + 30, true)
      + view.getUint16(cursor + 32, true);
  }
  if (lastCentralOffset < 0) throw new Error('No central directory entries');

  const localOffset = view.getUint32(lastCentralOffset + 42, true);
  const localNameLength = view.getUint16(localOffset + 26, true);
  const localExtraLength = view.getUint16(localOffset + 28, true);
  const compressedSize = view.getUint32(lastCentralOffset + 20, true);
  const descriptorOffset = localOffset + 30 + localNameLength + localExtraLength + compressedSize;
  if (view.getUint32(descriptorOffset, true) !== 0x08074b50) {
    throw new Error('Expected a signed data descriptor');
  }
  view.setUint32(
    descriptorOffset + fieldOffset,
    view.getUint32(descriptorOffset + fieldOffset, true) ^ 1,
    true,
  );
  return output;
}

function removeLastDataDescriptorSignature(zip: Uint8Array): Uint8Array {
  const output = addDataDescriptorToLastEntry(zip).slice();
  const view = new DataView(output.buffer, output.byteOffset, output.byteLength);
  let eocdOffset = -1;
  for (let offset = output.byteLength - 22; offset >= Math.max(0, output.byteLength - 22 - 0xffff); offset -= 1) {
    if (view.getUint32(offset, true) === 0x06054b50) {
      eocdOffset = offset;
      break;
    }
  }
  if (eocdOffset < 0) throw new Error('ZIP EOCD not found');

  const oldCentralOffset = view.getUint32(eocdOffset + 16, true);
  const entryCount = view.getUint16(eocdOffset + 10, true);
  let cursor = oldCentralOffset;
  let lastCentralOffset = -1;
  for (let index = 0; index < entryCount; index += 1) {
    if (view.getUint32(cursor, true) !== 0x02014b50) throw new Error('Invalid central directory');
    lastCentralOffset = cursor;
    cursor += 46
      + view.getUint16(cursor + 28, true)
      + view.getUint16(cursor + 30, true)
      + view.getUint16(cursor + 32, true);
  }
  if (lastCentralOffset < 0) throw new Error('No central directory entries');
  const localOffset = view.getUint32(lastCentralOffset + 42, true);
  const localNameLength = view.getUint16(localOffset + 26, true);
  const localExtraLength = view.getUint16(localOffset + 28, true);
  const compressedSize = view.getUint32(lastCentralOffset + 20, true);
  const descriptorOffset = localOffset + 30 + localNameLength + localExtraLength + compressedSize;
  if (view.getUint32(descriptorOffset, true) !== 0x08074b50) {
    throw new Error('Expected a signed data descriptor');
  }

  const unsigned = new Uint8Array(output.byteLength - 4);
  unsigned.set(output.subarray(0, descriptorOffset));
  unsigned.set(output.subarray(descriptorOffset + 4), descriptorOffset);
  const unsignedView = new DataView(unsigned.buffer, unsigned.byteOffset, unsigned.byteLength);
  let unsignedEocdOffset = -1;
  for (let offset = unsigned.byteLength - 22; offset >= Math.max(0, unsigned.byteLength - 22 - 0xffff); offset -= 1) {
    if (unsignedView.getUint32(offset, true) === 0x06054b50) {
      unsignedEocdOffset = offset;
      break;
    }
  }
  if (unsignedEocdOffset < 0) throw new Error('ZIP EOCD not found after signature removal');
  unsignedView.setUint32(unsignedEocdOffset + 16, oldCentralOffset - 4, true);
  return unsigned;
}

function renameMimetypeLocalHeaderOnly(zip: Uint8Array): Uint8Array {
  const output = zip.slice();
  const view = new DataView(output.buffer, output.byteOffset, output.byteLength);
  let eocdOffset = -1;
  for (let offset = output.byteLength - 22; offset >= Math.max(0, output.byteLength - 22 - 0xffff); offset -= 1) {
    if (view.getUint32(offset, true) === 0x06054b50) {
      eocdOffset = offset;
      break;
    }
  }
  if (eocdOffset < 0) throw new Error('ZIP EOCD not found');

  const centralOffset = view.getUint32(eocdOffset + 16, true);
  if (view.getUint32(centralOffset, true) !== 0x02014b50) throw new Error('Invalid first central entry');
  const localOffset = view.getUint32(centralOffset + 42, true);
  const nameLength = view.getUint16(localOffset + 26, true);
  const replacement = new TextEncoder().encode('mimetyqe');
  if (nameLength !== replacement.byteLength) throw new Error('Unexpected mimetype local name length');
  output.set(replacement, localOffset + 30);
  return output;
}

function renameRegularEntryLocalHeaderOnly(zip: Uint8Array): Uint8Array {
  const output = zip.slice();
  const view = new DataView(output.buffer, output.byteOffset, output.byteLength);
  const targetName = new TextEncoder().encode('Contents/section0.xml');
  const replacement = new TextEncoder().encode('Contents/sectionX.xml');
  let eocdOffset = -1;
  for (let offset = output.byteLength - 22; offset >= Math.max(0, output.byteLength - 22 - 0xffff); offset -= 1) {
    if (view.getUint32(offset, true) === 0x06054b50) {
      eocdOffset = offset;
      break;
    }
  }
  if (eocdOffset < 0) throw new Error('ZIP EOCD not found');

  const entryCount = view.getUint16(eocdOffset + 10, true);
  let cursor = view.getUint32(eocdOffset + 16, true);
  for (let index = 0; index < entryCount; index += 1) {
    if (view.getUint32(cursor, true) !== 0x02014b50) throw new Error('Invalid central directory');
    const nameLength = view.getUint16(cursor + 28, true);
    const extraLength = view.getUint16(cursor + 30, true);
    const commentLength = view.getUint16(cursor + 32, true);
    const nameStart = cursor + 46;
    const nameEnd = nameStart + nameLength;
    const name = output.subarray(nameStart, nameEnd);
    if (name.byteLength === targetName.byteLength && name.every((byte, i) => byte === targetName[i])) {
      const localOffset = view.getUint32(cursor + 42, true);
      const localNameLength = view.getUint16(localOffset + 26, true);
      if (localNameLength !== replacement.byteLength) throw new Error('Unexpected local entry name length');
      output.set(replacement, localOffset + 30);
      return output;
    }
    cursor = nameEnd + extraLength + commentLength;
  }
  throw new Error('Regular package part not found');
}

function corruptMimetypeCrc(zip: Uint8Array): Uint8Array {
  const output = zip.slice();
  const view = new DataView(output.buffer, output.byteOffset, output.byteLength);
  let eocdOffset = -1;
  for (let offset = output.byteLength - 22; offset >= Math.max(0, output.byteLength - 22 - 0xffff); offset -= 1) {
    if (view.getUint32(offset, true) === 0x06054b50) {
      eocdOffset = offset;
      break;
    }
  }
  if (eocdOffset < 0) throw new Error('ZIP EOCD not found');

  const centralOffset = view.getUint32(eocdOffset + 16, true);
  if (view.getUint32(centralOffset, true) !== 0x02014b50) throw new Error('Invalid first central entry');
  const localOffset = view.getUint32(centralOffset + 42, true);
  const incorrectCrc = view.getUint32(centralOffset + 16, true) ^ 1;
  view.setUint32(centralOffset + 16, incorrectCrc, true);
  view.setUint32(localOffset + 14, incorrectCrc, true);
  return output;
}

function addZip64EndRecords(zip: Uint8Array): Uint8Array {
  const view = new DataView(zip.buffer, zip.byteOffset, zip.byteLength);
  let eocdOffset = -1;
  for (let offset = zip.byteLength - 22; offset >= Math.max(0, zip.byteLength - 22 - 0xffff); offset -= 1) {
    if (view.getUint32(offset, true) === 0x06054b50) {
      eocdOffset = offset;
      break;
    }
  }
  if (eocdOffset < 0) throw new Error('ZIP EOCD not found');

  const entries = view.getUint16(eocdOffset + 10, true);
  const directorySize = view.getUint32(eocdOffset + 12, true);
  const directoryOffset = view.getUint32(eocdOffset + 16, true);

  const zip64End = new Uint8Array(56);
  const zip64EndView = new DataView(zip64End.buffer);
  zip64EndView.setUint32(0, 0x06064b50, true);
  zip64EndView.setBigUint64(4, 44n, true);
  zip64EndView.setUint16(12, 45, true);
  zip64EndView.setUint16(14, 45, true);
  zip64EndView.setUint32(16, 0, true);
  zip64EndView.setUint32(20, 0, true);
  zip64EndView.setBigUint64(24, BigInt(entries), true);
  zip64EndView.setBigUint64(32, BigInt(entries), true);
  zip64EndView.setBigUint64(40, BigInt(directorySize), true);
  zip64EndView.setBigUint64(48, BigInt(directoryOffset), true);

  const locator = new Uint8Array(20);
  const locatorView = new DataView(locator.buffer);
  locatorView.setUint32(0, 0x07064b50, true);
  locatorView.setUint32(4, 0, true);
  locatorView.setBigUint64(8, BigInt(eocdOffset), true);
  locatorView.setUint32(16, 1, true);

  const output = new Uint8Array(zip.byteLength + zip64End.byteLength + locator.byteLength);
  const insertionOffset = eocdOffset;
  output.set(zip.subarray(0, insertionOffset), 0);
  output.set(zip64End, insertionOffset);
  output.set(locator, insertionOffset + zip64End.byteLength);
  output.set(zip.subarray(insertionOffset), insertionOffset + zip64End.byteLength + locator.byteLength);
  return output;
}

function addCentralDirectoryCommentThatResemblesZip64Locator(zip: Uint8Array): Uint8Array {
  const view = new DataView(zip.buffer, zip.byteOffset, zip.byteLength);
  let eocdOffset = -1;
  for (let offset = zip.byteLength - 22; offset >= Math.max(0, zip.byteLength - 22 - 0xffff); offset -= 1) {
    if (view.getUint32(offset, true) === 0x06054b50) {
      eocdOffset = offset;
      break;
    }
  }
  if (eocdOffset < 0) throw new Error('ZIP EOCD not found');

  const count = view.getUint16(eocdOffset + 10, true);
  const directoryOffset = view.getUint32(eocdOffset + 16, true);
  let cursor = directoryOffset;
  let lastCentralEntry = -1;
  for (let index = 0; index < count; index += 1) {
    if (view.getUint32(cursor, true) !== 0x02014b50) throw new Error('Invalid central directory');
    lastCentralEntry = cursor;
    cursor += 46
      + view.getUint16(cursor + 28, true)
      + view.getUint16(cursor + 30, true)
      + view.getUint16(cursor + 32, true);
  }

  const comment = new Uint8Array([
    0x50, 0x4b, 0x06, 0x07,
    0, 0, 0, 0,
    0, 0, 0, 0,
    0, 0, 0, 0,
    1, 0, 0, 0,
  ]);
  const output = new Uint8Array(zip.byteLength + comment.byteLength);
  output.set(zip.subarray(0, eocdOffset), 0);
  output.set(comment, eocdOffset);
  output.set(zip.subarray(eocdOffset), eocdOffset + comment.byteLength);

  const outputView = new DataView(output.buffer);
  outputView.setUint16(lastCentralEntry + 32, comment.byteLength, true);
  outputView.setUint32(eocdOffset + comment.byteLength + 12, cursor - directoryOffset + comment.byteLength, true);
  return output;
}

function makeValidEntries(): Zippable {
  return {
    mimetype: [strToU8(MIME_TYPE), { level: 0 }],
    'version.xml': strToU8('<HCFVersion version="1.5.0.0"/>'),
    'Contents/content.hpf': strToU8('<opf:package/>'),
    'Contents/header.xml': strToU8('<hh:head/>'),
    'Contents/section0.xml': strToU8('<hs:sec/>'),
    'Contents/metadata.xml': strToU8('<meta/>'),
  };
}

function buildOverlappingLocalRecordsArchive(): Uint8Array {
  const metadataName = 'Contents/metadata.xml';
  const embeddedName = new TextEncoder().encode(metadataName);
  const embeddedRecord = new Uint8Array(30 + embeddedName.byteLength);
  const embeddedView = new DataView(embeddedRecord.buffer);
  embeddedView.setUint32(0, 0x04034b50, true);
  embeddedView.setUint16(4, 20, true);
  embeddedView.setUint16(26, embeddedName.byteLength, true);
  embeddedRecord.set(embeddedName, 30);

  const payload = new Uint8Array(512);
  payload.set(embeddedRecord, 64);
  const entries = makeValidEntries();
  entries['Contents/section0.xml'] = [strToU8(String.fromCharCode(...payload)), { level: 0 }];
  entries[metadataName] = [new Uint8Array(0), { level: 0 }];
  const archive = zipSync(entries);
  const view = new DataView(archive.buffer, archive.byteOffset, archive.byteLength);

  let eocdOffset = -1;
  for (let offset = archive.byteLength - 22; offset >= Math.max(0, archive.byteLength - 22 - 0xffff); offset -= 1) {
    if (view.getUint32(offset, true) === 0x06054b50) {
      eocdOffset = offset;
      break;
    }
  }
  if (eocdOffset < 0) throw new Error('ZIP EOCD not found');

  const entryCount = view.getUint16(eocdOffset + 10, true);
  let cursor = view.getUint32(eocdOffset + 16, true);
  let sectionCentralOffset = -1;
  let metadataCentralOffset = -1;
  const decoder = new TextDecoder();
  for (let index = 0; index < entryCount; index += 1) {
    if (view.getUint32(cursor, true) !== 0x02014b50) throw new Error('Invalid central directory');
    const nameLength = view.getUint16(cursor + 28, true);
    const extraLength = view.getUint16(cursor + 30, true);
    const commentLength = view.getUint16(cursor + 32, true);
    const name = decoder.decode(archive.subarray(cursor + 46, cursor + 46 + nameLength));
    if (name === 'Contents/section0.xml') sectionCentralOffset = cursor;
    if (name === metadataName) metadataCentralOffset = cursor;
    cursor += 46 + nameLength + extraLength + commentLength;
  }
  if (sectionCentralOffset < 0 || metadataCentralOffset < 0) {
    throw new Error('Required central directory entries not found');
  }

  const sectionLocalOffset = view.getUint32(sectionCentralOffset + 42, true);
  const sectionNameLength = view.getUint16(sectionLocalOffset + 26, true);
  const sectionExtraLength = view.getUint16(sectionLocalOffset + 28, true);
  const sectionDataStart = sectionLocalOffset + 30 + sectionNameLength + sectionExtraLength;
  view.setUint32(metadataCentralOffset + 42, sectionDataStart + 64, true);
  return archive;
}

describe('HWPX package validation', () => {
  it('accepts a valid OWPML ZIP with the stored mimetype first', () => {
    expect(isValidHwpxPackage(zipSync(makeValidEntries()))).toBe(true);
  });

  it('rejects a generic ZIP without the HWPX mimetype', () => {
    expect(isValidHwpxPackage(zipSync({
      'notes.txt': strToU8('not a HWPX document'),
    }))).toBe(false);
  });

  it('rejects an archive containing an entry with an empty filename', () => {
    const entries = makeValidEntries();
    entries[''] = strToU8('');
    expect(isValidHwpxPackage(zipSync(entries))).toBe(false);
  });

  it('rejects a package with an incorrect mimetype', () => {
    const entries = makeValidEntries();
    entries.mimetype = [strToU8('application/zip'), { level: 0 }];
    expect(isValidHwpxPackage(zipSync(entries))).toBe(false);
  });

  it('rejects a package where mimetype is not the first local entry', () => {
    const entries: Zippable = {
      'first.txt': strToU8('wrong order'),
      ...makeValidEntries(),
    };
    expect(isValidHwpxPackage(zipSync(entries))).toBe(false);
  });

  it('rejects a compressed mimetype entry', () => {
    const entries = makeValidEntries();
    entries.mimetype = [strToU8(MIME_TYPE), { level: 6 }];
    expect(isValidHwpxPackage(zipSync(entries))).toBe(false);
  });

  it('requires the content package, header and at least one section', () => {
    const missingContent = makeValidEntries();
    delete missingContent['Contents/content.hpf'];
    expect(isValidHwpxPackage(zipSync(missingContent))).toBe(false);

    const missingHeader = makeValidEntries();
    delete missingHeader['Contents/header.xml'];
    expect(isValidHwpxPackage(zipSync(missingHeader))).toBe(false);

    const missingSection = makeValidEntries();
    delete missingSection['Contents/section0.xml'];
    expect(isValidHwpxPackage(zipSync(missingSection))).toBe(false);
  });


  it('rejects duplicate package part names instead of accepting ambiguous ZIP entries', () => {
    const archive = renameZipEntry(
      zipSync(makeValidEntries()),
      'Contents/metadata.xml',
      'Contents/section0.xml',
    );
    expect(isValidHwpxPackage(archive)).toBe(false);
  });

  it('rejects case-insensitive duplicate package part names', () => {
    const archive = renameZipEntry(
      zipSync(makeValidEntries()),
      'Contents/metadata.xml',
      'contents/SECTION0.xml',
    );
    expect(isValidHwpxPackage(archive)).toBe(false);
  });

  it('rejects Unicode case-insensitive duplicate package part names', () => {
    const entries = makeValidEntries();
    entries['Contents/Ä.xml'] = strToU8('<x/>');
    entries['Contents/ä.xml'] = strToU8('<x/>');
    const archive = zipSync(entries);
    const view = new DataView(archive.buffer, archive.byteOffset, archive.byteLength);
    let eocdOffset = -1;
    for (let offset = archive.byteLength - 22; offset >= Math.max(0, archive.byteLength - 22 - 0xffff); offset -= 1) {
      if (view.getUint32(offset, true) === 0x06054b50) {
        eocdOffset = offset;
        break;
      }
    }
    expect(eocdOffset).toBeGreaterThanOrEqual(0);
    const count = view.getUint16(eocdOffset + 10, true);
    let cursor = view.getUint32(eocdOffset + 16, true);
    const decoder = new TextDecoder();
    const utf8Entries = new Set<string>();
    for (let index = 0; index < count; index += 1) {
      const flags = view.getUint16(cursor + 8, true);
      const nameLength = view.getUint16(cursor + 28, true);
      const extraLength = view.getUint16(cursor + 30, true);
      const commentLength = view.getUint16(cursor + 32, true);
      const name = decoder.decode(archive.subarray(cursor + 46, cursor + 46 + nameLength));
      if (name === 'Contents/Ä.xml' || name === 'Contents/ä.xml') {
        expect(flags & 0x0800).toBe(0x0800);
        utf8Entries.add(name);
      }
      cursor += 46 + nameLength + extraLength + commentLength;
    }
    expect(utf8Entries.size).toBe(2);
    expect(isValidHwpxPackage(archive)).toBe(false);
  });

  it('rejects full Unicode case-fold duplicates such as sharp-s and ss', () => {
    const entries = makeValidEntries();
    entries['Contents/Straße.xml'] = strToU8('<x/>');
    entries['Contents/STRASSE.xml'] = strToU8('<x/>');
    expect(isValidHwpxPackage(zipSync(entries))).toBe(false);
  });

  it('rejects canonically equivalent NFC and NFD package part names', () => {
    const entries = makeValidEntries();
    entries['Contents/café.xml'] = strToU8('<x/>');
    entries['Contents/cafe\u0301.xml'] = strToU8('<x/>');
    expect(isValidHwpxPackage(zipSync(entries))).toBe(false);
  });

  it('rejects canonically equivalent precomposed Hangul and decomposed Jamo names', () => {
    const entries = makeValidEntries();
    entries['Contents/각.xml'] = strToU8('<x/>');
    entries['Contents/\u1100\u1161\u11a8.xml'] = strToU8('<x/>');
    expect(isValidHwpxPackage(zipSync(entries))).toBe(false);
  });

  it('rejects full case-fold collisions beyond sharp-s', () => {
    const collisions = [
      ['Contents/Σ.xml', 'Contents/ς.xml'],
      ['Contents/ﬃ.xml', 'Contents/FFI.xml'],
      ['Contents/K.xml', 'Contents/k.xml'],
    ] as const;

    for (const [left, right] of collisions) {
      const entries = makeValidEntries();
      entries[left] = strToU8('<x/>');
      entries[right] = strToU8('<x/>');
      expect(isValidHwpxPackage(zipSync(entries)), `${left} / ${right}`).toBe(false);
    }
  });

  it('rejects a mimetype entry marked as using a data descriptor', () => {
    const archive = addDataDescriptorToMimetype(zipSync(makeValidEntries()));
    expect(unzipSync(archive).mimetype).toEqual(strToU8(MIME_TYPE));
    expect(isValidHwpxPackage(archive)).toBe(false);
  });

  it('rejects a mimetype local name that differs from its central directory name', () => {
    const archive = renameMimetypeLocalHeaderOnly(zipSync(makeValidEntries()));
    expect(isValidHwpxPackage(archive)).toBe(false);
  });

  it('rejects a regular package part whose local name differs from its central directory name', () => {
    const archive = renameRegularEntryLocalHeaderOnly(zipSync(makeValidEntries()));
    expect(isValidHwpxPackage(archive)).toBe(false);
  });

  it('rejects overlapping local file records even when each header matches its central entry', () => {
    const archive = buildOverlappingLocalRecordsArchive();
    const extracted = unzipSync(archive);
    expect(extracted['Contents/metadata.xml']).toBeDefined();
    expect(extracted['Contents/section0.xml']).toHaveLength(512);
    expect(isValidHwpxPackage(archive)).toBe(false);
  });

  it('rejects a mimetype whose CRC does not match its payload', () => {
    const archive = corruptMimetypeCrc(zipSync(makeValidEntries()));
    expect(isValidHwpxPackage(archive)).toBe(false);
  });

  it('rejects ZIP64 archives even when the classic EOCD fields still fit', () => {
    const archive = addZip64EndRecords(zipSync(makeValidEntries()));
    expect(isValidHwpxPackage(archive)).toBe(false);
  });

  it('does not mistake a central-directory file comment for a ZIP64 locator', () => {
    const archive = addCentralDirectoryCommentThatResemblesZip64Locator(zipSync(makeValidEntries()));
    expect(isValidHwpxPackage(archive)).toBe(true);
  });

  it('accepts data descriptors on ordinary entries while keeping mimetype strict', () => {
    const archive = addDataDescriptorToLastEntry(zipSync(makeValidEntries()));
    expect(unzipSync(archive)['Contents/section0.xml']).toBeDefined();
    expect(isValidHwpxPackage(archive)).toBe(true);
  });

  it('accepts ordinary entries with an unsigned data descriptor', () => {
    const archive = removeLastDataDescriptorSignature(zipSync(makeValidEntries()));
    expect(isValidHwpxPackage(archive)).toBe(true);
  });

  it('rejects an ordinary entry with a data descriptor CRC that disagrees with the central directory', () => {
    const archive = corruptLastDataDescriptorField(zipSync(makeValidEntries()), 4);
    expect(isValidHwpxPackage(archive)).toBe(false);
  });

  it('rejects a data descriptor with a compressed size that disagrees with the central directory', () => {
    const archive = corruptLastDataDescriptorField(zipSync(makeValidEntries()), 8);
    expect(isValidHwpxPackage(archive)).toBe(false);
  });

  it('rejects a data descriptor with an uncompressed size that disagrees with the central directory', () => {
    const archive = corruptLastDataDescriptorField(zipSync(makeValidEntries()), 12);
    expect(isValidHwpxPackage(archive)).toBe(false);
  });

  it('prefers the signed descriptor when signed and unsigned interpretations both match', () => {
    const signature = 0x08074b50;
    const descriptor = new Uint8Array(16);
    const view = new DataView(descriptor.buffer);
    for (let offset = 0; offset < descriptor.byteLength; offset += 4) {
      view.setUint32(offset, signature, true);
    }

    expect(resolveZipDataDescriptorEnd(view, 0, signature, signature, signature, 16)).toBe(16);
  });

  it('accepts the repository HWPX samples, not only generated fixtures', () => {
    const testDirectory = dirname(fileURLToPath(import.meta.url));
    const samples = [
      '(날짜)(교시)(과목명)교수.학습과정안(이름).hwpx',
      'demo_official_report.hwpx',
    ];

    for (const sample of samples) {
      const bytes = new Uint8Array(readFileSync(resolve(testDirectory, '../../../samples', sample)));
      expect(isValidHwpxPackage(bytes), sample).toBe(true);
    }
  });

});
