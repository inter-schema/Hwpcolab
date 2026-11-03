import { describe, expect, it } from 'vitest';
import { readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { strToU8, zipSync, type Zippable } from 'fflate';
import { isValidHwpxPackage } from './hwpx-package';

const MIME_TYPE = 'application/hwp+zip';

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
