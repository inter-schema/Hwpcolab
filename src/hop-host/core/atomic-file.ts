import {
  constants,
  closeSync,
  copyFileSync,
  existsSync,
  fsyncSync,
  openSync,
  renameSync,
  unlinkSync,
} from 'node:fs';
import { basename, dirname, join } from 'node:path';
import { randomUUID } from 'node:crypto';

/**
 * Copy a staging file to its destination without exposing a partially written
 * destination. The temporary sibling is fully copied and fsynced before the
 * final rename, and is removed on every failure path.
 */
export function copyFileAtomically(sourcePath: string, targetPath: string): void {
  const targetDirectory = dirname(targetPath);
  const targetName = basename(targetPath);
  const temporaryPath = join(
    targetDirectory,
    '.' + targetName + '.hop-tmp-' + randomUUID(),
  );
  let fileDescriptor: number | undefined;

  try {
    copyFileSync(sourcePath, temporaryPath, constants.COPYFILE_EXCL);
    fileDescriptor = openSync(temporaryPath, 'r+');
    fsyncSync(fileDescriptor);
    closeSync(fileDescriptor);
    fileDescriptor = undefined;

    renameSync(temporaryPath, targetPath);

    // Directory fsync is best-effort; opening directories is not supported
    // uniformly on Windows, while the file data itself was synced above.
    try {
      const directoryDescriptor = openSync(targetDirectory, 'r');
      try {
        fsyncSync(directoryDescriptor);
      } finally {
        closeSync(directoryDescriptor);
      }
    } catch {
      // The atomic rename has already succeeded. Do not report a false save
      // failure just because this platform cannot fsync a directory handle.
    }
  } finally {
    if (fileDescriptor !== undefined) {
      try {
        closeSync(fileDescriptor);
      } catch {
        // Preserve the original write/rename error.
      }
    }
    if (existsSync(temporaryPath)) {
      unlinkSync(temporaryPath);
    }
  }
}
