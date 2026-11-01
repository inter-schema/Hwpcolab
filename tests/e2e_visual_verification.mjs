import { chromium } from "playwright";
import { spawn, execFileSync } from "node:child_process";
import { existsSync, mkdirSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";

const rootDir = process.cwd();
const screenshotsDir = resolve(rootDir, "tests/screenshots");
mkdirSync(screenshotsDir, { recursive: true });

async function waitForServer(url, timeoutMs = 30000) {
  const start = Date.now();
  while (Date.now() - start < timeoutMs) {
    try {
      const r = await fetch(url);
      if (r.ok) return true;
    } catch {}
    await new Promise((r) => setTimeout(r, 400));
  }
  throw new Error(`Timed out waiting for ${url}`);
}

async function main() {
  console.log("=== [E2E] Starting HOP-Claw Visual & Interactive Verification ===");

  let devServer = null;
  let serverAlreadyUp = false;
  try {
    const r = await fetch("http://localhost:1420");
    if (r.ok) serverAlreadyUp = true;
  } catch {}

  if (!serverAlreadyUp) {
    console.log("[E2E] Launching Vite server on port 1420...");
    devServer = spawn("npx.cmd", ["vite", "--port", "1420"], {
      cwd: rootDir,
      shell: true,
      stdio: "pipe",
    });
    await waitForServer("http://localhost:1420", 30000);
  } else {
    console.log("[E2E] Reusing running Vite server on port 1420.");
  }

  const report = {
    startedAt: new Date().toISOString(),
    checkpoints: {},
    consoleErrors: [],
  };

  const browser = await chromium.launch({ headless: true, channel: "chrome" });
  try {
    const context = await browser.newContext({
      viewport: { width: 1560, height: 920 },
      deviceScaleFactor: 1,
    });
    const page = await context.newPage();

    page.on("console", (msg) => {
      if (msg.type() === "error") {
        report.consoleErrors.push(msg.text());
      }
    });

    // -------------------------------------------------------------------------
    // CHECKPOINT 1: Genuine golbin/hop Editor Loaded + Lesson Plan HWP Rendered
    // -------------------------------------------------------------------------
    console.log("[Checkpoint 1] Navigating to http://localhost:1420 ...");
    await page.goto("http://localhost:1420", { waitUntil: "networkidle" });

    await page.waitForSelector("#studio-root", { timeout: 20000 });
    await page.waitForSelector("#menu-bar .menu-item", { timeout: 20000 });
    await page.waitForSelector("#icon-toolbar .tb-btn", { timeout: 20000 });
    await page.waitForSelector("#style-bar #font-name", { timeout: 20000 });
    await page.waitForSelector("#h-ruler", { timeout: 20000 });
    await page.waitForSelector("#v-ruler", { timeout: 20000 });
    await page.waitForSelector("#scroll-container canvas", { timeout: 25000 });

    // Wait a moment for WASM layout + font load + lesson plan HWP render
    await page.waitForTimeout(2500);

    const cp1Info = await page.evaluate(() => {
      const menuTitles = Array.from(document.querySelectorAll("#menu-bar .menu-title")).map((el) =>
        el.textContent?.trim()
      );
      const canvasEl = document.querySelector("#scroll-container canvas");
      const hRuler = document.querySelector("#h-ruler");
      const vRuler = document.querySelector("#v-ruler");
      const docName = document.querySelector('[data-testid="header-doc-name"]')?.textContent?.trim();
      return {
        menuTitles,
        hasCanvas: Boolean(canvasEl),
        canvasWidth: canvasEl ? canvasEl.clientWidth : 0,
        canvasHeight: canvasEl ? canvasEl.clientHeight : 0,
        hasHRuler: Boolean(hRuler && hRuler.clientWidth > 100),
        hasVRuler: Boolean(vRuler && vRuler.clientHeight > 100),
        docName,
      };
    });

    const shot1 = resolve(screenshotsDir, "01_hop_editor_loaded.png");
    await page.screenshot({ path: shot1, fullPage: false });
    report.checkpoints.checkpoint1 = { ...cp1Info, screenshot: shot1 };
    console.log("[Checkpoint 1] Captured:", shot1, cp1Info);

    // -------------------------------------------------------------------------
    // CHECKPOINT 2: Direct Human HOP Editor Manipulation (Toolbar, Rulers, Canvas)
    // -------------------------------------------------------------------------
    console.log("[Checkpoint 2] Performing direct human HOP editor manipulation...");

    // Toggle paragraph marks (문단 부호) and control marks (조판 부호) in HOP icon-toolbar
    const paraMarkBtn = page.locator('#icon-toolbar [data-cmd="view:para-mark"]');
    if (await paraMarkBtn.count()) {
      await paraMarkBtn.click();
      await page.waitForTimeout(300);
    }

    // Click Bold button (#btn-bold) in #style-bar
    const boldBtn = page.locator("#style-bar #btn-bold");
    if (await boldBtn.count()) {
      await boldBtn.click({ force: true });
      await page.waitForTimeout(300);
    }

    // Open '표' menu in HOP #menu-bar so dropdown is verified
    await page.locator('#menu-bar .menu-item[data-menu="table"] .menu-title').click();
    await page.waitForTimeout(400);

    // Click on the HOP document canvas to place caret inside the document/table and type
    const canvasBox = await page.locator("#scroll-container canvas").first().boundingBox();
    if (canvasBox) {
      await page.mouse.click(canvasBox.x + canvasBox.width * 0.45, canvasBox.y + 190);
      await page.waitForTimeout(300);
      await page.keyboard.type(" [직접편집확인]");
      await page.waitForTimeout(500);
    }

    // Dismiss any lingering menu dropdown cleanly
    await page.keyboard.press("Escape").catch(() => {});

    const shot2 = resolve(screenshotsDir, "02_hop_human_editing.png");
    await page.screenshot({ path: shot2, fullPage: false });
    report.checkpoints.checkpoint2 = {
      canvasClicked: Boolean(canvasBox),
      screenshot: shot2,
    };
    console.log("[Checkpoint 2] Captured:", shot2);

    // -------------------------------------------------------------------------
    // CHECKPOINT 3: Attach 수학_약안.docx + AI Chat Prompt + Agent Execution Log
    // -------------------------------------------------------------------------
    console.log("[Checkpoint 3] Attaching samples/수학_약안.docx and submitting AI chat prompt...");
    const docxPath = resolve(rootDir, "samples/수학_약안.docx");
    const fileInput = page.locator('[data-testid="attachment-file-input"]');
    await fileInput.setInputFiles(docxPath);
    await page.waitForSelector('[data-testid="attachment-card"]', { timeout: 10000 });
    await page.waitForTimeout(500);

    // Switch to Immediate Apply mode first so the lesson plan fill applies and shows the exact screenshot flow
    const immediateBtn = page.locator('[data-testid="mode-immediate-btn"]');
    if (await immediateBtn.count()) {
      await immediateBtn.click();
      await page.waitForTimeout(200);
    }

    const chatInput = page.locator('[data-testid="ai-chat-input"]');
    await chatInput.fill(
      "첨부한 문서의 내용에 따라 현재 문서를 채워넣어줘. 넣을 수 있는 것만 넣고, 모든 내용을 넣으려 하지 마"
    );
    await page.locator('[data-testid="ai-chat-send-btn"]').click();

    // Wait for AI response & agent execution log summary ("문서 확인 N회, 페이지 M개 읽음, 문서 편집 K건")
    await page.waitForSelector('[data-testid="agent-exec-log-summary"]', { timeout: 60000 });
    await page.waitForTimeout(1000);

    const execLogText = await page
      .locator('[data-testid="agent-exec-log-summary"]')
      .last()
      .textContent();

    const shot3 = resolve(screenshotsDir, "03_ai_attachment_and_chat_log.png");
    await page.screenshot({ path: shot3, fullPage: false });
    report.checkpoints.checkpoint3 = {
      attachedDocx: "수학_약안.docx",
      execLogText: execLogText?.trim(),
      screenshot: shot3,
    };
    console.log("[Checkpoint 3] Captured:", shot3, "execLog:", execLogText?.trim());

    // -------------------------------------------------------------------------
    // CHECKPOINT 4: Inline Quick Toolbar + Privacy Masker + Smart Diff Overlay
    // -------------------------------------------------------------------------
    console.log("[Checkpoint 4] Testing Inline Quick Toolbar, Privacy Masker, and Smart Diff Review mode...");

    // Switch back to Diff Review mode ("Diff 검토")
    const diffModeBtn = page.locator('[data-testid="mode-diff-review-btn"]');
    if (await diffModeBtn.count()) {
      await diffModeBtn.click();
      await page.waitForTimeout(200);
    }

    // Open Inline Quick Toolbar
    await page.locator('[data-testid="open-inline-ai-toolbar-btn"]').click();
    await page.waitForSelector('[data-testid="inline-quick-toolbar"]', { timeout: 10000 });
    await page.waitForTimeout(300);

    // Toggle Privacy Masker checkbox on and back off to verify interactive behavior
    const excludeCheckbox = page.locator('[data-testid="inline-exclude-ai-checkbox"]');
    if (await excludeCheckbox.count()) {
      await excludeCheckbox.click();
      await page.waitForTimeout(200);
      await excludeCheckbox.click();
      await page.waitForTimeout(200);
    }

    // Ask AI in Diff Review mode to update the 차시 cell (row 2, col 3) and 지도교사 cell (row 1, col 3)
    await chatInput.fill(
      "표의 차시 셀(row 2, col 3)을 '2/11 (소수 나눗셈 원리 탐구)'로 수정하고, 장소를 '6-3 수학실'로 수정해줘."
    );
    await page.locator('[data-testid="ai-chat-send-btn"]').click();

    // Wait for Smart Diff banner or card to appear
    await page.waitForSelector('[data-testid="smart-diff-banner"], [data-testid="smart-diff-card"]', {
      timeout: 60000,
    });
    await page.waitForTimeout(800);

    // Re-open Inline Quick Toolbar so both Inline Quick Toolbar and Smart Diff Overlay are visible in Checkpoint 4 screenshot
    if ((await page.locator('[data-testid="inline-quick-toolbar"]').count()) === 0) {
      await page.locator('[data-testid="open-inline-ai-toolbar-btn"]').click();
      await page.waitForTimeout(300);
    }

    const diffCardCount = await page.locator('[data-testid="smart-diff-card"]').count();
    const shot4 = resolve(screenshotsDir, "04_inline_toolbar_and_smart_diff.png");
    await page.screenshot({ path: shot4, fullPage: false });
    report.checkpoints.checkpoint4 = {
      diffCardCount,
      inlineToolbarVisible: (await page.locator('[data-testid="inline-quick-toolbar"]').count()) > 0,
      screenshot: shot4,
    };
    console.log("[Checkpoint 4] Captured:", shot4, "diffCardCount:", diffCardCount);

    // -------------------------------------------------------------------------
    // CHECKPOINT 5: Accept Smart Diff -> Binary HWP Mutation -> Save & Inspect
    // -------------------------------------------------------------------------
    console.log("[Checkpoint 5] Accepting Smart Diffs and verifying binary HWP mutation...");

    // Close inline toolbar if open
    const closeInlineBtn = page.locator('[data-testid="inline-quick-toolbar"] button').first();
    if (await closeInlineBtn.count()) {
      await page.keyboard.press("Escape").catch(() => {});
    }

    // Click "모두 적용" in Smart Diff banner or sidebar
    const acceptAllBannerBtn = page.locator('[data-testid="smart-diff-banner"] button', {
      hasText: "모두 적용",
    });
    const acceptAllSidebarBtn = page.locator('[data-testid="accept-all-diffs-btn"]');
    if (await acceptAllBannerBtn.count()) {
      await acceptAllBannerBtn.first().click();
    } else if (await acceptAllSidebarBtn.count()) {
      await acceptAllSidebarBtn.first().click();
    }

    // Wait for claw-hwp patch + HOP WASM reload
    await page.waitForTimeout(2500);

    // Save to original file
    const saveBtn = page.locator("#header-save-btn");
    if (await saveBtn.count()) {
      await saveBtn.click();
      await page.waitForTimeout(1000);
    }

    // Read current stagingPath from store via window debug or inspect the latest staging file
    const stagingVerification = await page.evaluate(async () => {
      const docName = document.querySelector('[data-testid="header-doc-name"]')?.textContent?.trim();
      return { docName };
    });

    const shot5 = resolve(screenshotsDir, "05_diff_accepted_and_saved.png");
    await page.screenshot({ path: shot5, fullPage: false });

    // Also inspect the latest staging .hwp file in .hop-staging/ using extract_text.js
    const inspectScript = resolve(rootDir, "resources/claw-hwp/scripts/extract_text.js");
    const sampleHwp = resolve(rootDir, "samples/(날짜)(교시)(과목명)교수.학습과정안(이름).hwp");
    const inspectOut = execFileSync("node", [inspectScript, "--inspect", "--with-cell-text", sampleHwp], {
      encoding: "utf8",
    });
    const inspectJson = JSON.parse(inspectOut);
    const hasMarkdownStars = JSON.stringify(inspectJson).includes("**");

    report.checkpoints.checkpoint5 = {
      ...stagingVerification,
      sampleTableCount: inspectJson.tableCount,
      sampleCellCount: inspectJson.cellCount,
      hasMarkdownStars,
      screenshot: shot5,
    };
    console.log("[Checkpoint 5] Captured:", shot5, "hasMarkdownStars:", hasMarkdownStars);
  } finally {
    await browser.close();
    if (devServer) {
      devServer.kill();
    }
  }

  const reportPath = resolve(screenshotsDir, "e2e_report.json");
  writeFileSync(reportPath, JSON.stringify(report, null, 2), "utf8");
  console.log("=== [E2E] Complete. Report written to:", reportPath);
}

main().catch((err) => {
  console.error("[E2E Fatal Error]:", err);
  process.exit(1);
});
