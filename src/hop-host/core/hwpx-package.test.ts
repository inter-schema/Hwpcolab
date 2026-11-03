import { describe, expect, it } from 'vitest';
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
});
