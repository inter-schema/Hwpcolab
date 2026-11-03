# Beta Branch Review Log

> Working notes for the ongoing Hwpcolab Beta audit. This file is intentionally kept on Beta only; do not modify main as part of this work.

## Scope and baseline

- Repository: `inter-schema/Hwpcolab`
- Target branch: `Beta`
- Baseline HEAD checked after the ChatGPT Codex Connector installation: `0c23baaf63a49e75fd176a8b18d182acfa91546e`
- The current Beta branch is unprotected according to the GitHub branch API response.
- The GitHub App installation is scoped to this one repository, not all repositories.

## Findings from the earlier read-only pass

These findings must be revalidated against the current Beta files before changing code.

1. **Desktop local-font detection**
   - File: `src/hop-host/core/desktop-local-fonts.ts`
   - Earlier inspection found an unconditional early `return false` in `isDesktopTauriRuntime()`, preventing the Tauri-native local-font path from being reached.
   - The frontend appears to call `list_local_fonts` and `read_local_font`, while the Tauri command registration and native catalog implementation were not present in the inspected `src-tauri` files.
   - Revalidate current command wiring and compare with HOP's `fontdb`-based implementation before porting anything.

2. **Save durability**
   - File: `src-tauri/src/commands/document.rs`
   - Earlier inspection found staging output copied to the destination with `fs::copy`.
   - Evaluate temp-file write, flush/sync, verification, atomic replacement, and rollback/recovery behavior, including Windows replacement semantics.

3. **HWPX identification**
   - Earlier inspection found ZIP `PK` magic used as the HWPX discriminator.
   - Validate package members, `mimetype`, and required HWPX structure so arbitrary ZIP files are not misidentified.

4. **DOCX XML parsing**
   - File: `src/core/multiFileParser.ts`
   - Earlier inspection found regex-based XML extraction.
   - Test namespaces, entities, tabs, line breaks, and tables; prefer a real XML parser where the current implementation is lossy.

5. **Font development-server path validation**
   - File: `vite.config.ts`
   - Earlier inspection found a string-prefix path containment check using `startsWith(fontsDir)`.
   - Replace with canonical/relative path containment and traversal regression tests if the issue remains.

6. **Test harness**
   - Earlier inspection found Vitest tests under `src/hop-host/core`, but no corresponding root dependency/scripts in the inspected package files.
   - Verify current package scripts and CI before adding or changing test infrastructure.

7. **Dependency maintenance**
   - Review Vite, PDF.js, SheetJS, React, and other dependencies against current upstream releases and security advisories.
   - Do not perform blind bulk upgrades. Require build, tests, and document round-trip checks.

## Work protocol

- Re-read each affected file from the current `Beta` head before editing.
- Make small, isolated changes with regression tests.
- Run the narrow tests first, then the full available test/build checks.
- Inspect the resulting diff and verify HWP/HWPX-related paths for compatibility.
- Keep all commits on `Beta`; do not change `main`.
- Update this log with each completed fix, test command, result, and remaining risk.
- Do not claim a fix is complete until the relevant tests have passed.
