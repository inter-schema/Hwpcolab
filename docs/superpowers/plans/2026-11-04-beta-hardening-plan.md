# Beta Hardening Implementation Plan

> Living plan for the Beta-only hardening workstream. The original implementation used GitHub APIs and Actions; COS was explicitly reauthorized for local QA on 2026-11-04. All integration remains limited to Beta.

> **This file owns task state, not the defect narrative.** Keep ordered work items, checkboxes, dependencies, and open/closed gates here. Record defect details, RED/GREEN evidence, exact CI runs, and review findings in [`BETA_REVIEW_LOG.md`](../../../BETA_REVIEW_LOG.md). Record architecture and technical rationale in [`docs/superpowers/specs/2026-11-04-beta-hardening-design.md`](../specs/2026-11-04-beta-hardening-design.md). Keep only short-lived continuation context in the local, untracked `TEMP_MEMORY.md`.

**Goal:** Improve the whole Hwpcolab Beta workstream's font handling, document-format validation, safe saves, imports, local development bridge, and repeatable verification.

**Baseline:** `e46f7230074cab63362f41d46cc1d1153085f745`  
**Implementation branch:** `codex/beta-hardening-2026-11-04`  
**Integration target:** `origin/Beta` only. Never modify `main`.  
**PR:** #1

## Guardrails

- Keep the existing Tauri 2 + Vite + RHWP architecture.
- Preserve frontend command names and payload shapes.
- Behavior changes require a regression test that fails for the intended reason before implementation.
- Do not blindly upgrade HOP/RHWP or replace vendored code.
- Commit both JavaScript and Rust lockfiles; verify with locked installs.
- Do not claim a platform-specific runtime check from unit-test evidence alone.

## Completed work

### Task 1: Test harness and runtime detection
- [x] Add Vitest and happy-dom.
- [x] Add deterministic frontend test command and CI.
- [x] Cover Tauri internal marker, tauri: protocol, browser, and absent-window cases.
- [x] Restore desktop runtime detection without invoking Tauri commands in a normal browser.

### Task 2: Native local-font catalog
- [x] Port the relevant HOP v0.4.5 fontdb catalog behavior into Hwpcolab's Tauri structure.
- [x] Add OS-specific extra user-font directories.
- [x] Register list_local_fonts and read_local_font.
- [x] Canonicalize requested paths and enforce approved-root/extension checks.
- [x] Add runtime and native path regression tests.

### Task 3: HWPX package validation and safe saves
- [x] Validate ZIP structure, first/stored mimetype, exact OWPML media type and required document parts.
- [x] Reject generic ZIPs, malformed packages and encrypted OWPML parts.
- [x] Recognize the binary ZIP local-file-header signature in native document routing.
- [x] Allow valid data descriptors on ordinary ZIP entries while keeping mimetype strict.
- [x] Add generated and repository HWPX fixtures.
- [x] Replace direct destination overwrites with same-directory temporary-file replacement, sync and cleanup in Rust and Vite paths.
- [x] Test successful overwrite and preservation of existing targets on failure.

### Task 4: DOCX import
- [x] Replace regex-only XML extraction with namespace-aware DOM parsing.
- [x] Preserve paragraph boundaries, text/entities, xml:space, tabs, breaks and table structure.
- [x] Add generated regression fixtures and repository DOCX/XLSX sample coverage.
- [x] Bound attachment and ZIP expansion sizes.

### Task 5: Vite font serving and path security
- [x] Add canonical path containment, extension checks, malformed-encoding handling and symlink escape tests.
- [x] Ensure rejected font paths terminate with 404 rather than falling through to general Vite static middleware.
- [x] Verify valid WOFF2 retrieval and production build.

### Task 6: LLM request lifecycle and development-bridge parity
- [x] Add deterministic test for native stale-cancellation cleanup.
- [x] Scope native cleanup to a unique AtomicU64 registration token.
- [x] Reject duplicate active request IDs.
- [x] Add identity-safe ActiveHttpRequests for the Vite bridge.
- [x] Align Vite bridge URL validation, timeout clamp, request/response byte limits, redirect policy and response-body cancellation with native behavior.
- [x] Keep timeout active until response body reading completes.
- [x] Add regression tests for duplicate IDs, ID reuse, URL schemes/userinfo, UTF-8 byte limits, timeout bounds, streaming response limits and BOM handling.

### Task 7: Lockfiles, dependency audit and CI
- [x] Commit package-lock.json and src-tauri/Cargo.lock.
- [x] Add a scoped lockfile-sync job for the same-repository Beta workstream.
- [x] Use npm ci and cargo test --locked for the same-repository PR and Beta push paths.
- [x] Add npm audit --audit-level=high; the latest completed frontend run passed.
- [x] Update actions/checkout to v7.0.1 and actions/setup-node to v7.1.0 to avoid the Node 20 runtime deprecation warning.
- [x] Run frontend tests/build on Ubuntu and Rust tests on Windows.

## Current verification evidence

- Frontend: 227 tests pass (32 files); production build passes.
- Windows Rust: the latest completed PR run (`41776400497`) passed rustfmt, Clippy and `cargo test --locked` for the previous published checkpoint. New native tests in the current local patch still need a fresh Windows run.
- Locked frontend install: npm ci passes.
- Dependency audit: npm audit --audit-level=high passes.
- The most recent completed integrated source checkpoint before the current local patch is workflow run `41803650311` on head `2407072f96b2a9415be638a96bce055a7d7b0792`; all three jobs succeeded, including Windows Clippy and `cargo test --locked`.
- The memory-only commit `e7d3a385e766a4c699a858b54ccf9a4e1264b87f` also has a fully successful run `41806776539`; no code changed in that commit.
- The current local source patch is not yet on the PR branch and needs fresh Windows Rust CI.

## Remaining work

### Task 8: Upstream comparison and final integration
- [x] Compare HOP and RHWP release versions against the reviewed upstream state.
- [x] Port only the relevant HOP native font catalog behavior.
- [x] Confirm RHWP v0.8.7 is already current; no blind engine replacement.
- [ ] Obtain the final current-head Windows cargo test --locked result and latest Actions run.
- [ ] Re-read the complete PR diff, including command registration, font allow-list, save parity, HWPX parity, DOCX import and LLM lifecycle.
- [ ] Record remaining platform-specific limitations. Packaged Tauri WebView smoke testing remains distinct from Rust unit tests.
- [ ] Confirm Beta has not moved behind the work branch.
- [ ] Integrate the reviewed result into Beta only; do not merge to main.

### Task 9: Native/TypeScript ZIP directory integrity parity
- [x] Reproduce the native duplicate-member acceptance from the current Windows CI failure.
- [x] Inspect the classic ZIP central directory before relying on the name-indexed Rust `ZipArchive` view.
- [x] Reject duplicate exact/ASCII-case-folded names, ZIP64 locator/sentinel archives, malformed central-directory bounds, encrypted entries, and non-UTF-8 names that are not marked UTF-8.
- [x] Verify the first `mimetype` local header against its central-directory metadata and payload; reject a data descriptor on that entry.
- [x] Keep legal data descriptors on ordinary entries accepted.
- [x] Align TypeScript ZIP64-locator and filename-encoding decisions with the native validator.
- [x] Add ASCII/Unicode duplicate, ZIP64, mimetype descriptor, and local-header mismatch regressions.
- [x] Validate the ZIP64 locator structure and test that locator-like bytes in a central-directory comment do not cause a false rejection.
- [x] Verify the TypeScript ZIP64 regression produces RED without the locator guard and GREEN with it.
- [x] Run Rust formatting, Clippy and the full locked test suite on Windows CI for the previously published checkpoint (`41776400497`).
- [ ] Obtain an independent reviewer/subagent pass against the latest new commit after publication. The first pass found the Unicode full-fold and local-header gaps; the attempted follow-up was blocked by the PR SHA lag.

### Task 10: Reject empty ZIP member names consistently
- [x] Reproduce a TypeScript/native parity gap with a valid HWPX envelope containing an empty-name ZIP member.
- [x] Confirm the TypeScript regression test fails before the fix because the validator returns `true`.
- [x] Reject zero-length central-directory filenames in the TypeScript validator.
- [x] Add a native unit test proving the raw ZIP preflight rejects a zero-length central-directory filename.
- [x] Re-run the full frontend suite, TypeScript compiler, Vite production build and npm security audit.
- [x] Validate the empty-name and mimetype CRC native tests, formatting and Clippy on Windows CI for the published checkpoint (`41776400497`).

### Task 11: Verify the mimetype payload CRC
- [x] Reproduce acceptance when local and central CRC fields agree with each other but not with the MIME payload.
- [x] Confirm the new TypeScript regression test fails before the fix.
- [x] Calculate CRC-32 over the actual `mimetype` bytes and compare it with the central-directory CRC.
- [x] Add a native regression fixture that corrupts both CRC fields while preserving the MIME payload.
- [x] Validate the native mimetype CRC regression and prior compile fixes on Windows CI (`41776400497`).

### Task 12: Validate every local file header against its central-directory entry
- [x] Reproduce acceptance when an ordinary `Contents/section0.xml` local name is changed while the central-directory name remains unchanged.
- [x] Reject mismatched local filename bytes, flags, compression method, CRC and size metadata for all entries in TypeScript and Rust.
- [x] Preserve valid ordinary entries with data descriptors whose local metadata is zero or matches the central directory.
- [x] Add matching TypeScript and Rust local-name-only mismatch regressions.
- [ ] Validate the native change and full fixture matrix on Windows CI.

### Task 13: Use full Unicode case folding for duplicate names
- [x] Confirm simple lowercasing allows `Straße` and `STRASSE` to remain distinct.
- [x] Add TypeScript full case folding with pinned `unicode-case-folding` 1.1.2 and a sharp-s regression.
- [x] Add Rust case-insensitive key comparison with pinned `unicase` 2.10.0 and a matching regression.
- [x] Update package and Cargo lockfiles locally; publish regenerated lockfiles through the repository lockfile-sync workflow.
- [ ] Verify exact JS/Rust behavior and locked installation in CI.

### Task 14: Verify ordinary ZIP data descriptors
- [x] Add a signed ordinary-entry data descriptor and confirm valid descriptors remain accepted.
- [x] Corrupt only the descriptor CRC and confirm the TypeScript test fails before the fix.
- [x] Verify descriptor CRC, compressed size and uncompressed size with and without the optional signature in both validators.
- [ ] Run the new native descriptor regression and complete Windows CI on the latest published head.

## Latest COS verification (2026-11-04)
- Full Vitest: 32 files, 231 tests passed after canonical Unicode normalization, extra full-fold pairs and descriptor-ambiguity coverage.
- Focused HWPX TypeScript suite: 28 tests passed.
- TypeScript compile and Vite production build: passed; pre-existing warnings remain for `/theme-init.js`, externalized Node modules, mixed Tauri imports and large chunks.
- `npm audit --audit-level=high`: 0 vulnerabilities.
- `git diff --check`: passed.
- `npm ci --no-audit --no-fund` passed in the reconciled worktree; esbuild 0.25.12 executes successfully. `npm audit --audit-level=high` reports 0 vulnerabilities.
- Rust tests/format/Clippy were not run locally because Cargo, rustc and rustfmt are unavailable in the Windows COS workspace. Windows run `41809160934` passed formatting, Clippy and `cargo test --locked` for the published overlap/descriptor patch at `6c55c4a`; the newest NFD and descriptor-priority Rust tests still need another CI run.
- Z.ai `glm-4.7-flash` returned HTTP 429 / error 1305; only then the BAZE backup `gpt-6-luna` returned HTTP 200. Independent Rust/TS reviewers found the Unicode normalization and descriptor-ambiguity edge cases; both are addressed in the current local patch. The reviewer also confirmed the LLM bridge is registered through `vite.config.ts`.
- Code commit `6c55c4a` passed all three jobs in run `41809160934`, including Windows formatting, Clippy, and `cargo test --locked`.
- Lockfile sync created bot-format child `2b3154b`; its bot-originated run `41809220727` was `action_required`. The current PR head before the next patch is `2b3154b913bd987a8ad44b8165c4633c2726dd38`.

### Current reconciled-worktree checkpoint (2026-11-04)
- Local resume branch was 6 commits ahead and 8 behind the remote PR branch from common ancestor `7be157a`; instead of pushing it, a disposable worktree was created at remote PR head `2407072` and the local patch was three-way applied.
- Conflict resolution preserves the remote Cargo lockfile (`unicase 2.10.0`) and the locally improved range-returning local-record validator/descriptor tests.
- The current reconciled worktree is based on `2407072`; the remote PR has since received memory commit `e7d3a38` and that commit's full Actions run `41806776539` succeeded. The source patch, plan/log and updated TEMP_MEMORY.md are prepared for one commit parented to e7d3a38.
- Current local evidence: HWPX 24/24, full Vitest 227/227, TypeScript compile, Vite build, npm ci, npm audit (0 vulnerabilities), and diff checks all pass. Fresh Windows Clippy/cargo test is still required for the new native tests.
- The overlap fixture is extractable by an ordinary ZIP reader before the HWPX validator rejects it, demonstrating that the overlap policy triggers rejection.

## References

- Design: `docs/superpowers/specs/2026-11-04-beta-hardening-design.md`
- Evidence and running history: `BETA_REVIEW_LOG.md`


### Latest Windows CI follow-up
- Run 41802915430 on the 7-source-file PR update: lockfile sync and frontend tests/build passed; Rust formatting passed; Clippy failed with E0382 because UniCase::new(name) moved the String used later for required-part matching. Rust tests were skipped.
- The Rust fix now uses UniCase::new(name.clone()); verify on a fresh Windows run.
- Cargo resolved the semver requirement unicase = "2.8.1" to lockfile version 2.10.0. JS uses unicode-case-folding 1.1.2.
- Implemented in the current local patch: reject overlapping local ZIP record ranges, accept valid unsigned descriptors, and independently reject descriptor CRC/compressed-size/uncompressed-size corruption. Fresh Windows native CI is still required.

### Task 15: Reject overlapping ZIP local records
- [x] Construct an HWPX fixture with a valid metadata local header embedded inside the stored payload of section0, and point the central metadata record at the embedded header.
- [x] Verify an ordinary ZIP reader can extract the metadata entry and the full section payload from the crafted archive.
- [x] Compare complete local-record ranges (header, name/extra, compressed data, and descriptor where present) and reject overlaps in TypeScript and Rust.
- [x] Add matching TypeScript and Rust regressions for the nested-record fixture.
- [x] Validate the native fixture and Windows Clippy/tests in run `41809160934` (source head `6c55c4a`).

### Task 16: Cover signed and unsigned data descriptors
- [x] Preserve valid signed descriptors and add a valid unsigned-descriptor fixture.
- [x] Validate descriptor CRC, compressed size and uncompressed size independently.
- [x] Reject truncated or mismatched descriptor layouts.
- [x] Run the expanded native descriptor matrix in Windows CI in run `41809160934` (source head `6c55c4a`).

### Task 17: Detect canonically equivalent Unicode package-part names
- [x] Normalize comparison keys with NFD, full Unicode case folding, then NFD again in TypeScript and Rust.
- [x] Add Latin NFC/NFD collision and precomposed Hangul/decomposed Jamo regressions.
- [x] Add full-fold pairs for Greek final sigma, ffi ligature, and Kelvin sign.
- [ ] Validate the new Rust normalization tests and locked dependency resolution on Windows CI.

### Task 18: Resolve signed/unsigned descriptor ambiguity compatibly
- [x] Factor descriptor resolution into testable helpers in TypeScript and Rust.
- [x] Prefer a fully matching signed 16-byte descriptor; use unsigned 12-byte only when signed does not match.
- [x] Add helper-level regression proving signed wins when both interpretations match.
- [ ] Validate the native helper and full descriptor matrix on Windows CI.

### Latest follow-up status
- Local HWPX 28/28, full Vitest 231/231, TypeScript compile and Vite build passed; npm audit reports 0 vulnerabilities.
- The NFD/descriptor-priority patch is local-only and must be published parented to `2b3154b`, followed by a fresh Windows Clippy/cargo test run.
- The packaged Windows Tauri WebView smoke test remains outstanding.

## 20. Authoritative handoff snapshot — 2026-11-04

This section supersedes earlier status summaries in this plan where their test counts, worktree, remote head, or next action differ. Historical checkpoints remain useful evidence and are not deleted.

### Active workspace and publication boundary

- Active COS reconciliation worktree: `[local path redacted]
- The worktree is currently detached at local `HEAD 2407072` and has staged/unstaged local changes. Do not assume this checkout is the live PR head.
- The latest remote PR head recorded in the handoff at 2026-11-04 19:05 Asia/Seoul is `2b3154b913bd987a8ad44b8165c4633c2726dd38`; refresh the PR/ref before any publication because that value has not been re-fetched in this task.
- Original `Hwpcolab-Beta`, older `Hwpcolab-Beta-hardening`, and divergent `Hwpcolab-Beta-resume` worktrees are preserved. Do not merge their working trees or reset/stash them as a cleanup shortcut.
- Do not commit, push, merge PR #1, or modify `Beta`/`main` as part of this plan-only/QA handoff. Publication and Beta integration remain separate guarded actions.

### Implemented locally, CI still open

- Task 17 code and regressions are present locally: canonical duplicate-name keys use NFD → full Unicode case fold → NFD in TypeScript and Rust; coverage includes Latin NFC/NFD, Hangul/Jamo, Greek final sigma, the `ﬃ` ligature, and Kelvin sign.
- Task 18 code and regression are present locally: prefer a fully matching signed 16-byte data descriptor; accept the unsigned 12-byte interpretation only if the signed interpretation does not match. A helper-level both-match test is present in both implementations.
- Task 19 code and regressions are present locally: TypeScript preserves leading U+FEFF in decoded ZIP filenames with `TextDecoder(..., { ignoreBOM: true })`; both runtimes reject a BOM-prefixed required package-part name.
- Task 20 code and regressions are present locally: both validators require ZIP64 locator/record structures to be after the complete classic central directory and require the declared ZIP64 record to end exactly at the locator.
- At the 20:05 COS verification checkpoint, the local full frontend suite passed 32 files / 234 tests, TypeScript compilation passed, and the Vite production build passed. The latest focused HWPX suite passed 31/31. This is local evidence, not Windows Rust CI.
- Windows run `41809160934` passed all three jobs for code head `6c55c4a46724fc0078567e9cb975c235a0a74622`, including Rust formatting, Clippy, `cargo test --locked`, frontend tests/build, and lockfile sync. It predates the Task 17–20 Rust changes.
- Bot-child run `41809220727` for `2b3154b` ended `action_required`; it is not a green check for that exact SHA.

### Remaining gates, in execution order

1. Reconcile the active local worktree against a freshly fetched PR head and verify the intended patch parent. Preserve staged and unstaged changes; do not publish from a stale detached base.
2. Re-run the focused HWPX suite, full frontend suite, TypeScript/build, dependency audit, and whitespace/diff checks against the exact local snapshot to be reviewed.
3. Run fresh Windows CI against the exact resulting source SHA. Require Rust formatting, Clippy, and `cargo test --locked` to pass for Tasks 17–20, plus frontend/build and lockfile synchronization.
4. Complete independent read-only Rust and TypeScript security/parity reviews against that same exact source snapshot/SHA. Convert any actionable finding into a RED regression, minimal fix, GREEN check, and a dated ledger entry. The current read-only passes and targeted re-reviews are complete; restart a reviewer only if source changes again.
5. Perform the packaged Windows Tauri WebView smoke test: open/edit/save/reopen real HWPX, enumerate/render installed fonts, and exercise LLM request/cancellation UI. Unit tests and Rust CI do not close this gate.
6. Re-read the complete final diff and re-check Beta/main refs. Only then prepare a separate Beta-only integration decision; do not merge PR #1 or touch `main` without explicit instruction.

### Current checkbox interpretation

- Tasks 1–16: implementation and listed regression work is recorded complete; their individual CI evidence remains tied to the exact historical SHAs/runs cited above and in `BETA_REVIEW_LOG.md`.
- Task 17: TypeScript implementation/tests pass locally; matching Rust implementation/regressions are present but need Windows CI execution.
- Task 18: TypeScript implementation/tests pass locally; matching Rust implementation/regressions are present but need Windows CI execution.
- Task 19: TypeScript BOM mismatch reproduced RED and fixed GREEN locally; Rust parity regression added but awaits Windows CI.
- Task 20: ZIP64 central-comment false rejection reproduced RED and fixed GREEN in TypeScript; matching Rust implementation/regression added but awaits Windows CI.
- Final PR review, packaged WebView smoke test, and Beta integration are not complete.

## 21. Fresh COS verification of the organized snapshot — 2026-11-04

Run from `[local path redacted]

- [x] `node .\\node_modules\\vitest\\vitest.mjs run src` — **32 files, 234 tests passed** at the final 20:11 verification checkpoint.
- [x] `node .\\node_modules\\vitest\\vitest.mjs run src/hop-host/core/hwpx-package.test.ts` — **31 tests passed** after the BOM and ZIP64-comment regressions.
- [x] `node .\\node_modules\\typescript\\bin\\tsc --noEmit` — passed.
- [x] `node .\\node_modules\\vite\\bin\\vite.js build` — final verification passed in **14.10 seconds** at 20:11.
- [x] `git diff --check` and `git diff --cached --check` — passed; only LF-to-CRLF working-copy warnings.
- [ ] Run `cargo fmt --check`, Clippy, and `cargo test --locked` on Windows CI for the current NFD, descriptor-priority, BOM and ZIP64-placement Rust changes. Cargo/rustc/rustfmt are unavailable in the local Windows PATH.
- [x] Collect two independent read-only reviews (Rust and TypeScript) plus targeted final re-reviews. No worker edited files. Reports are summarized in `BETA_REVIEW_LOG.md` sections 18–20.

The Vite build still reports the pre-existing `/theme-init.js` script warning, externalized Node core modules from `rhwpEngine.ts`, mixed static/dynamic Tauri API imports, and a main chunk above 500 kB. No npm audit was rerun in this verification pass; the last recorded `npm audit --audit-level=high` result is 0 vulnerabilities from the 19:05 local checkpoint.

Final repeat after the last code/document changes completed at 20:11: full Vitest 32 files / 234 tests passed; TypeScript compilation passed; Vite build passed in 14.10 seconds; staged and unstaged `git diff --check` passed. Rust checks remain unrun locally because Cargo/rustc/rustfmt are not available on the current Windows PATH.

## 22. Independent-review fixes and remaining gates — 2026-11-04

### Task 19: Preserve UTF-8 BOM as literal filename content

- [x] Add a RED fixture with every ZIP member BOM-prefixed; old TextDecoder unexpectedly accepted it.
- [x] Isolate the defect with canonical `mimetype` and only BOM-prefixed `Contents/content.hpf`; old decoder again returned true.
- [x] Use `TextDecoder('utf-8', { fatal: true, ignoreBOM: true })` so central and local filename decodes retain U+FEFF.
- [x] Add Rust parity regression for a BOM-prefixed required content part with UTF-8 flag set.
- [x] Focused HWPX TypeScript suite GREEN: 31/31.
- [ ] Fresh Windows Rust formatting, Clippy and `cargo test --locked` for the new parity regression.

### Task 20: Keep ZIP64-looking bytes in central comments/payloads opaque

- [x] Add a RED classic-HWPX fixture with a ZIP64-EOCD-shaped record in stored metadata payload and locator-shaped bytes in the final central-directory comment pointing to it.
- [x] Verify the old TypeScript validator falsely rejected the classic archive.
- [x] In TypeScript and Rust, require candidate locator and referenced ZIP64 EOCD to be after the complete classic central directory; require the declared ZIP64 record to end exactly at the locator.
- [x] Add matching Rust and TypeScript fixtures. Existing genuine-ZIP64 rejection remains covered.
- [x] Focused HWPX TypeScript suite GREEN: 31/31.
- [ ] Fresh Windows Rust formatting, Clippy and `cargo test --locked` for the ZIP64 false-positive regression and existing ZIP64 rejection fixture.

### Final acceptance gates

- [x] Repeat full local frontend tests, TypeScript compile, Vite build and diff checks on the exact final document/source snapshot. Final results: Vitest 32 files / 234 tests, `tsc` passed, Vite build 14.10s, both diff checks passed.
- [ ] Refresh live PR #1 head and refs before any publication; reconcile staged/unstaged changes against the exact remote parent. No commit or push occurred in this cleanup pass.
- [ ] Run all Windows CI jobs on the exact final source SHA, including lockfile sync, frontend/audit/build, Rust formatting, Clippy, and `cargo test --locked`.
- [ ] Re-review final source SHA if any code changes after the two completed read-only audits.
- [ ] Complete packaged Windows Tauri WebView smoke test (HWPX open/edit/save/reopen; installed-font enumerate/render; LLM UI/cancellation).
- [ ] Keep Beta and main untouched; do not merge PR #1 without separate explicit instruction.

## 23. Fresh COS continuation and exact-source status — 2026-11-04

### Remote state checked read-only

- Active QA worktree remains `[local path redacted]
- Refreshed `FETCH_HEAD` from `origin/codex/beta-hardening-2026-11-04`: `2efa9d6a4eab0ae0449c99196cfb89e23b76c693`, parent `2b3154b913bd987a8ad44b8165c4633c2726dd38`. The branch ref is one commit ahead of the PR metadata endpoint, which still reports `2b3154b`.
- The current local worktree includes additional uncommitted Task 19/20 and documentation changes relative to remote `2efa9d6`; it is not an exact checkout of the remote PR branch. No commit or push was attempted.
- PR #1 remains open/unmerged, base `Beta` at `e46f723`; `main` remains `0c23baa`. Exact Actions lookup for `2efa9d6` returned zero runs. Run `41809220727` on `2b3154b` concluded `action_required`; successful Windows Rust run `41809160934` on `6c55c4a` predates the current Rust changes.

### Fresh local validation — 2026-11-04 20:33 Asia/Seoul

- [x] Focused HWPX TypeScript suite: **31/31 passed**.
- [x] Full Vitest suite: **32 files, 234/234 passed**.
- [x] TypeScript compile (`tsc --noEmit`): passed.
- [x] Vite production build: passed in **14.28 seconds**; existing script/module, externalized Node core module, mixed Tauri import, and large-chunk warnings remain.
- [x] `npm audit --audit-level=high`: **0 vulnerabilities**.
- [x] Staged and unstaged `git diff --check`: passed; only LF-to-CRLF warnings.
- [ ] Windows `cargo fmt --check`, Clippy, and `cargo test --locked`: not run locally because Cargo/rustc/rustfmt are unavailable; a new Actions run on the exact final source SHA is still required.

### Gate interpretation

- The live PR and branch refs have now been refreshed, but the staged/unstaged local patch has not been reconciled into a commit parented to the exact remote branch head. Keep the section 22 publication/reconciliation gate open.
- Sections 18–20 already contain the independent Rust and TypeScript reviews and targeted re-reviews of the current Task 17–20 logic. No production source changed during this continuation, so no new code-review dispatch was triggered.
- Packaged Windows WebView smoke tests remain unperformed: real HWPX open/edit/save/reopen, installed-font enumeration/rendering, and LLM UI/cancellation.
- Preserve the original Beta, hardening, and resume worktrees. The follow-up PR branch may be updated under the user's explicit publication authorization; do not modify Beta/main or merge PR #1.

## 24. Publication candidate reconciled onto the current PR head — 2026-11-04

### Reconciled snapshot

- [x] Fetch the exact live branch head: `codex/beta-hardening-2026-11-04` at `2efa9d6a4eab0ae0449c99196cfb89e23b76c693`.
- [x] Preserve the original detached worktree's staged/unstaged patch and reapply it onto a new branch based on that exact SHA: `codex/beta-hardening-publish-2026-11-04`.
- [x] Resolve the ZIP64 boundary/test and documentation append conflicts; remove all conflict markers and stage the six-file diff.
- [x] Keep `TEMP_MEMORY.md` out of the patch and preserve the local continuation copy outside the repository. The remote tracked copy is unchanged.
- [x] Repeat focused and full frontend tests, TypeScript compile, Vite build, npm audit, and staged diff checks on this exact reconciled snapshot: HWPX **31/31**, full Vitest **32 files / 234 tests**, `tsc --noEmit` passed, Vite build **14.03s**, npm audit **0 vulnerabilities**, `git diff --cached --check` passed.
- [ ] Commit and push the new commit to the existing PR branch using a normal non-force push. The remote fast-forward must still start at `2efa9d6`; if it has moved, fetch and reconcile again.
- [ ] Verify the new GitHub Actions run on the exact pushed SHA, including Windows Rust formatting, Clippy, and `cargo test --locked`.

Beta, main, and PR merge state are outside this publication step. The packaged Windows Tauri WebView smoke test also remains an open acceptance gate.

--- Identity notice ---
This call is filed as Unattributed because its exact ChatGPT conversation is not known yet. Unattributed does not switch on Read-only mode or disable tools. Continue using every enabled tool, including apply_patch, exec_command, Desktop and Plugins; their existing permissions still apply. This request id owns its workspace, update_plan, terminals and agents/subagents. Use the process and run ids returned to this request; later exact proof attaches that state to its chat. A refusal about a particular process, worker or chat target applies only to that operation, not to file edits or other tools. Report the actual tool result; do not claim mutation or terminal tools are unavailable because of this notice, and never replay successful work.