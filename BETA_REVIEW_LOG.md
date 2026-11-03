# Beta Branch Review Log

> Living engineering ledger for the Hwpcolab Beta hardening effort. The integration target is `Beta` only. Never modify `main` or another release branch.

## 1. Verified baseline and boundaries

- Repository: `inter-schema/Hwpcolab`
- Integration target: `Beta`
- Fresh baseline: `e46f7230074cab63362f41d46cc1d1153085f745` (`docs(beta): add ongoing review log`, 2026-11-03)
- Implementation branch: `codex/beta-hardening-2026-11-04`, created directly from the verified Beta baseline.
- The linked GitHub installation has push permission for this repository.
- All implementation commits remain on the isolated branch until tests, CI and review are satisfactory. The final integration destination is Beta only.
- User explicitly authorized broad code and work-file additions on Beta and separately instructed that COS must not be used. Work is therefore performed through GitHub APIs and GitHub Actions only; no COS tools are used.

## 2. Goals

1. Restore native Tauri local-font enumeration and safely scoped font-byte reads.
2. Reject arbitrary ZIP files as HWPX and protect document saves from interrupted writes.
3. Replace regex-based DOCX XML extraction with namespace-aware parsing that preserves WordprocessingML structure.
4. Close Vite font asset path traversal, sibling-prefix and symlink escape cases.
5. Make frontend and Rust tests repeatable in CI, then reconcile generated lockfiles.
6. Compare current HOP/RHWP upstream changes and port only independently justified fixes.

Non-goals: no main-branch edits, no blanket dependency upgrades, no wholesale upstream rebase, no indiscriminate system-font bundling, and no claims of platform validation without an executed CI result.

## 3. Findings revalidated at e46f723

### F1. Desktop native fonts are disabled and commands are missing
- `src/hop-host/core/desktop-local-fonts.ts`: `isDesktopTauriRuntime()` returns false before its real Tauri marker/protocol check.
- Frontend calls `list_local_fonts` and `read_local_font`.
- `src-tauri/src/lib.rs`, `src-tauri/src/commands/mod.rs`, and `src-tauri/Cargo.toml` have no corresponding native catalog module or command registrations.
- HOP v0.4.5 contains a fontdb-based catalog reference. Adapt it carefully, including canonical path allow-listing; upstream path checks are not accepted without local review.

### F2. Document save overwrites the target directly
- `src-tauri/src/commands/document.rs`: `commit_staging_to_original` uses `fs::copy` to the final target.
- `vite.config.ts`: development bridge uses `copyFileSync` to the final target.
- Both paths need same-directory staging, complete write/sync, atomic replacement semantics, cleanup and preservation tests.

### F3. ZIP magic is treated as HWPX
- Native and Vite development paths classify any `PK` prefix as HWPX.
- Require a valid OWPML package signature and essential parts; reject generic ZIP archives and malformed packages.

### F4. DOCX extraction is regex-based
- `src/core/multiFileParser.ts` extracts `w:p` and `w:t` by fixed prefix and loses tabs, breaks, tables and whitespace semantics.
- Use an XML parser/DOM with namespace URI semantics and fixture-driven tests.

### F5. Vite font route uses unsafe string-prefix containment
- `vite.config.ts` uses `candidate.startsWith(fontsDir)`, which does not enforce path-component boundaries or prevent symlink escapes.
- Handle malformed URL encoding without an unhandled middleware exception; canonicalize both root and candidate.

### F6. The existing test suite cannot be run from the remote manifest
- Vitest test files exist under `src/hop-host/core`, but remote `package.json` has no Vitest dependency or `test` script.
- `package-lock.json` already resolves Vite 6.4.5 while the manifest still declares `^6.0.3`; align the manifest to the patched Vite 6.4.5 floor and regenerate the lock through CI.

## 4. Implementation order

1. Add Vitest/happy-dom test execution and CI; prove the native runtime regression test fails for the expected reason.
2. Restore native font runtime detection and implement Tauri font catalog/commands with canonical path checks.
3. Add HWPX structural validation and durable save replacement in both native and Vite development paths.
4. Replace DOCX regex extraction and add real ZIP/XML fixtures.
5. Harden Vite font asset middleware with canonical path containment.
6. Compare upstream and dependencies, run the complete validation matrix, review and merge the clean PR into Beta.

## 5. Verification protocol

- Every behavior change starts with a test written before production code.
- A RED test must fail for the intended reason, not a syntax/import error.
- A GREEN test must pass, followed by the full relevant suite and build.
- Rust behavior is verified on Windows CI; frontend tests/build are verified on Ubuntu CI.
- Generated `package-lock.json` and `src-tauri/Cargo.lock` are captured from the first CI setup run, committed, and then subsequent CI uses locked installs/builds.
- Record exact branch/commit IDs, GitHub Actions runs, job conclusions and remaining risks below.
- Do not claim completion until the final PR head has green checks and its diff has been inspected.

## 6. Work history

### 2026-11-04 — Baseline revalidation
- Confirmed Beta HEAD: `e46f7230074cab63362f41d46cc1d1153085f745`.
- Created `codex/beta-hardening-2026-11-04` from Beta.
- Confirmed repository push permission.
- Re-read current font runtime, Tauri registration, save, DOCX and Vite font middleware source.
- No production code has been changed at this checkpoint.


### 2026-11-04 — CI RED: test discovery boundary
- PR #1 triggered workflow run `41715250196`.
- Frontend dependency installation succeeded.
- `npm test` failed before the new runtime regression test could be evaluated: Vitest's default discovery traversed `third_party/rhwp/rhwp-studio` and collected upstream `.test.mjs`/E2E suites that depend on packages and workspace files not installed by the Hwpcolab root manifest (including `pixelmatch`, `pngjs`, `vite-plugin-pwa`, and upstream-local TypeScript/scripts/assets).
- This is a test-harness scope defect, not evidence that the runtime regression test is red. Add an explicit first-party test include boundary (`src/**/*.test.ts`) before accepting the runtime RED result.
- npm lock artifact uploaded by this run: artifact ID `13270301874` (`beta-npm-lock`).
- Windows Rust job was still running at the time of this log update.


### 2026-11-04 — CI RED confirmed
- First PR run exposed test discovery collecting upstream E2E suites. The Vitest config was narrowed to `src/**/*.test.ts`.
- Second run: frontend `npm install` succeeded; Vitest executed 23 first-party test files: **147 passed, 9 failed**.
- The two new `isDesktopTauriRuntime` cases (Tauri v2 marker and `tauri:` protocol) failed with `expected false to be true`, directly reproducing the unconditional early return.
- Six existing local-font behavior tests failed downstream for the same runtime-dispatch cause.
- One unrelated existing test suite failed at import time because `src/hop-host/command/replace-upstream-commands.test.ts` referenced `../../index.html` (resolving under `src/`) instead of the repository-root `index.html`. Corrected the test import path by one directory level.
- These findings are now isolated; the production runtime fix removes only the unconditional early return.


### 2026-11-04 — Native font catalog validation
- The first font catalog CI run (41716034285) completed frontend tests and build successfully.
- Windows Rust test job also completed successfully with the native font catalog, command registration, `fontdb 0.23.0`, and canonical allow-list unit tests.
- The later HWPX test-only run (41716475651) confirmed the intended TypeScript RED: 23 test files passed, 163 tests passed, and the new HWPX suite could not import the not-yet-implemented package validator.
- This is an expected RED checkpoint; no HWPX production path has been changed yet.


### 2026-11-04 — Atomic save RED tests
- Added Node tests for complete replacement, missing staging source, replacement failure, and temporary-file cleanup.
- Added matching Rust tests for successful overwrite, missing source preservation, and replacement failure preserving an existing directory.
- These tests intentionally reference the not-yet-implemented atomic copy helper and will be evaluated after the implementation is committed; do not treat this checkpoint as green.


### 2026-11-04 — DOCX parsing, atomic save and font asset security
- Replaced regex-only DOCX extraction with WordprocessingML namespace-aware DOM parsing. Preserves content in `w:t`, explicit tabs, line breaks, non-breaking/soft hyphens, table rows/cells and preserved spaces; ignores raw XML indentation between elements.
- Added DOCX fixtures for alternate namespace prefixes, entities, pretty-printed indentation, xml:space, tabs, breaks, Markdown tables and malformed XML.
- Added native and Node atomic-save helpers. Both write a unique temporary sibling, complete the copy, flush/fsync file data, then atomically replace the destination. Native helper uses `tempfile::NamedTempFile::persist`; Vite bridge uses a Node temp sibling and rename. Directory sync is best-effort where the platform supports it.
- Atomic-save tests cover successful replacement, missing staging input preserving the old file, and replacement failure with temporary-file cleanup.
- Added a canonical WOFF2 path resolver for Vite's dev middleware. It rejects malformed percent encoding, traversal, sibling-prefix paths, symlink escapes, missing/non-file targets and non-WOFF2 extensions. Middleware uses the canonical resolver instead of string-prefix containment.
- Latest code before current verification: PR branch head `deb94d5a3e28a4aa720a665b6c27b24409b6a016` (font path security changes).
- Earlier current-branch run `41717504483` was for a pre-implementation commit and failed only because `atomic-file.ts` was not yet present at that checkpoint: 169 existing tests passed. This is not evidence about the current head.
- A fresh current-head run is being queued after the latest PR update. Do not claim DOCX, HWPX or atomic-save test success until that run completes.


### 2026-11-04 — SheetJS security audit
- The manifest previously resolved registry `xlsx@0.18.5`, which is affected by SheetJS ReDoS CVE-2024-22363 (all CE releases through 0.20.1) and is distributed from a stale npm registry endpoint.
- SheetJS's own advisory identifies 0.20.2+ as the ReDoS fix, and its official installation documentation recommends the CDN NodeJS tarball. The official CDN currently lists 0.20.3 as the recommended release.
- Updated `package.json` to the official `https://cdn.sheetjs.com/xlsx-0.20.3/xlsx-0.20.3.tgz` tarball and added a workbook-import compatibility fixture that asserts both Markdown and structured sheet data.
- Lockfile regeneration and full CI validation remain pending; do not consider this dependency upgrade verified until the current-head install, tests and build pass.


### 2026-11-04 — LLM proxy cancellation correctness
- Traced the frontend call sites: the OpenAI-compatible provider passes `request_id`, while `claudeProvider.ts` invokes the same Tauri command without one. The previous Rust implementation created a oneshot receiver unconditionally but only stored its sender when an ID existed; the sender was immediately dropped for Claude/no-ID calls, causing the select to resolve as cancelled before sending.
- Found a second race: inserting duplicate request IDs into the HashMap replaced the original sender. Cleanup from either request could also remove the other request's cancellation entry.
- Added localhost HTTP integration tests for both cases before changing the implementation:
  - A no-ID request must reach a local server and receive HTTP 200.
  - A duplicate in-flight ID must be rejected with an explicit conflict message; the first request must remain cancellable.
- Fixed registration to create the oneshot channel only when an ID exists, use HashMap::Entry to reject duplicate active IDs, surface lock poisoning as an error, and avoid stale cleanup removing another active request.
- Tokio `net` feature was added for the local TCP fixture.
- RED execution was requested in Actions run `41719841955`; GitHub reported both jobs queued for an extended period, so the exact failing output was not captured before implementing the fix. This is recorded as a TDD verification gap, not as a confirmed RED observation.
- Fix commit: `171893a89987b9ef41d116403e36ac3ee28a3d03`. A fresh Windows Rust CI run is required to validate both integration tests and compilation.


### 2026-11-04 — Tauri WebView CSP hardening
- Found `src-tauri/tauri.conf.json` explicitly set `app.security.csp: null`, which disables Tauri's Content Security Policy protection.
- Added separate production and development CSPs. Production allows local/custom-protocol assets, Tauri IPC, app-bundled workers/fonts/images and WASM execution, but no remote network origins or general eval. Development adds only the Vite localhost origin and HMR websocket.
- Added configuration contract tests for non-null CSP, IPC, the required Tauri custom protocol, WASM support, no general `unsafe-eval`, object-src none, and dev-only localhost/HMR.
- Tauri v2 documentation notes that CSP is only enabled when configured and that its injected asset CSP modifications append hashes/nonces to bundled code. Runtime smoke testing in packaged WebView remains desirable after CI.
