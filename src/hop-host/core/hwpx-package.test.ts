import { describe, expect, it } from 'vitest';
import { readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { strToU8, unzipSync, zipSync, type Zippable } from 'fflate';
import { isValidHwpxPackage } from './hwpx-package';

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

function makeValidEntries(): Zippable {
  return {
    mimetype: [strToU8(MIME_TYPE), { level: 0 }],
    'version.xml': strToU8('<HCFVersion version="1.5.0.0"/>'),
    'Contents/content.hpf': strToU8('<opf:package/>'),
    'Contents/header.xml': strToU8('<hh:head/>'),
    'Contents/section0.xml': strToU8('<hs:sec/>'),
  };
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
      'META-INF/manifest.xml',
      'Contents/section0.xml',
    );
    expect(isValidHwpxPackage(archive)).toBe(false);
  });

  it('accepts data descriptors on ordinary entries while keeping mimetype strict', () => {
    const archive = addDataDescriptorToLastEntry(zipSync(makeValidEntries()));
    expect(unzipSync(archive)['Contents/section0.xml']).toBeDefined();
    expect(isValidHwpxPackage(archive)).toBe(true);
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
