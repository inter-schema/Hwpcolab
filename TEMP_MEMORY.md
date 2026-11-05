# Hwpcolab Beta Hardening — Temporary Working Memory

> Long-running engineering handoff. Read this file before resuming. Update it after every material test, review, commit, or CI result. This is working context, not a substitute for source, tests, or CI evidence.

Updated: 2026-11-05 23:59 Asia/Seoul

## Repository boundaries

- Repository: `inter-schema/Hwpcolab`
- Integration base: `Beta` (local commit: `3e67e14`, origin/Beta: `fb05cd96591f585234a6b443bc19f8378c40b726`)
- Current working branch: `codex/beta-hardening-2026-11-05`
  - Base: `Beta` (`3e67e14`)
  - Feature D commits: `2e57373` (memory core + adversarial PII + injection), `21a98e6` (memory UI + store + e2e)
- Origin remote: `https://github.com/inter-schema/Hwpcolab.git`
- Push permission: Confirmed (authenticated as `inter-schema`, push deliberately not performed)

## Verification status (2026-11-05)

1. **Unit Tests (`vitest run` / `npm test`)**:
   - **55 files, 420 tests passed, 0 failures (100% PASS)**.
   - Includes full suites for:
     - Stream A (HWPX nested table): `hwpxTableIndex.test.ts`, `nestedTable.engine.test.ts`, `opTranslator.nestedTable.test.ts`, `policyGuard.nestedTable.test.ts`, `useDocStore.defect03.test.ts`
     - Stream B (Custom provider & policy): `endpointPolicy.test.ts`, `openaiCompat.test.ts`
     - Stream C (Agentic loop v2 & legacy): `loopV2.test.ts`, `aiOrchestrator.integration.test.ts`, `aiOrchestrator.legacy.test.ts`, `claude.test.ts`, `toolRuntime.test.ts`
     - Stream D (Writing preference memory & PII safety): `piiPatterns.test.ts`, `frontmatter.test.ts`, `store.test.ts`, `generate.test.ts`, `refine.test.ts`, `injection.test.ts`, `adversarialPii.test.ts` (Gate 2 red-team), `memoryUi.test.ts`

2. **Frontend Build (`tsc && vite build`)**:
   - Zero TypeScript compile errors (`tsc --noEmit`).
   - Clean production bundle output in ~28s (`npm run build`).

3. **Live Playwright E2E Verification**:
   - `tests/e2e_writing_memory.mjs`: **32/32 passed (100%)**
     - Start screen / header triggers, multi-doc masked memory draft preview before save, open doc standalone memory, AI section diff refinement & rollback (v1->v2->v3), selective context injection & PII zero leakage.
   - `tests/e2e_providers_loops.mjs`: **26/26 passed (100%)**
     - Custom OpenAI-compatible provider & SSRF/metadata blocking, loop mode radio persistence (legacy vs v2), HWPX diff proposals, request cancellation (AbortSignal), nested table (`insert_nested_table`) diff proposal & apply.
   - `tests/verify_ui_and_gpl.mjs`: **23/23 passed (100%)**
     - Reasoning level selector layout overlap 0, split-pane resize slider, dummy icon removal, GPL v3.0 license verification.
   - `tests/verify_all_3_fixes.mjs`: **17/17 passed (100%)**
     - Font language auto-resolution in '대표' mode across 7 tables, file open routing, unclipped dropdown portals and IME composition persistence.

4. **Desktop Packaging (Tauri v2 + MinGW)**:
   - Toolchain environment loaded from `[local path redacted]
   - Standalone exe: `src-tauri/target/release/hop-claw.exe` (52.9 MB, valid PE x86_64).
   - WiX MSI installer: `src-tauri/target/release/bundle/msi/HOP-Claw_0.1.0_x64_en-US.msi` (35.2 MB).
   - NSIS setup installer: `src-tauri/target/release/bundle/nsis/HOP-Claw_0.1.0_x64-setup.exe` (31.9 MB).

## Next operator / agent guidelines

- When running Tauri commands locally, ensure MinGW bin is on PATH (`. [local path redacted]
- Any new test scripts or screenshots must be kept in scratch directories or gitignored folders to preserve git cleanliness.
- Target branch for integration remains `Beta`.
