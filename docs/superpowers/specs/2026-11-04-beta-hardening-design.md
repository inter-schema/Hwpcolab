# Beta Hardening Design

## Intent

Improve the reliability of local HWP/HWPX editing without replacing the existing Tauri 2, Vite and RHWP architecture. Users should be able to use installed fonts, safely open valid HWPX packages, save without exposing the original file to partial writes, and import DOCX text without losing structure.

## Goals

- Restore the already-designed desktop local-font frontend bridge by implementing the missing Tauri catalog/read commands.
- Limit font-byte reads to supported, canonical files under approved OS/user font directories.
- Identify HWPX from OWPML package structure rather than generic ZIP magic.
- Replace final document files only after a complete same-directory temporary write and durable flush.
- Parse DOCX XML by namespace URI, preserving paragraph, table, cell, tab, break and whitespace semantics.
- Limit Vite font requests to canonical regular files below the fonts asset root.
- Provide repeatable frontend/Rust CI and selective upstream integration.

## Non-goals and anti-goals

- No changes to `main` or unrelated branches.
- No bulk dependency refresh, whole-engine replacement or broad architectural rewrite.
- No copying or bundling of proprietary OS fonts.
- No document upload or external processing introduced.
- No weakening of the existing HWP/HWPX editing compatibility contract.
- No claim that Windows/Rust checks passed unless the Windows workflow actually reports success.

## Approaches considered

1. **Focused hardening in the existing architecture (recommended).** Add small helpers, missing Tauri commands and tests; keep native and Vite development behavior consistent. This creates a reviewable diff and limits document compatibility risk.
2. **Rebase/replace the desktop implementation from upstream HOP.** Rejected because Hwpcolab has app-specific commands and document bridge code. A wholesale replacement would overwrite behavior unrelated to these findings.
3. **Documentation-only audit.** Rejected because the user explicitly authorized implementation and the frontend/native defects can be isolated and tested.

## Design decisions

- Use HOP v0.4.5's font catalog as a reference, not a blind copy. Add a direct fontdb dependency only after confirming API/lock compatibility. Canonicalize both file and allowed roots; use component-aware containment.
- A ZIP is HWPX only when the required OWPML package signature and essential document parts validate. Confirm behavior using repository fixtures and generated minimal archives.
- Save by writing a unique temporary sibling, syncing it, verifying the completed bytes, then using an OS-appropriate replacement operation. A failed operation must leave the old target intact.
- Use a namespace-aware XML parser for DOCX. Preserve the existing `parseLocalAttachment` result shape and human-readable failure handling.
- Vite font serving must handle decoding errors, traversal, sibling-prefix paths and symlink escapes as a normal not-found response.
- Dependency changes are limited to required test tooling, security-patched Vite and narrowly justified parser/archive dependencies. Regenerate locks in CI and verify with locked installs afterwards.

## Constraints

- Work branch: `codex/beta-hardening-2026-11-04`, based on `e46f7230074cab63362f41d46cc1d1153085f745`.
- Final integration target: `Beta` only.
- Preserve the current Tauri command argument and return shape consumed by the frontend.
- Test-first for behavior changes; no completion claim without current CI evidence.
