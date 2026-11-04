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


### 2026-11-04 — LLM proxy resource and redirect hardening
- Added HTTP/HTTPS endpoint validation and rejected URL userinfo. Loopback and private custom endpoints remain available for local Ollama and user-selected gateways.
- Clamped request timeouts to 1 second–10 minutes (default 180 seconds).
- Bounded serialized request bodies to 20 MiB and streamed response chunks with a 32 MiB cap; an oversized Content-Length is rejected before body consumption.
- Disabled automatic HTTP redirects so provider authorization headers cannot be carried to a redirect destination.
- Kept cancellation active across both request send and response-body reading.
- Added unit/integration coverage for URL scheme/userinfo, request and response limits, timeout normalization, and cross-host redirect suppression.
- Fix commit: `8ada00e6caccfa90b75e62e6af07c5bc0c7beebe`; additional redirect/large-response tests commit: `97039a9ab947c3041580d360e64b2dd11722fe67`.
- Current Windows Rust CI validation is pending; these resource hardening behaviors are not marked verified yet.


### 2026-11-04 — Attachment ZIP-bomb and file-size hardening
- Added a preflight helper using fflate's ZIP filter callback. It inspects central-directory expanded sizes without inflating entries, enforces per-entry/total expansion limits, and supports a path allow-list.
- DOCX parsing now preflights only `word/document.xml` (max 16 MiB) and extracts only that entry; it no longer inflates unrelated embedded media.
- XLSX parsing preflights all ZIP entries (max 32 MiB per entry / 128 MiB total) before SheetJS parses the workbook.
- Added regression tests for per-entry expansion overflow, total expansion overflow, and ignoring unneeded large DOCX media entries.
- Added a 128 MiB raw attachment size limit at three boundaries: browser picker/drop before `arrayBuffer()`, parser entry point, and native Tauri metadata check before `fs::read`. UI now surfaces the size-limit error instead of leaving an unhandled promise rejection.
- Remaining caveat: XLS legacy OLE containers do not use ZIP, so the ZIP expansion preflight applies to DOCX/XLSX only. The raw 128 MiB input limit applies to every attachment type.

## 8. Extended correctness loop — 2026-11-04

This continuation was performed directly through GitHub and GitHub Actions. Chat On Steroids was not used.

### Additional defects found and corrected

#### F7. Native HWPX routing used an incorrectly escaped ZIP signature
- The open_document_session gate used a byte string containing literal backslash sequences rather than the four ZIP local-header bytes.
- A RED test verified that the real byte sequence 50 4B 03 04 was not recognized.
- Fixed by comparing against [0x50, 0x4B, 0x03, 0x04].
- The HWPX package validator itself was already tested; this defect was in the native document-open routing gate.

#### F8. Native LLM cancellation cleanup could remove a replacement request
- The cleanup guard originally removed the cancellation-map entry by request ID only.
- Race: request A is cancelled and removed; request B reuses the same ID; A's delayed Drop cleanup then removes B's entry.
- A deterministic Tokio integration test reproduced the race.
- Fixed with monotonically increasing registration tokens (AtomicU64). Cleanup removes an entry only if its token still matches the registration it owns.
- Duplicate active request IDs remain rejected.

#### F9. Vite font middleware fell through after rejecting an unsafe path
- The path resolver rejected traversal/symlink/sibling paths, but the middleware called next() when resolution failed.
- That allowed the request to continue into Vite's general static middleware rather than terminating at the font route.
- Extracted a testable middleware helper; rejected paths now return HTTP 404 and do not call next().

#### F10. Vite LLM request registry had the same stale-cleanup race
- The development bridge used a raw Map<string, AbortController>, overwrote duplicate IDs, and unconditionally deleted IDs in finally.
- Added ActiveHttpRequests: duplicate registration is rejected, cancellation removes and aborts the current controller, and cleanup is identity-checked.
- Added tests for duplicate IDs and ID reuse after cancellation.

#### F11. Vite development LLM proxy was weaker than the native proxy
- It followed redirects, accepted non-HTTP schemes/userinfo, did not bound request/response bodies, cleared timeout before reading the response body, and did not clamp timeout values.
- Added shared helpers and tests:
  - HTTP/HTTPS-only endpoint validation and rejection of URL userinfo
  - UTF-8 byte-size bound for serialized request bodies
  - timeout clamp to 1 second–10 minutes
  - streaming response-size enforcement, including Content-Length preflight
  - UTF-8 BOM removal
  - manual redirect handling
  - timeout remains active until response body reading finishes
- The development bridge now follows the same safety envelope as the native Rust bridge.

#### F12. TypeScript HWPX validator rejected legal ZIP data descriptors on ordinary entries
- The validator rejected general-purpose flag bit 3 for every ZIP entry, although ZIP permits data descriptors for ordinary entries.
- Added a generated HWPX fixture whose final document part uses a valid signed data descriptor; fflate successfully unzips it.
- The TypeScript validator now accepts descriptors on ordinary entries while retaining strict first-entry mimetype validation.
- Encryption flags remain rejected. A Rust test verifies that an encrypted OWPML document part is rejected by the native validator.

### TDD and verification evidence

- RED: workflow run 41740640233 reproduced the native ZIP-signature routing failure (22 passed, the new regression test failed).
- RED: workflow run 41741637004 reproduced stale native LLM cancellation cleanup (23 passed, the new race test failed).
- RED: workflow run 41742242016 reproduced Vite active-request registry behavior (existing tests passed; new registry assertions failed).
- RED: workflow run 41742434249 exposed the Vite LLM HTTP helper gaps; a separate test-only typo was corrected before the green run.
- RED: workflow run 41742805991 reproduced rejection of a valid ordinary-entry ZIP data descriptor.
- GREEN: workflow run 41743013233 passed 25 Windows Rust tests and 209 frontend tests, plus the frontend build.
- GREEN: workflow run 41742673645 passed the 208-test frontend suite/build and 24 Windows Rust tests.
- GREEN: workflow run 41742350152 passed the 202-test frontend suite/build and 24 Windows Rust tests.
- GREEN: workflow run 41743372019 completed npm ci, npm audit --audit-level=high, the 209-test frontend suite and production build successfully. Its Windows cargo test --locked job was still running at the time of this log update.
- Workflow run 41743558019 is the current validation after upgrading GitHub Actions runtimes; treat its final result as authoritative for the latest workflow file.

Test-authoring corrections recorded for transparency:
- One intermediate document.rs patch accidentally displaced the existing HWPX validator import; it was restored before the behavior test was rerun.
- The Tokio race test needed std::future::Future in scope.
- The LLM bridge test initially referenced the wrong timeout constant; corrected before the passing 208-test run.
- These were test/patch construction errors, not additional production defects.

### Lockfiles and CI hardening

- Committed the generated root package-lock.json and src-tauri/Cargo.lock to this workstream.
- Added a narrowly scoped same-repository PR job that regenerates and commits lockfiles only on codex/beta-hardening-2026-11-04; it does not run for forks or push events.
- Frontend CI now uses npm ci, runs npm audit --audit-level=high, executes Vitest and builds the production frontend.
- Windows CI now uses cargo test --locked.
- Updated GitHub Actions to actions/checkout@v7.0.1 and actions/setup-node@v7.1.0 to avoid the Node 20 deprecation warning. Official release pages were checked before the update.
- The lockfile synchronization job confirmed the committed lockfiles are already up to date.

### Upstream comparison

- HOP v0.4.5 was used as the reference for the native font catalog; only the relevant catalog/command behavior was adapted to Hwpcolab's Tauri command architecture.
- RHWP v0.8.7 was already the current stable version in the vendored engine, so no blind RHWP replacement was made.
- No wholesale HOP rebase or RHWP source rewrite was justified by the reviewed differences.

### Current state and remaining gates

- Integration base remains origin/Beta at e46f7230074cab63362f41d46cc1d1153085f745; comparison showed no Beta commits missing from the workstream at the last check.
- All work remains on codex/beta-hardening-2026-11-04 and PR #1. main has not been modified.
- Lockfiles are committed and the npm audit gate passed.
- Remaining before integration:
  1. Obtain the final result of current-head Windows cargo test --locked and the latest Actions run.
  2. Re-read the complete PR diff, especially the Tauri command registration, font allow-list, native/Vite save parity, HWPX parser parity, and LLM request lifecycle.
  3. Record any remaining platform-only limitations. A packaged Tauri WebView smoke test is not equivalent to the Windows Rust unit suite and remains a separate runtime check.
  4. Integrate into Beta only after the current-head checks are green. Do not merge to main.
