# Beta Branch Review Log

> Living engineering ledger for the Hwpcolab Beta hardening effort. The integration target is `Beta` only. Never modify `main` or another release branch.

## 1. Verified baseline and boundaries

- Repository: `inter-schema/Hwpcolab`
- Integration target: `Beta`
- Fresh baseline: `e46f7230074cab63362f41d46cc1d1153085f745` (`docs(beta): add ongoing review log`, 2026-11-03)
- Implementation branch: `codex/beta-hardening-2026-11-04`, created directly from the verified Beta baseline.
- The linked GitHub installation has push permission for this repository.
- All implementation commits remain on the isolated branch until tests, CI and review are satisfactory. The final integration destination is Beta only.
- User explicitly authorized broad code and work-file additions on the isolated PR branch. The initial implementation phase was GitHub-only at the user's direction; on 2026-11-04 the user explicitly reauthorized COS for local QA. The integration target remains Beta only.

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
- Added localhost HTTP integration tests for both cases:
  - A no-ID request must reach a local server and receive HTTP 200.
  - A duplicate in-flight ID must be rejected with an explicit conflict message; the first request must remain cancellable.
- Fix registration to create the oneshot channel only when an ID exists, use HashMap::Entry to reject duplicate active IDs, surface lock poisoning as an error, and avoid stale cleanup removing another active request.
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

## 9. COS reconnection handoff — 2026-11-04

- Created `COS_RECONNECT_TEST_PLAN.md` in the repository root on the isolated PR branch.
- The plan contains CON-01 through CON-08 connection/API safety checks, a baseline refresh requirement, a concrete P0 HWPX duplicate-fixture task, subsystem regression matrices for fonts, HWPX, saves, DOCX/XLSX, Vite path containment and LLM proxy parity, plus a packaged Windows WebView smoke test.
- Proposed execution backlog is numbered COS-BOOT-01 through COS-REVIEW-01 with acceptance evidence and a definition-of-done checklist.
- COS has not yet been connected or used for this test-preparation step. The supplied API contract is intentionally not assumed; the next COS session must inspect it before making requests or configuring credentials.
- Plan commit: `639f1543330d570224070c57b2ca6eb291d5283b`.
- PR #1 is still open against `Beta`; the work branch remains separate. `main` remains untouched.

## 10. COS follow-up — native ZIP validation hardening (2026-11-04)

### Reconnected baseline

- COS workspace became reachable at `/chatonster`.
- Created a fresh local worktree `/chatonster/_work/Hwpcolab-Beta-resume` from the then-current remote PR head `7be157ae69e853a3abe6c9f47fe63cdd2ab80eb3`.
- Local branch: `codex/beta-hardening-resume-2026-11-04`.
- PR #1 remains open, base `Beta` at `e46f7230074cab63362f41d46cc1d1153085f745`; remote PR head and Beta were unchanged during this session.
- The previous COS worktree and the original Beta checkout were left untouched.

### Confirmed current-head CI failure

- GitHub Actions run `41755601913` on commit `7be157a` completed with frontend tests/build and lockfile sync passing, but Windows Rust tests failed.
- Rust result: 26 passed, 1 failed. Failing test: `hwpx_package::tests::rejects_duplicate_package_part_names`.
- The job log shows the fixture contains two central-directory records named `Contents/section0.xml`; the native validator nevertheless accepts it. This confirms the duplicate-record issue is in the native validator/ZIP index handling, not merely a fixture that failed to create a duplicate.

### Local changes

- Test-only commit `b7997ed5d2e595ce2845c14568d0a214a5f19b43` was created locally before implementation changes. It is not pushed.
- Follow-up local implementation commits: `825cbbb9791bf4b8f7e2dfe1280c9ff6995e20bc` (raw native ZIP directory preflight and parity checks) and `ec78f88a8060322898f5d4da9eddfc63bf78d436` (structural ZIP64 locator validation and false-positive coverage). Both remain local and are not pushed.
- Implementation checkpoint: `ec78f88a8060322898f5d4da9eddfc63bf78d436`; a subsequent ledger-only commit records the updated 218-test result. All commits remain local on `codex/beta-hardening-resume-2026-11-04`; no push was performed.
- Native HWPX validation now has a raw classic-ZIP central-directory preflight before `ZipArchive` indexing. It rejects malformed central-directory layouts, multi-disk archives, structurally valid ZIP64 locators/sentinels, duplicate names, unsupported encryption flags, non-UTF-8 or incorrectly flagged non-ASCII names, and mismatched first-entry `mimetype` local/central metadata.
- TypeScript HWPX validation now rejects a ZIP64 locator even when classic EOCD count/size/offset fields still fit, and applies the same UTF-8 filename-flag policy as the native path.
- Regression coverage was added for case-insensitive and Unicode case-insensitive duplicate names, a genuine mimetype data descriptor, local/central mimetype name mismatch, ZIP64 end records with otherwise representable classic EOCD values, a locator-like byte sequence in a valid central-directory comment, and a valid data descriptor on an ordinary package part.
- The TypeScript mimetype descriptor fixture is checked with `unzipSync` before asserting rejection, so it is a readable ZIP with a real descriptor rather than only a modified flag bit.

### Local verification

- Focused TypeScript HWPX suite: **15 tests passed**.
- Full Vitest suite: **32 files, 218 tests passed**.
- `npm run build`: exit code 0. Existing warnings remain for `/theme-init.js`, Node core modules externalized from `rhwpEngine.ts`, mixed static/dynamic Tauri API imports, and chunks over 500 KB.
- `git diff --check`: exit code 0; Git reports only the repository's existing LF-to-CRLF working-copy normalization warning.
- TDD RED evidence: temporarily removing the TypeScript ZIP64-locator guard made `rejects ZIP64 archives even when the classic EOCD fields still fit` fail because the validator returned `true`; restoring the guard made the focused test pass.
- The ZIP64 locator detector validates the locator's disk fields, referenced ZIP64 EOCD signature and record bounds. A regression test confirms a locator-like byte pattern inside an otherwise valid central-directory comment is not mistaken for a ZIP64 locator.
- Rust execution was **not run** in this Windows COS environment: `cargo`/`rustc` are not installed or on PATH, and WSL is not installed. The new Rust implementation and new Rust regression tests therefore still require Windows CI validation.

### Z.ai and subagent execution

- The API key file was read from the COS workspace without printing its value. The official Z.ai `/models` request returned HTTP 200; the listed catalog did not include the two requested Flash IDs.
- A direct `glm-4.7-flash` chat-completion request was accepted and returned that model ID, but the small token budget produced no visible answer text. A later minimal request returned HTTP 429 (service temporarily overloaded), and a full code-review request returned HTTP 400 without actionable response details. No independent Z.ai review result is claimed.
- Two COS worker dispatches using `glm-4.6v-flash` failed before starting because the ChatGPT worker launcher could not start a ChatGPT chat for that model ID. The active worker status confirms both workers failed; no worker review or test report was returned. The current worker launcher does not establish that it can route agents to the Z.ai API.
- No API key value was printed, logged, or committed.

### Remaining gates

1. Run Rust formatting, Clippy and the full Rust suite on Windows CI against the new code; verify the genuine mimetype descriptor fixture and ZIP64 fixture compile and behave as expected.
2. Obtain a working Z.ai/Agent Desktop subagent route or use a compatible reviewer; the requested independent subagent review has not yet completed.
3. Re-run the latest PR checks and review the complete diff after the new branch head is published. No push, PR update, or merge was performed in this session.
4. Packaged Windows Tauri WebView smoke testing remains outstanding.

## 11. COS continuation — empty ZIP member parity (2026-11-04)

### Verified boundary and workspace

- Active checkout: `C:\Users\jun\Documents\chatonster\_work\Hwpcolab-Beta-resume`.
- Local branch: `codex/beta-hardening-resume-2026-11-04`; pre-change HEAD `b980382b5fc09fb3b20ea2f49b4ae87242241a3e`.
- PR #1 base: `Beta` at `e46f7230074cab63362f41d46cc1d1153085f745`.
- Remote PR branch head before publication: `7be157ae69e853a3abe6c9f47fe63cdd2ab80eb3`; local resume branch was four commits ahead and had no remote-only commits.
- Remote `main` was read-only checked at `0c23baaf63a49e75fd176a8b18d182acfa91546e`. No writes, checkout, commits or pushes were made to `main` or `Beta`.
- The separate `Hwpcolab-Beta` and `Hwpcolab-Beta-hardening` working directories were not modified during this continuation.

### Defect and RED/GREEN evidence

- A generated ZIP containing all required HWPX members plus one empty-name member was accepted by `isValidHwpxPackage()`.
- The first test-only run failed as expected: `expected true to be false` at `hwpx-package.test.ts` for the empty filename fixture.
- TypeScript fix: reject `nameLength === 0` while parsing central-directory records.
- Native Rust already rejected zero-length central-directory names. Added a direct unit regression for `inspect_classic_zip_directory()` to preserve that invariant.

### Local verification

- Focused TypeScript HWPX suite: **16 passed**.
- Full Vitest suite: **32 files, 219 tests passed**.
- TypeScript compile: passed.
- Vite production build: passed. Existing warnings remain for Node core modules externalized from `rhwpEngine.ts`, mixed static/dynamic Tauri API imports, and chunks over 500 KB.
- `npm audit --audit-level=high`: **0 vulnerabilities**.
- `git diff --check`: passed; only LF-to-CRLF working-copy warnings were reported.
- Rust formatting, Clippy and Rust tests were not run locally because Cargo, rustc and rustfmt are unavailable in this Windows COS environment. The new Rust regression is pending Windows CI.

### Z.ai and independent review status

- The user-provided secret file was read without printing its value or copying it into the repository. The official Z.ai `/models` endpoint returned HTTP 200.
- A direct `glm-4.7-flash` chat-completion request returned HTTP 200 but no visible text when constrained to 8 output tokens. A follow-up request with thinking disabled returned HTTP 429, business code `1305` (service temporarily overloaded). No Z.ai code-review answer is claimed.
- Two read-only COS worker dispatches using the account-listed `glm-4.7` model failed before starting: the worker launcher could not confirm that model in the ChatGPT model picker. No replacement worker was launched and no subagent findings are claimed.
- The API `/models` response listed 12 model IDs and did not include either requested Flash ID; nevertheless the direct `glm-4.7-flash` completion route accepted the model name before the 429 response.
- No credential value was printed, written to project files, committed or included in a prompt.

### Publication and remaining gates

- The initial five-file snapshot was published to PR branch `codex/beta-hardening-2026-11-04` as commit `b64630e`, parented to `7be157a`, after all five remote blob SHAs matched the local snapshot.
- The workflow's lockfile-sync job subsequently created `23b269b` as a child of `b64630e`. Its compare result changed only `src-tauri/src/hwpx_package.rs` (+75/-48); that remote version is the base for the next update and must be preserved.
- The current CRC parity fix, Rust test and two Rust compile corrections are local working-tree changes on the resume branch. They have not yet been published; the next update must be parented to the current PR head `23b269b` with an expected-head guard.
- After publication: inspect fresh PR checks and the complete diff; run Rust formatting/Clippy/tests through Windows CI; keep the independent reviewer and packaged Windows WebView smoke test as explicit gates.
- `Beta` and `main` remain unchanged. Do not merge the PR without a separate integration decision.

### Follow-up: mimetype CRC parity and first Windows CI result

- Added a generated HWPX regression fixture that preserves the `application/hwp+zip` bytes but changes the CRC in both the local and central headers. Before the fix, TypeScript accepted it (`expected false, received true`).
- TypeScript now computes CRC-32 over the actual MIME payload and compares it with the central-directory CRC.
- Added the corresponding native test. It changes both stored CRC fields and expects `ZipArchive`'s full `read_to_string()` of the MIME entry to reject the checksum mismatch.
- Full Vitest after this fix: **32 files, 220 tests passed**. TypeScript compilation passed. Vite build was rerun and its final result is recorded in the continuation output.
- The first Windows CI run for commit `b64630e` passed lockfile sync, frontend tests/build and Rust formatting, but Clippy failed compilation at `src-tauri/src/hwpx_package.rs:46` and `:154`: each expression applied `?` to `bytes.get(...)` and then compared the unwrapped slice to `Some(...)`. Removed the two `?` operators; a new Windows CI run is required to verify compilation, Clippy and Rust tests.
- The first Windows CI run for `b64630e` failed in Clippy compilation. A later workflow run on the bot-created `23b269b` commit finished as `action_required`; no Rust tests ran for that run. Current PR head is `23b269b1fddece62d179def136eb82adddf464af`.
- The CRC and compile fixes are local-only until the next parent-checked PR update. The next publication must preserve the `23b269b` Rust-source diff and use that exact SHA as parent/expected head.
- The CRC/compile follow-up was published as `ce7cd14`, parented to `23b269b`. Its five changed blobs were verified against the intended local snapshot.
- The subsequent `Sync dependency lockfiles` job formatted `src-tauri/src/hwpx_package.rs` and created bot commit `24e6029` (+11/-8 in that file only). The formatted file still contains the CRC helper/test and no longer contains either invalid `bytes.get(..)? != Some(..)` expression.
- Workflow `41776178382` on bot head `24e6029` ended `action_required` and did not execute tests. Workflow `41776142633` on code head `ce7cd14` completed lockfile sync, frontend tests/build and Rust formatting successfully; Windows Clippy was still running at this log update, with Rust tests pending.
- Current PR head: `24e6029115679f85840a2b86b5fea967231ac7c6`. The next user-authored follow-up is a documentation-only status update parented to this exact head, intended to trigger a fresh CI run after automatic formatting has stabilized.
- `main` and `Beta` were not modified. No merge was performed.

## 12. Continued QA loop — local-header parity, Unicode case folding and descriptor integrity (2026-11-04)

### API / reviewer route

- Primary `z.aifree.env` was tested first using `glm-4.7-flash`; the completion returned HTTP 429 / service-overload code 1305.
- Only after that failure, the added `backup_baze_api.env` was tested. BAZE `/models` and a `gpt-6-luna` chat completion both returned HTTP 200 with a non-empty answer. No key value was printed, logged or committed.
- A COS worker's first read-only pass identified the full-Unicode-case-fold gap and ordinary-entry local-header mismatch. Its attempted second pass was blocked because the new local changes were not yet on the PR SHA; request a fresh pass after publication.
- The second COS worker did not start within the worker launcher timeout. No review result is claimed from it.

### New defects and regression evidence

1. **Ordinary-part local-header mismatch.** A fixture changes only the local filename for `Contents/section0.xml` to `Contents/sectionX.xml`, leaving the central directory untouched. Before the fix, the TypeScript validator accepted it. Both validators now compare every local header to its central entry: raw filename bytes, general-purpose flags, compression method, CRC, compressed/uncompressed sizes, and local-data bounds.
2. **Full Unicode case-fold collisions.** Existing Unicode lowercase checks did not fold `Straße` and `STRASSE` to the same key. Added the `unicode-case-folding` 1.1.2 TypeScript package and `unicase` 2.8.1 Rust crate, with a regression in each implementation.
3. **Corrupt ordinary data descriptor.** A generated signed descriptor for the final ordinary part had only its CRC field modified. Before the fix, the TypeScript validator accepted it (RED confirmed: expected false, received true). Both implementations now verify descriptor CRC and sizes against the central directory, with and without the optional descriptor signature, while preserving valid ordinary data descriptors.

### Local verification

- Focused HWPX TypeScript suite after the latest changes: **20 passed**.
- Full Vitest: **32 files, 223 tests passed**.
- TypeScript compilation passed.
- Vite production build passed. Existing warnings remain for `/theme-init.js`, Node core modules externalized from `rhwpEngine.ts`, mixed Tauri imports, and large chunks.
- `git diff --check` passed. Rust cannot be run locally because Cargo/rustc/rustfmt are not installed in this COS Windows workspace; new native regressions require Windows CI.
- The latest completed integrated Actions checkpoint before this local patch was run `41776400497` on head `18e0c92`, with all three jobs successful, including Windows `cargo fmt`, Clippy, and `cargo test --locked`.

### Current publication boundary

- Latest verified remote PR head before this local change set: `18e0c9254a12f63f61a6c5cb4055b8de1ae9817a`.
- Current local patch changes nine files: `package.json`/`package-lock.json`, `src-tauri/Cargo.toml`/`Cargo.lock`, both validators, matching tests, and this report/plan. The next PR commit will include the seven source/manifest/report files; the repository's lockfile-sync workflow will generate and publish the two lockfiles.
- These changes have not yet been published. Next: create a commit parented to exactly `18e0c92` and update only `codex/beta-hardening-2026-11-04` with an expected-head guard. Then verify lockfile sync, npm audit, frontend tests/build and Windows Rust formatting/Clippy/tests. Request a fresh reviewer pass against the exact new SHA.
- Packaged Windows Tauri WebView smoke test remains outstanding. `Beta` and `main` were not modified; do not merge the PR on this report alone.

## 14. Persistent memory, divergent-branch reconciliation and expanded ZIP validation (2026-11-04)

### Persistent working memory

- Created `TEMP_MEMORY.md` at the repository root to preserve PR/base/main refs, API-provider precedence, local-vs-remote branch divergence, current working changes, verified test evidence, known gaps and next actions.
- Published the memory file to PR #1 as commit `e7d3a385e766a4c699a858b54ccf9a4e1264b87f`, parented to remote head `2407072f96b2a9415be638a96bce055a7d7b0792`.
- Actions run `41806776539` on the memory-only commit completed successfully in all three jobs (lockfile sync, Windows Rust tests, frontend tests/build). This does not validate the subsequent local-only source changes.

### Reconciliation of the diverged local checkout

- Local resume branch HEAD was `699af2903b70ce20c3d855c8306e2a97d92036ed`; remote PR head was `2407072f96b2a9415be638a96bce055a7d7b0792`. Both descended from `7be157a`, but local had 6 unique commits and remote had 8 unique commits. Directly pushing the local branch would have been unsafe.
- Created a disposable worktree at remote PR head `2407072` and applied the local working diff with Git's three-way apply.
- Resolved conflicts by retaining remote `Cargo.lock` version `unicase 2.10.0`, aligning `Cargo.toml` to 2.10.0, and integrating the local range-returning verifier and regression fixtures. The npm and Cargo lockfiles already matched the remote and were preserved.
- The resulting detached worktree is based on remote `2407072`; the merged source patch is staged locally and is not yet published. No local commit/force push was made.

### New local test evidence after reconciliation

- Focused HWPX suite: **24/24 passed**.
- Full Vitest: **32 files, 227/227 tests passed**.
- TypeScript compile: passed.
- Vite production build: passed in 24.54 seconds. Existing warnings remain: `/theme-init.js` is a non-module script, RHWP browser bundle externalizes Node core modules, Tauri imports are mixed static/dynamic, and the main JS chunk exceeds 500 kB.
- `npm ci --no-audit --no-fund`: passed after a single sequential install; `esbuild --version` returned 0.25.12.
- `npm audit --audit-level=high`: **0 vulnerabilities**.
- `git diff --check` and `git diff --cached --check`: passed after conflict resolution.
- The overlap fixture now proves an ordinary ZIP reader can extract both the embedded metadata entry and the 512-byte section payload, while the HWPX validator rejects the package because the two local-record ranges intersect.
- The Rust overlap test performs the same `ZipArchive` extraction assertions before requiring the native HWPX validator to reject the archive. This native assertion has not run locally; it must be confirmed by the next Windows CI.

### Exact current publication boundary

- Remote PR head remains `e7d3a385e766a4c699a858b54ccf9a4e1264b87f` (memory commit), base `Beta` at `e46f7230074cab63362f41d46cc1d1153085f745`.
- `Beta` was last checked at `e46f7230074cab63362f41d46cc1d1153085f745`; `main` at `0c23baaf63a49e75fd176a8b18d182acfa91546e`.
- Current merged source patch is prepared for one parent-checked commit on detached worktree `C:\Users\jun\Documents\chatonster\_work\Hwpcolab-Beta-current`; six tracked files are modified (`BETA_REVIEW_LOG.md`, plan, `src-tauri/Cargo.toml`, Rust HWPX validator, TypeScript HWPX tests and validator) and `TEMP_MEMORY.md` is newly added. The npm/Cargo lockfiles already contain the required dependencies.
- After publication, verify the returned commit SHA and require fresh Windows rustfmt, Clippy and `cargo test --locked`, plus frontend CI on that exact code head.
- Spawn fresh read-only Rust and TS review workers against the newly published exact head. Keep the packaged Windows Tauri WebView smoke test outstanding. Do not modify `Beta`/`main` or merge PR #1.

## 16. Canonical Unicode normalization and descriptor ambiguity compatibility (2026-11-04)

### Independent reviewer follow-up

- Rust review identified a cross-platform path collision risk not covered by case folding alone: canonically equivalent NFC/NFD names (for example `café` vs `cafe` + combining acute) may alias on some filesystems.
- Rust review also noted that rejecting a descriptor when both signed and unsigned interpretations match can reject a rare valid archive.
- The TypeScript reviewer questioned the LLM bridge call site in its first pass; direct inspection confirmed `vite.config.ts` imports and registers the bridge. Document-open also invokes the HWPX validator. No production-call-site defect remains.

### Local fixes and regressions

1. Duplicate-name keys now use canonical caseless matching: NFD → full Unicode case fold → NFD. TypeScript uses built-in `String.normalize('NFD')`; Rust uses `unicode-normalization` 0.1.26 plus `unicase` 2.10.0.
2. Added NFC/NFD collision regressions for Latin accent forms and precomposed Hangul syllable versus decomposed Jamo. Added full case-fold collision pairs for Greek final sigma, the ffi ligature, and Kelvin sign.
3. Descriptor parsing now prefers the signed 16-byte interpretation when the signature and all fields match; only if that interpretation fails does it try the unsigned 12-byte form. Added a helper-level test where both interpretations match and signed is selected.

### Local verification after this follow-up

- Focused HWPX TypeScript suite: **28/28 passed**.
- Full Vitest: **32 files, 231/231 tests passed**.
- TypeScript compilation: passed.
- Vite production build: passed in 16.98 seconds. Existing warnings remain for `/theme-init.js`, externalized Node core modules, mixed Tauri static/dynamic imports, and the large main chunk.
- `npm audit --audit-level=high`: **0 vulnerabilities**.
- The new Rust NFD and descriptor-priority tests have not yet run in CI.

### Latest completed Windows checkpoint and publication boundary

- Actions run `41809160934` for code head `6c55c4a46724fc0078567e9cb975c235a0a74622` completed successfully in all three jobs. Windows formatting, Clippy and `cargo test --locked` passed; frontend tests/build and lockfile sync passed.
- Lockfile sync created bot-format child `2b3154b913bd987a8ad44b8165c4633c2726dd38`. Its bot-originated run `41809220727` was `action_required`, so that exact bot child has not had its own full run.
- Current PR head before the next follow-up: `2b3154b913bd987a8ad44b8165c4633c2726dd38`, base Beta. The NFD/descriptor-priority changes are local-only and will be published as a new commit parented to that exact head, with a fresh Windows run required.
- Beta and main remain unchanged. No merge was performed. The packaged Windows Tauri WebView smoke test remains outstanding.
