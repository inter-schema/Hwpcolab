# Hwpcolab Beta Hardening — Temporary Working Memory

> Long-running engineering handoff. Read this file before resuming. Update it after every material test, review, commit, or CI result. This is working context, not a substitute for source, tests, or CI evidence.

Updated: 2026-11-04 18:34 Asia/Seoul

## Repository boundaries

- Repository: inter-schema/Hwpcolab
- PR: https://github.com/inter-schema/Hwpcolab/pull/1
- Integration base: Beta only. Never modify main and never merge PR #1 without a separate explicit instruction.
- Current remote PR head before the pending source patch: e7d3a385e766a4c699a858b54ccf9a4e1264b87f
- PR base: Beta at e46f7230074cab63362f41d46cc1d1153085f745
- main: last checked at 0c23baaf63a49e75fd176a8b18d182acfa91546e
- Local original checkout: [local path redacted]
- Original local branch: codex/beta-hardening-resume-2026-11-04, HEAD 699af2903b70ce20c3d855c8306e2a97d92036ed
- That local branch diverged from the PR branch at 7be157a: local was 6 commits ahead / 8 behind. Never push that local branch directly.
- Current reconciliation worktree: [local path redacted]
- Use GitHub API commit/tree/ref or a deliberate fast-forward only with an exact expected-head guard. Never force-push.

## API priority and secret handling

- Primary Z.ai secret file: [local path redacted]
- Backup BAZE secret file: [local path redacted]
- User instruction: try Z.ai first; use BAZE only after an actual Z.ai failure.
- Last primary test: glm-4.7-flash returned HTTP 429 / code 1305 (service overload).
- After that failure only, BAZE gpt-6-luna model listing and chat completion returned HTTP 200 and a non-empty response.
- Never print, commit, copy, or include any API key value in prompts or logs. Record provider/model/status/error code only.

## Current HWPX changes in the reconciled worktree

Existing remote code already has central-directory parsing, HWPX OWPML/mimetype checks, MIME payload CRC, ZIP64 rejection, duplicate/empty-name checks, full Unicode case folding, and per-entry local-header consistency.

New local-only changes now integrated in the detached PR worktree:

1. Calculate the complete local ZIP record end for each entry, including header, name/extra fields, compressed payload, and data descriptor when bit 3 is set.
2. Sort local-record ranges by start offset and reject intersections/overlaps.
3. Accept signed and unsigned ordinary-entry data descriptors. Keep first/stored mimetype strict.
4. Verify descriptor CRC, compressed size, and uncompressed size against central-directory metadata. Reject corrupted/truncated/ambiguous layouts.
5. Add an overlap fixture where a valid Contents/metadata.xml local header is embedded in stored Contents/section0.xml payload and the central entry points to it.
6. The overlap fixture now proves ordinary ZIP reading still extracts the metadata entry and 512-byte section payload; the HWPX validator rejects the overlapping record ranges.
7. Add unsigned descriptor acceptance and separate CRC/compressed-size/uncompressed-size corruption regressions.
8. Preserve Rust Unicode duplicate-name detection with UniCase and clone the String for key insertion to avoid Clippy E0382.

## Current source patch / staged files

Six tracked files are modified in Hwpcolab-Beta-current, plus this new memory file:

- BETA_REVIEW_LOG.md
- docs/superpowers/plans/2026-11-04-beta-hardening-plan.md
- src-tauri/Cargo.toml
- src-tauri/src/hwpx_package.rs
- src/hop-host/core/hwpx-package.test.ts
- src/hop-host/core/hwpx-package.ts

The actual current worktree has staged/unstaged changes across these six tracked files plus untracked TEMP_MEMORY.md; inspect git status and use the complete working-tree contents when publishing. package.json/package-lock.json and src-tauri/Cargo.lock already match the remote PR and were preserved. The Cargo lockfile contains unicase 2.10.0 and npm lockfile contains unicode-case-folding 1.1.2.

## Verified local evidence for the current reconciled patch

- Focused HWPX Vitest: 24/24 passed.
- Full Vitest: 32 files, 227/227 passed.
- TypeScript compiler: passed.
- Vite production build: passed in 24.54 seconds. Existing warnings remain: /theme-init.js is a non-module script, RHWP bundle externalizes Node core modules, Tauri imports are mixed static/dynamic, and the main JS chunk exceeds 500 kB.
- npm ci --no-audit --no-fund: passed after one sequential clean install. esbuild reports 0.25.12. Earlier failed installs were caused by overlapping npm processes and EBUSY/ENOTEMPTY; do not run concurrent installers.
- npm audit --audit-level=high: 0 vulnerabilities.
- git diff --check and git diff --cached --check passed before the latest log/memory edits; rerun before publication.
- Local COS Windows checkout has no cargo/rustc/rustfmt. Fresh Windows Actions is required for the new native tests.

## Remote CI evidence and gaps

- Remote PR head 2407072f96b2a9415be638a96bce055a7d7b0792 had Actions run 41803650311, all 3 jobs successful: lockfile sync, Windows Rust tests, frontend tests/build. This is prior to the current local-only overlap/descriptor expansion.
- Memory-only commit e7d3a385e766a4c699a858b54ccf9a4e1264b87f has run 41806776539, all 3 jobs successful. It changed only TEMP_MEMORY.md and does not verify current source changes.
- Current Rust overlap test additionally opens the fixture with zip::ZipArchive and verifies ordinary extraction before the HWPX validator rejects it. This native test has not yet run on Windows.
- Packaged Windows Tauri WebView smoke test remains outstanding: real HWPX open/edit/save/reopen, installed-font enumeration/rendering, and LLM UI/cancellation.
- Fresh independent reviewer pass against the next exact PR head remains required. Earlier review found the Unicode case-fold and ordinary local-header mismatch; those were fixed. Another review pass identified overlap and descriptor matrix gaps; those are now covered locally.

## Required next actions

1. Read TEMP_MEMORY.md, BETA_REVIEW_LOG.md, and docs/superpowers/plans/2026-11-04-beta-hardening-plan.md before continuing.
2. Recheck PR #1 head and Beta/main refs. Expected source-patch parent is e7d3a385e766a4c699a858b54ccf9a4e1264b87f.
3. Run git diff --cached --check and inspect all staged files; ensure no unrelated or secret files are present.
4. Read the exact current local contents of all six modified files plus TEMP_MEMORY.md and create blobs/tree/commit parented to e7d3a38. Update only codex/beta-hardening-2026-11-04 with force=false and expected_sha=e7d3a38... .
5. Check latest Actions run and inspect every job step: lockfile sync, npm ci/audit/Vitest/build, Windows rustfmt/Clippy/cargo test --locked.
6. Spawn/reuse two read-only subagents against the exact newly published head: Rust ZIP parser/security and TypeScript/parity/adjacent security. Do not let reviewers edit or commit.
7. If CI/review finds a bug, add a RED fixture, minimal fix, GREEN tests, update this file and the review ledger, and republish to the PR branch only.
8. Do not merge PR #1. Do not directly modify Beta or main.

## Reporting rules

- Separate local test results, Actions evidence, static review, and unrun packaged-app checks.
- Never claim a commit, push, review, test, or CI result without its actual tool evidence.
- Do not call the loop complete while concrete unverified checks or known risks remain.
