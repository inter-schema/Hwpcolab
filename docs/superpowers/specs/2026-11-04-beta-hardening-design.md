# Beta Hardening Design

> **Purpose:** durable architecture decision record. This file explains constraints, options, technical selections, reasons, and non-goals. It is deliberately not a task checklist or verification ledger. See `docs/superpowers/plans/2026-11-04-beta-hardening-plan.md` for task status and `BETA_REVIEW_LOG.md` for defect descriptions, RED/GREEN evidence, CI results, and chronological review findings.

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
- Treat HWPX package-part names consistently across platforms, including Unicode canonical-equivalence and full case-fold collisions.
- Validate each ZIP local record as a bounded, non-overlapping range consistent with its central-directory entry; support legal ordinary-entry data descriptors without weakening the first/stored `mimetype` rule.
- Preserve ZIP filename bytes' exact Unicode meaning across runtimes; a UTF-8 BOM is filename content, not a stream marker that may be silently consumed.
- Distinguish a real ZIP64 locator/EOCD record from opaque comment or payload bytes that merely resemble those structures.

## Non-goals and anti-goals

- No changes to `main` or unrelated branches.
- No bulk dependency refresh, whole-engine replacement or broad architectural rewrite.
- No copying or bundling of proprietary OS fonts.
- No document upload or external processing introduced.
- No weakening of the existing HWP/HWPX editing compatibility contract.
- No claim that Windows/Rust checks passed unless the Windows workflow actually reports success.
- No blanket rejection of ordinary ZIP data descriptors merely because bit 3 is set; legal signed and unsigned descriptor layouts must remain readable when their fields match.
- No Unicode filename policy based only on ASCII lowercasing or locale-sensitive lowercase conversion.
- No silent removal of U+FEFF from a ZIP filename during decoding.
- No ZIP64 classification based only on a locator-shaped signature without checking structural placement after the complete classic central directory.

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
- Duplicate HWPX package-part keys use canonical caseless comparison: NFD normalization, full Unicode case folding, then NFD normalization again. Keep original entry names and bytes unchanged; normalized strings are comparison keys only. This catches both case-fold expansions (for example `Straße`/`STRASSE`) and canonically equivalent spellings (for example composed/decomposed accents or Hangul).
- Decode ZIP filenames with fatal UTF-8 while preserving a leading U+FEFF as literal filename content (`TextDecoder` `ignoreBOM: true` in TypeScript). This keeps TypeScript behavior aligned with Rust UTF-8 decoding and prevents a BOM-prefixed name from aliasing an exact OWPML part name.
- For classic ZIP detection, first validate the classic EOCD's central-directory offset and size. Only interpret the 20 bytes before EOCD as a ZIP64 locator when they begin at or after the complete central-directory end; the referenced ZIP64 EOCD must also begin outside that directory and its declared record must end exactly at the locator. Central-directory comments and local payloads are opaque data, even when they contain ZIP64-shaped bytes.
- Validate ZIP local records against the raw central-directory metadata, including filename bytes, flags, method, CRC/sizes (allowing descriptor placeholders only when bit 3 is set), data bounds, and the complete descriptor extent. Sort complete local-record ranges and reject overlap so an entry cannot be nested inside another entry's payload.
- For an ordinary entry with bit 3 set, resolve the descriptor by preferring the signed 16-byte form when the signature and all three values match. Fall back to the unsigned 12-byte form only when the signed interpretation does not match. This preserves valid archives in the rare case where both byte interpretations appear to match while keeping the choice deterministic. The first `mimetype` entry remains stored and fully described in its local header.
- Dependency changes are limited to required test tooling, security-patched Vite and narrowly justified parser/archive dependencies. Regenerate locks in CI and verify with locked installs afterwards.

## Constraints

- Work branch: `codex/beta-hardening-2026-11-04`, based on `e46f7230074cab63362f41d46cc1d1153085f745`.
- Final integration target: `Beta` only.
- Preserve the current Tauri command argument and return shape consumed by the frontend.
- Test-first for behavior changes; no completion claim without current CI evidence.
- The design does not imply that the current local patch has been published or that the latest Rust-only tests have passed CI; execution status lives in the plan and evidence ledger.

--- Identity notice ---
This call is filed as Unattributed because its exact ChatGPT conversation is not known yet. Unattributed does not switch on Read-only mode or disable tools. Continue using every enabled tool, including apply_patch, exec_command, Desktop and Plugins; their existing permissions still apply. This request id owns its workspace, update_plan, terminals and agents/subagents. Use the process and run ids returned to this request; later exact proof attaches that state to its chat. A refusal about a particular process, worker or chat target applies only to that operation, not to file edits or other tools. Report the actual tool result; do not claim mutation or terminal tools are unavailable because of this notice, and never replay successful work.