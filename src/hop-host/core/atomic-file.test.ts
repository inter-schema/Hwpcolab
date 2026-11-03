import { mkdtempSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterEach, describe, expect, it } from 'vitest';
import { copyFileAtomically } from './atomic-file';

const temporaryDirectories: string[] = [];

function createDirectory(): string {
  const directory = mkdtempSync(join(tmpdir(), 'hwpcolab-atomic-'));
  temporaryDirectories.push(directory);
  return directory;
}

afterEach(() => {
  for (const directory of temporaryDirectories.splice(0)) {
    rmSync(directory, { recursive: true, force: true });
  }
});

describe('atomic file replacement', () => {
  it('replaces an existing target with the complete staged bytes', () => {
    const directory = createDirectory();
    const source = join(directory, 'staging.hwp');
    const target = join(directory, 'document.hwp');
    writeFileSync(source, Buffer.from('complete-new-document'));
    writeFileSync(target, Buffer.from('old-document'));

    copyFileAtomically(source, target);

    expect(readFileSync(target, 'utf8')).toBe('complete-new-document');
    expect(readdirSync(directory).sort()).toEqual(['document.hwp', 'staging.hwp']);
  });

  it('leaves the existing target unchanged when the staging file is missing', () => {
    const directory = createDirectory();
    const target = join(directory, 'document.hwp');
    writeFileSync(target, Buffer.from('preserve-this-document'));

    expect(() => copyFileAtomically(join(directory, 'missing.hwp'), target)).toThrow();

    expect(readFileSync(target, 'utf8')).toBe('preserve-this-document');
    expect(readdirSync(directory)).toEqual(['document.hwp']);
  });

  it('preserves an existing destination directory when replacement fails and cleans its temporary file', () => {
    const directory = createDirectory();
    const source = join(directory, 'staging.hwp');
    const targetDirectory = join(directory, 'document.hwp');
    mkdirSync(targetDirectory);
    writeFileSync(join(targetDirectory, 'keep.txt'), 'keep');
    writeFileSync(source, 'new-content');

    expect(() => copyFileAtomically(source, targetDirectory)).toThrow();

    expect(readFileSync(join(targetDirectory, 'keep.txt'), 'utf8')).toBe('keep');
    expect(readdirSync(directory).sort()).toEqual(['document.hwp', 'staging.hwp']);
  });
});
