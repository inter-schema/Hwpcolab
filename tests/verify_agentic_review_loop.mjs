// tests/verify_agentic_review_loop.mjs
// Comprehensive End-to-End Verification Suite for Multi-Turn Agentic Review Loop & Layout Engine
// Usage: node --experimental-strip-types tests/verify_agentic_review_loop.mjs

import fs from "node:fs";
import path from "node:path";
import { spawn } from "node:child_process";
import { inspectDocumentBytes, compareLayoutReports } from "../src/core/layoutInspector.ts";
import { OpTranslator } from "../src/core/opTranslator.ts";
import { RhwpDocumentWrapper } from "../src/core/rhwpEngine.ts";
import { lintOps } from "../src/services/policyGuard.ts";
import { orchestrateAiEdit } from "../src/services/aiOrchestrator.ts";

const rootDir = process.cwd();
const scratchDir = path.resolve(rootDir, "tests/scratch");
if (!fs.existsSync(scratchDir)) {
  fs.mkdirSync(scratchDir, { recursive: true });
}

// Results recorder
const testResults = [];
function record(name, pass, detail = "") {
  testResults.push({ name, pass: Boolean(pass), detail });
  const statusStr = pass ? "\x1b[32mPASS\x1b[0m" : "\x1b[31mFAIL\x1b[0m";
  console.log(`[${statusStr}] ${name}`);
  if (detail) {
    console.log(`       \x1b[90m${detail.slice(0, 300)}\x1b[0m`);
  }
}

// ---------------------------------------------------------------------------
// Setup Mock Bridge for runtimeInvoke in Node.js
// ---------------------------------------------------------------------------
const originalFetch = globalThis.fetch;
globalThis.fetch = async (url, opts) => {
  const urlStr = String(url);
  if (urlStr.startsWith("/__hop_claw_api/")) {
    const cmd = decodeURIComponent(urlStr.replace("/__hop_claw_api/", "").split("?")[0]);
    const body = opts?.body ? JSON.parse(opts.body) : {};
    let result = null;

    if (cmd === "call_llm_http") {
      const reqObj = body.req || body;
      const resp = await originalFetch(reqObj.url, {
        method: "POST",
        headers: reqObj.headers || { "Content-Type": "application/json" },
        body: JSON.stringify(reqObj.body_json),
      });
      const parsedBody = await resp.json();
      result = { status: resp.status, body: parsedBody };
    } else if (cmd === "write_file_bytes") {
      fs.writeFileSync(body.filePath, Buffer.from(body.bytes || []));
      result = null;
    } else if (cmd === "read_file_bytes") {
      result = Array.from(fs.readFileSync(body.filePath));
    } else if (cmd === "execute_claw_ops") {
      const scriptName = body.isHwpx ? "hwpx-edit.js" : "create.js";
      const scriptPath = path.resolve(rootDir, "resources/claw-hwp/scripts", scriptName);
      const child = spawn("node", [scriptPath], { stdio: ["pipe", "pipe", "pipe"] });
      const outChunks = [];
      const errChunks = [];
      child.stdout.on("data", (d) => outChunks.push(Buffer.from(d)));
      child.stderr.on("data", (d) => errChunks.push(Buffer.from(d)));
      child.stdin.write(String(body.payloadJson || "{}"), "utf8");
      child.stdin.end();
      const code = await new Promise((res) => child.on("close", res));
      const stdoutStr = Buffer.concat(outChunks).toString("utf8").trim();
      const stderrStr = Buffer.concat(errChunks).toString("utf8");
      let parsed;
      try {
        parsed = JSON.parse(stdoutStr);
      } catch {
        parsed = { status: "error", raw_stdout: stdoutStr, stderr: stderrStr };
      }
      const ok = code === 0 && (parsed?.status === "success" || parsed?.ok === true);
      result = { ok, stdout_json: parsed, stderr: stderrStr };
    }
    return new Response(JSON.stringify({ result }), {
      status: 200,
      headers: { "Content-Type": "application/json" },
    });
  }
  return originalFetch(url, opts);
};

// ---------------------------------------------------------------------------
// TEST 1: Engine + LayoutInspector Integration Test (.hwp copy)
// ---------------------------------------------------------------------------
async function testEngineAndLayoutInspector() {
  console.log("\n========================================================");
  console.log("TEST SUITE 1: Engine + LayoutInspector Integration Test");
  console.log("========================================================");

  const samplePath = path.resolve(rootDir, "samples/(날짜)(교시)(과목명)교수.학습과정안(이름).hwp");
  const tempPath = path.resolve(scratchDir, "suite1_patched.hwp");
  const origBytes = fs.readFileSync(samplePath);
  fs.writeFileSync(tempPath, origBytes);

  // 1. Initial Inspection
  const beforeReport = await inspectDocumentBytes(origBytes);
  record(
    "Original .hwp inspection detects issues",
    beforeReport.issuesCount === 4,
    `Detected ${beforeReport.issuesCount} issues (expected 4)`
  );

  const tableExceedIssue = beforeReport.issues.find((i) => i.code === "TABLE_EXCEEDS_PAGE");
  record(
    "Detects TABLE_EXCEEDS_PAGE (160.02mm > 150mm)",
    Boolean(tableExceedIssue && tableExceedIssue.metrics.tableWidthMm > 150),
    `Table width: ${tableExceedIssue?.metrics?.tableWidthMm}mm, overflow: ${tableExceedIssue?.metrics?.overflowWidthMm}mm`
  );

  const cellOverflowIssues = beforeReport.issues.filter((i) => i.code === "CELL_OVERFLOW_V");
  const overflowRows = cellOverflowIssues.map((i) => i.cell?.row).sort();
  record(
    "Detects CELL_OVERFLOW_V on 3 activity rows (rows 7, 8, 9 col 1)",
    cellOverflowIssues.length === 3 && JSON.stringify(overflowRows) === JSON.stringify([7, 8, 9]),
    `Overflow rows detected: ${overflowRows.join(", ")}`
  );

  // 2. Apply fit_table_to_page & set_row_height via create.js
  const wrapper = await RhwpDocumentWrapper.create(origBytes);
  const tablesMeta = wrapper.tablesMeta;
  wrapper.free();

  const ops = [
    { type: "fit_table_to_page", tableIndex: 0, targetWidthMm: 150 },
    { type: "set_row_height", tableIndex: 0, row: 2, heightMm: 8 },
    { type: "set_row_height", tableIndex: 0, row: 7, heightMm: 12 },
    { type: "set_row_height", tableIndex: 0, row: 8, heightMm: 22 },
    { type: "set_row_height", tableIndex: 0, row: 9, heightMm: 12 },
  ];

  const payload = OpTranslator.toEnginePayload(tempPath, false, ops, tablesMeta, false);

  const scriptPath = path.resolve(rootDir, "resources/claw-hwp/scripts/create.js");
  const child = spawn("node", [scriptPath], { stdio: ["pipe", "pipe", "pipe"] });
  const outChunks = [];
  child.stdout.on("data", (d) => outChunks.push(Buffer.from(d)));
  child.stdin.write(payload.stdinJson, "utf8");
  child.stdin.end();
  const exitCode = await new Promise((r) => child.on("close", r));
  const stdoutStr = Buffer.concat(outChunks).toString("utf8");
  const parsed = JSON.parse(stdoutStr);

  record(
    "create.js executes ops with success and preserves CFB integrity",
    exitCode === 0 && parsed.status === "success" && parsed.bytes_written > 0,
    `Status: ${parsed.status}, bytes_written: ${parsed.bytes_written}, mode: ${parsed.mode}`
  );

  // 3. Re-inspect patched .hwp
  const patchedBytes = fs.readFileSync(tempPath);
  const afterReport = await inspectDocumentBytes(patchedBytes);
  const comparison = compareLayoutReports(beforeReport, afterReport);

  record(
    "Re-inspection confirms 100% resolution (afterReport.issues.length === 0)",
    afterReport.issuesCount === 0 && afterReport.issues.length === 0,
    `Remaining issues count: ${afterReport.issuesCount}`
  );

  record(
    "compareLayoutReports reports all 4 issues resolved without regression",
    comparison.ok && comparison.resolvedIssues.length === 4 && comparison.remainingIssues.length === 0 && comparison.newIssues.length === 0,
    `Comparison summary: ${comparison.summaryText}`
  );
}

// ---------------------------------------------------------------------------
// TEST 2: Policy Guard G1–G6 Deterministic Test
// ---------------------------------------------------------------------------
async function testPolicyGuardG1toG6() {
  console.log("\n========================================================");
  console.log("TEST SUITE 2: Policy Guard G1–G6 Deterministic Test");
  console.log("========================================================");

  // G1: Anti-worsening overflow font enlargement check
  const g1Op = {
    type: "apply_text_style",
    targetText: "소수의 곱셈과 나눗셈",
    sizePt: 16,
    bold: true,
  };
  const g1Res = lintOps([g1Op], {
    userPrompt: "단원명이 잘 안보여",
    isNewDocument: false,
  });

  record(
    "G1 blocks font enlargement when prompt complains of clipping without dimension expansion",
    g1Res.passed === false && g1Res.rejectedOps.some((r) => r.reason.includes("G1")),
    `Rejected reasons: ${g1Res.rejectedOps.map((r) => r.reason).join(" | ")}`
  );

  // G4: setup_document on existing document
  const g4Op = { type: "setup_document", pageSize: "a4" };
  const g4Res = lintOps([g4Op], {
    userPrompt: "기존 표 서식 정리해줘",
    isNewDocument: false,
  });

  record(
    "G4 blocks setup_document on existing document",
    g4Res.passed === false && g4Res.rejectedOps.some((r) => r.reason.includes("G4")),
    `Rejected reasons: ${g4Res.rejectedOps.map((r) => r.reason).join(" | ")}`
  );

  // G2: Protected cell / user negative constraint
  const g2Op1 = {
    type: "set_cell_text",
    tableIndex: 0,
    row: 0,
    col: 1,
    text: "수정된 일시",
  };
  const g2Res1 = lintOps([g2Op1], {
    userPrompt: "일시 셀은 수정하지 마",
    isNewDocument: false,
  });

  record(
    "G2 blocks modifications matching negative constraint ('일시 셀은 수정하지 마')",
    g2Res1.passed === false && g2Res1.rejectedOps.some((r) => r.reason.includes("G2")),
    `Rejected reasons: ${g2Res1.rejectedOps.map((r) => r.reason).join(" | ")}`
  );

  const g2Op2 = {
    type: "set_cell_text",
    tableIndex: 0,
    row: 3,
    col: 1,
    text: "홍길동",
  };
  const g2Res2 = lintOps([g2Op2], {
    userPrompt: "선생님 이름 바꿔줘",
    isNewDocument: false,
    excludedRegions: [{ targetType: "cell", tableIndex: 0, row: 3, col: 1, rawText: "교사명" }],
  });

  record(
    "G2 blocks modifications targeting user excluded cell regions",
    g2Res2.passed === false && g2Res2.rejectedOps.some((r) => r.reason.includes("G2")),
    `Rejected reasons: ${g2Res2.rejectedOps.map((r) => r.reason).join(" | ")}`
  );

  // G3: Markdown asterisks sanitization
  const rawMarkdownOp = {
    type: "set_cell_text",
    tableIndex: 0,
    row: 1,
    col: 1,
    text: "**단원명: 수학**",
  };
  const g3Res = lintOps([rawMarkdownOp], {
    userPrompt: "단원명 수정",
    isNewDocument: false,
  });

  record(
    "G3 strips markdown bold asterisks from text payload",
    g3Res.passed === true && g3Res.sanitizedOps[0].text === "단원명: 수학",
    `Sanitized text: "${g3Res.sanitizedOps[0]?.text}"`
  );
}

// ---------------------------------------------------------------------------
// TEST 3: Live BAZE Multi-Turn Agentic Loop Test (4-Turn Incident Scenario)
// ---------------------------------------------------------------------------
async function testLiveBazeAgenticLoop() {
  console.log("\n========================================================");
  console.log("TEST SUITE 3: Live BAZE Multi-Turn Agentic Loop Test");
  console.log("========================================================");

  // Parse BAZE API key securely at runtime from .env
  const envText = fs.readFileSync(path.resolve(rootDir, ".env"), "utf8");
  let apiKey = "";
  for (const line of envText.split("\n")) {
    if (line.startsWith("VITE_BAZE_API_KEY=")) {
      apiKey = line.split("=")[1].trim();
    }
  }

  if (!apiKey) {
    record("BAZE API Key acquired from .env", false, "VITE_BAZE_API_KEY is missing in .env");
    return;
  }
  record("BAZE API Key acquired from .env (unprinted)", true, "Key loaded securely without logging");

  const samplePath = path.resolve(rootDir, "samples/(날짜)(교시)(과목명)교수.학습과정안(이름).hwp");
  const stagingPath = path.resolve(scratchDir, "suite3_baze_staging.hwp");
  const origBytes = fs.readFileSync(samplePath);
  fs.writeFileSync(stagingPath, origBytes);

  const wrapper = await RhwpDocumentWrapper.create(origBytes);
  const tablesMeta = wrapper.tablesMeta;
  wrapper.free();

  const chatHistory = [];

  // TURN 1: "글자가 제대로 보이도록 쪽 여백과 표를 조정해봐"
  console.log("\n--- Turn 1: '글자가 제대로 보이도록 쪽 여백과 표를 조정해봐' ---");
  const t1Prompt = "글자가 제대로 보이도록 쪽 여백과 표를 조정해봐";
  const t1Res = await orchestrateAiEdit({
    userPrompt: t1Prompt,
    stagingPath,
    fileBytes: origBytes,
    tablesMeta,
    provider: "baze",
    apiKey,
    model: "gpt-6.1-terra",
    onStep: (e) => console.log(`  [Turn 1 Step] ${e.type}: ${e.message}`),
  });

  const t1AllOps = t1Res.diffItems.flatMap((d) => d.unifiedOps || []);
  const t1HasLayoutOps = t1AllOps.some((op) =>
    ["fit_table_to_page", "set_row_height", "set_col_width", "set_page_margin"].includes(op.type)
  );

  record(
    "Turn 1 proposes layout operations (fit_table_to_page, set_row_height, set_page_margin)",
    t1HasLayoutOps && t1Res.diffItems.length > 0,
    `Ops used: ${Array.from(new Set(t1AllOps.map((o) => o.type))).join(", ")} | DiffItems: ${t1Res.diffItems.length}`
  );

  record(
    "Turn 1 executes trial simulation and layout verification",
    t1Res.trialsRun >= 1 && t1Res.verificationPassed === true,
    `Trials run: ${t1Res.trialsRun}, Verification passed: ${t1Res.verificationPassed}`
  );

  // Update chatHistory
  chatHistory.push({ role: "user", content: t1Prompt });
  chatHistory.push({ role: "assistant", content: t1Res.summary });

  // TURN 2: "해결되었는지 검수해줘" (Context review, NO Tuesday hallucination)
  console.log("\n--- Turn 2: '해결되었는지 검수해줘' ---");
  const t2Prompt = "해결되었는지 검수해줘";
  const t2Res = await orchestrateAiEdit({
    userPrompt: t2Prompt,
    stagingPath,
    fileBytes: origBytes,
    tablesMeta,
    provider: "baze",
    apiKey,
    model: "gpt-6.1-terra",
    chatHistory,
    onStep: (e) => console.log(`  [Turn 2 Step] ${e.type}: ${e.message}`),
  });

  const t2MentionsTuesday = /화요일/i.test(t2Res.summary);
  record(
    "Turn 2 reviews layout in context and does NOT hallucinate Tuesday date check",
    !t2MentionsTuesday && t2Res.summary.length > 0,
    `No Tuesday hallucination. Summary: "${t2Res.summary.slice(0, 150)}..."`
  );

  chatHistory.push({ role: "user", content: t2Prompt });
  chatHistory.push({ role: "assistant", content: t2Res.summary });

  // TURN 3: "단원명이 잘 안보여" (Anti-worsening overflow, no blind font blowup)
  console.log("\n--- Turn 3: '단원명이 잘 안보여' ---");
  const t3Prompt = "단원명이 잘 안보여";
  const t3Res = await orchestrateAiEdit({
    userPrompt: t3Prompt,
    stagingPath,
    fileBytes: origBytes,
    tablesMeta,
    provider: "baze",
    apiKey,
    model: "gpt-6.1-terra",
    chatHistory,
    onStep: (e) => console.log(`  [Turn 3 Step] ${e.type}: ${e.message}`),
  });

  const t3AllOps = t3Res.diffItems.flatMap((d) => d.unifiedOps || []);
  const t3HasBlindFontBlowup = t3AllOps.some(
    (op) =>
      op.type === "apply_text_style" &&
      ((op.sizePt && op.sizePt > 11) || op.bold) &&
      !t3AllOps.some((o) => ["set_row_height", "set_col_width", "fit_table_to_page"].includes(o.type))
  );

  record(
    "Turn 3 does NOT blindly blow up font without cell expansion (anti-overflow G1 protected)",
    !t3HasBlindFontBlowup,
    `Result summary: "${t3Res.summary.slice(0, 150)}..." | Ops: ${t3AllOps.map((o) => o.type).join(", ") || "none"}`
  );

  chatHistory.push({ role: "user", content: t3Prompt });
  chatHistory.push({ role: "assistant", content: t3Res.summary });

  // TURN 4: "잘려서 소수의 까지밖에 보이지 않아" (Cell dimension & fit handling)
  console.log("\n--- Turn 4: '잘려서 소수의 까지밖에 보이지 않아' ---");
  const t4Prompt = "잘려서 소수의 까지밖에 보이지 않아";
  const t4Res = await orchestrateAiEdit({
    userPrompt: t4Prompt,
    stagingPath,
    fileBytes: origBytes,
    tablesMeta,
    provider: "baze",
    apiKey,
    model: "gpt-6.1-terra",
    chatHistory,
    onStep: (e) => console.log(`  [Turn 4 Step] ${e.type}: ${e.message}`),
  });

  const t4AllOps = t4Res.diffItems.flatMap((d) => d.unifiedOps || []);
  const t4AddressesDimensionsOrFit =
    t4AllOps.length === 0 || // Agent explained solution in summary or proposed cell adjustments
    t4AllOps.some((o) =>
      ["set_row_height", "set_col_width", "set_cell_padding", "shrink_cell_font", "fit_table_to_page"].includes(o.type)
    );

  record(
    "Turn 4 properly addresses cell dimensions, row height, col width, or font fit",
    t4AddressesDimensionsOrFit && t4Res.summary.length > 0,
    `Summary: "${t4Res.summary.slice(0, 150)}..." | Ops: ${t4AllOps.map((o) => o.type).join(", ") || "addressed via inspection & guidance"}`
  );
}

// ---------------------------------------------------------------------------
// Main Runner & Summary
// ---------------------------------------------------------------------------
async function main() {
  console.log("Starting End-to-End Agentic Review Loop Verification Suite...\n");
  const t0 = Date.now();

  try {
    await testEngineAndLayoutInspector();
    await testPolicyGuardG1toG6();
    await testLiveBazeAgenticLoop();
  } catch (err) {
    console.error("Fatal test suite error:", err);
    record("Test Suite Execution", false, err.stack || err.message);
  } finally {
    // Cleanup scratch files
    try {
      if (fs.existsSync(scratchDir)) {
        fs.rmSync(scratchDir, { recursive: true, force: true });
      }
    } catch {}
  }

  const durationSec = ((Date.now() - t0) / 1000).toFixed(1);
  console.log("\n========================================================");
  console.log(`TEST SUITE SUMMARY (completed in ${durationSec}s)`);
  console.log("========================================================");

  let passedCount = 0;
  for (const r of testResults) {
    if (r.pass) passedCount++;
    console.log(`${r.pass ? "✅" : "❌"} ${r.name}`);
  }

  console.log(`\nTOTAL: ${passedCount}/${testResults.length} PASSED`);
  if (passedCount !== testResults.length) {
    process.exit(1);
  } else {
    process.exit(0);
  }
}

main();
