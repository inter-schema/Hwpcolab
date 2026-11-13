// tests/verify_ui_and_gpl.mjs
// Automated Playwright DOM & Geometry Verification for UI Fixes and GPL v3.0
import { chromium } from "playwright";
import { createServer } from "vite";
import fs from "node:fs";
import path from "node:path";

const rootDir = process.cwd();
const shotDir = path.resolve(rootDir, "tests/screenshots");
if (!fs.existsSync(shotDir)) {
  fs.mkdirSync(shotDir, { recursive: true });
}

const testResults = [];
function record(name, pass, detail = "") {
  testResults.push({ name, pass: Boolean(pass), detail });
  const statusStr = pass ? "\x1b[32mPASS\x1b[0m" : "\x1b[31mFAIL\x1b[0m";
  console.log(`[${statusStr}] ${name}`);
  if (detail) {
    console.log(`       \x1b[90m${detail.slice(0, 300)}\x1b[0m`);
  }
}

function toRect(b) {
  return {
    left: b.x,
    top: b.y,
    right: b.x + b.width,
    bottom: b.y + b.height,
  };
}

function assertOverlap(boxA, boxB) {
  if (!boxA || !boxB) return false;
  const a = toRect(boxA);
  const b = toRect(boxB);
  return !(
    a.right <= b.left ||
    a.left >= b.right ||
    a.bottom <= b.top ||
    a.top >= b.bottom
  );
}

async function run() {
  console.log("===============================================================");
  console.log("   AUTOMATED PLAYWRIGHT UI & GPL v3.0 VERIFICATION SUITE       ");
  console.log("===============================================================\n");

  let viteServer = null;
  let browser = null;

  try {
    // 1. Start Vite dev server programmatically
    console.log("Starting in-process Vite dev server...");
    viteServer = await createServer();
    await viteServer.listen();
    const listenUrl = (viteServer.resolvedUrls?.local?.[0] || "http://localhost:1420/").replace(/\/$/, "");
    console.log(`Vite dev server is ready at ${listenUrl}!\n`);

    // 2. Launch Playwright headless Chromium
    console.log("Launching headless Chromium...");
    browser = await chromium.launch({ headless: true });
    const context = await browser.newContext({
      viewport: { width: 1600, height: 950 },
    });
    const page = await context.newPage();

    console.log(`Navigating to ${listenUrl}...`);
    await page.goto(listenUrl, { waitUntil: "networkidle" });
    await page.evaluate(() => {
      localStorage.removeItem("hop_sidebar_width");
    });
    await page.reload({ waitUntil: "networkidle" });
    await page.waitForTimeout(800);

    // -------------------------------------------------------------------------
    // TEST 1: Reasoning Level Visibility & Zero Overlap
    // -------------------------------------------------------------------------
    console.log("\n--- TEST 1: Reasoning Level Visibility & Zero Overlap ---");
    const selectLocator = page.locator("#sidebar-reasoning-effort-select");
    const modelBtnLocator = page.locator("#sidebar-model-settings-btn");
    const diffBtnLocator = page.locator("#sidebar-mode-diff-btn");

    await page.waitForSelector("#sidebar-reasoning-effort-select", { timeout: 10000 });

    const selectBox = await selectLocator.boundingBox();
    const selectParentBox = await selectLocator.locator("xpath=..").boundingBox();
    const modelBtnBox = await modelBtnLocator.boundingBox();
    const diffBtnBox = await diffBtnLocator.boundingBox();

    record(
      "Reasoning Effort Select Exists and is Visible",
      selectBox !== null && selectBox.width > 50 && selectBox.height > 18,
      `Select dimensions: width=${selectBox?.width?.toFixed(1)}px, height=${selectBox?.height?.toFixed(1)}px`
    );

    const overlapWithModelBtn = assertOverlap(selectBox, modelBtnBox);
    record(
      "ZERO Overlap with Model Settings Button",
      !overlapWithModelBtn,
      `Select left=${selectBox?.x?.toFixed(1)}, right=${(selectBox.x + selectBox.width).toFixed(1)} | ModelBtn left=${modelBtnBox?.x?.toFixed(1)}, right=${(modelBtnBox.x + modelBtnBox.width).toFixed(1)}`
    );

    const overlapWithDiffBtn = assertOverlap(selectBox, diffBtnBox);
    record(
      "ZERO Overlap with Diff Review Button (Distinct Rows)",
      !overlapWithDiffBtn,
      `Select bottom=${(selectBox.y + selectBox.height).toFixed(1)}, DiffBtn top=${diffBtnBox?.y?.toFixed(1)}`
    );

    const parentOverlapDiff = assertOverlap(selectParentBox, diffBtnBox);
    record(
      "ZERO Overlap between Reasoning Container and Row 2 Controls",
      !parentOverlapDiff,
      `Container bottom=${(selectParentBox.y + selectParentBox.height).toFixed(1)}, Row2 top=${diffBtnBox?.y?.toFixed(1)}`
    );

    // Check options: none, low, medium, high, xhigh
    const options = await selectLocator.locator("option").all();
    const optionValues = await Promise.all(options.map((opt) => opt.getAttribute("value")));
    const requiredOptions = ["none", "low", "medium", "high", "xhigh"];
    const allOptionsPresent = requiredOptions.every((v) => optionValues.includes(v));
    record(
      "Reasoning Effort Options (none, low, medium, high, xhigh) Present",
      allOptionsPresent,
      `Found options: [${optionValues.join(", ")}]`
    );

    // Changing value updates cleanly
    await selectLocator.selectOption("low");
    let currentVal = await selectLocator.inputValue();
    record("Select Value Updated to 'low'", currentVal === "low", `Current value: ${currentVal}`);

    await selectLocator.selectOption("xhigh");
    currentVal = await selectLocator.inputValue();
    record("Select Value Updated to 'xhigh'", currentVal === "xhigh", `Current value: ${currentVal}`);

    // Restore to high
    await selectLocator.selectOption("high");

    // Take screenshot of header & fixed sidebar top controls
    const headerFixedPath = path.resolve(shotDir, "ui_fixed_sidebar_header.png");
    await page.screenshot({ path: headerFixedPath });
    record("Screenshot Saved: ui_fixed_sidebar_header.png", fs.existsSync(headerFixedPath), headerFixedPath);

    // -------------------------------------------------------------------------
    // TEST 2: Mouse Click-and-Slide Resizable Split Ratio
    // -------------------------------------------------------------------------
    console.log("\n--- TEST 2: Mouse Click-and-Slide Resizable Split Ratio ---");
    const sidebar = page.locator("#ai-chat-sidebar");
    const editorHost = page.locator("#hop-editor-host");
    const resizeHandle = page.locator("#sidebar-resize-handle");

    const initialSidebarBox = await sidebar.boundingBox();
    const initialEditorBox = await editorHost.boundingBox();
    record(
      "Initial Sidebar Width is ~400px",
      initialSidebarBox !== null && Math.abs(initialSidebarBox.width - 400) <= 5,
      `Initial sidebar width: ${initialSidebarBox?.width?.toFixed(1)}px, editor width: ${initialEditorBox?.width?.toFixed(1)}px`
    );

    // Drag handle to the right by 150px (from 400px to ~550px)
    const handleBox = await resizeHandle.boundingBox();
    const startX = handleBox.x + handleBox.width / 2;
    const startY = handleBox.y + handleBox.height / 2;

    await page.mouse.move(startX, startY);
    await page.mouse.down();
    await page.mouse.move(startX + 150, startY, { steps: 10 });
    await page.mouse.up();
    await page.waitForTimeout(300);

    const resizedSidebarBox = await sidebar.boundingBox();
    const resizedEditorBox = await editorHost.boundingBox();

    const widthIncreasedTo550 = Math.abs(resizedSidebarBox.width - 550) <= 8;
    record(
      "Mouse Drag Increases Sidebar Width to ~550px",
      widthIncreasedTo550,
      `Resized sidebar width: ${resizedSidebarBox.width.toFixed(1)}px (target: ~550px)`
    );

    const editorAdjusted = resizedEditorBox.width < initialEditorBox.width - 130;
    record(
      "Editor Host Pane Responded and Adjusted Accordingly",
      editorAdjusted,
      `Editor width changed from ${initialEditorBox.width.toFixed(1)}px to ${resizedEditorBox.width.toFixed(1)}px`
    );

    // Save resized screenshot
    const resizedPath = path.resolve(shotDir, "ui_resized_550px.png");
    await page.screenshot({ path: resizedPath });
    record("Screenshot Saved: ui_resized_550px.png", fs.existsSync(resizedPath), resizedPath);

    // Double-click handle to reset back to 400px
    await resizeHandle.dblclick();
    await page.waitForTimeout(300);
    const resetSidebarBox = await sidebar.boundingBox();
    record(
      "Double-Click Resize Handle Resets Width Back to 400px",
      Math.abs(resetSidebarBox.width - 400) <= 5,
      `Reset sidebar width: ${resetSidebarBox.width.toFixed(1)}px`
    );

    // Right-dock test: click dock toggle button
    const dockToggleBtn = page.locator("#header-dock-toggle-btn");
    await dockToggleBtn.click();
    await page.waitForTimeout(300);

    // Verify handle is now on left of sidebar (right side of editor)
    const rightHandleBox = await resizeHandle.boundingBox();
    record(
      "Sidebar Docked to Right Successfully",
      rightHandleBox !== null && rightHandleBox.x > 800,
      `Right dock handle X position: ${rightHandleBox?.x?.toFixed(1)}px`
    );

    // Ensure reset to 400px before drag
    await resizeHandle.dblclick();
    await page.waitForTimeout(200);

    // Drag handle left by 120px to increase right-docked sidebar to ~520px
    const rightHandleBoxNow = await resizeHandle.boundingBox();
    const rStartX = rightHandleBoxNow.x + rightHandleBoxNow.width / 2;
    const rStartY = rightHandleBoxNow.y + rightHandleBoxNow.height / 2;
    await page.mouse.move(rStartX, rStartY);
    await page.mouse.down();
    await page.mouse.move(rStartX - 120, rStartY, { steps: 10 });
    await page.mouse.up();
    await page.waitForTimeout(300);

    const rightResizedBox = await sidebar.boundingBox();
    record(
      "Right-Dock Drag Increases Sidebar Width to ~520px",
      Math.abs(rightResizedBox.width - 520) <= 8,
      `Right resized sidebar width: ${rightResizedBox.width.toFixed(1)}px (target: ~520px)`
    );

    // Reset back to left dock & 400px
    await dockToggleBtn.click();
    await page.waitForTimeout(200);
    await resizeHandle.dblclick();
    await page.waitForTimeout(200);

    // -------------------------------------------------------------------------
    // TEST 3: Top-Right Dummy Icons & User Profile Removed
    // -------------------------------------------------------------------------
    console.log("\n--- TEST 3: Top-Right Dummy Icons & User Profile Removed ---");
    const header = page.locator("#top-studio-header");
    const headerText = await header.innerText();

    record(
      "Hardcoded User '홍길동' is NOT Present in Header",
      !headerText.includes("홍길동"),
      `Header text sample: "${headerText.replace(/\s+/g, " ").trim().slice(0, 100)}"`
    );

    const docSearchIconCount = await header.locator('[title="문서 내 검색"]').count();
    record(
      "Dummy Icon [title='문서 내 검색'] is NOT Present",
      docSearchIconCount === 0,
      `Count: ${docSearchIconCount}`
    );

    const helpIconCount = await header.locator('[title="도움말"]').count();
    record(
      "Dummy Icon [title='도움말'] is NOT Present",
      helpIconCount === 0,
      `Count: ${helpIconCount}`
    );

    const badge2Count = await header.locator("text='2'").count();
    record(
      "Dummy Notification Badge '2' is NOT Present",
      badge2Count === 0,
      `Count: ${badge2Count}`
    );

    // -------------------------------------------------------------------------
    // TEST 4: GNU GPL v3.0 License Verification
    // -------------------------------------------------------------------------
    console.log("\n--- TEST 4: GNU GPL v3.0 License Verification ---");
    const licenseFilePath = path.resolve(rootDir, "LICENSE");
    const licenseFileExists = fs.existsSync(licenseFilePath);
    const licenseFileStats = licenseFileExists ? fs.statSync(licenseFilePath) : null;
    const licenseFileContent = licenseFileExists ? fs.readFileSync(licenseFilePath, "utf8") : "";

    const containsGplTitle = licenseFileContent.includes("GNU GENERAL PUBLIC LICENSE");
    const containsGplVersion = licenseFileContent.includes("Version 3, 29 June 2007");
    const licenseSizeOk = (licenseFileStats?.size || 0) > 30000;

    record(
      "Root LICENSE File Exists, Contains GPL v3 Header & Size > 30KB",
      licenseFileExists && containsGplTitle && containsGplVersion && licenseSizeOk,
      `File size: ${licenseFileStats?.size} bytes (minimum 30,000 required)`
    );

    // Open GPL v3.0 License Modal from Header
    const licenseBtn = page.locator("#header-license-btn");
    await licenseBtn.click();
    await page.waitForSelector("#license-modal", { state: "visible" });

    const modalText = await page.locator("#license-modal").innerText();
    const modalHasGplTitle = modalText.includes("GNU GENERAL PUBLIC LICENSE");
    const modalHasGplVersion = modalText.includes("Version 3, 29 June 2007");

    record(
      "GPL v3.0 Modal Opens and Renders Full License Information",
      modalHasGplTitle && modalHasGplVersion,
      `Modal text contains 'GNU GENERAL PUBLIC LICENSE' & 'Version 3, 29 June 2007'`
    );

    // Save license modal screenshot
    const gplv3ModalPath = path.resolve(shotDir, "ui_gplv3_modal.png");
    await page.screenshot({ path: gplv3ModalPath });
    record("Screenshot Saved: ui_gplv3_modal.png", fs.existsSync(gplv3ModalPath), gplv3ModalPath);

    // Close Modal
    await page.locator("#license-modal-close-btn").click();
    await page.waitForSelector("#license-modal", { state: "hidden" });
    record("License Modal Closes Cleanly via Close Button", true);

    console.log("\n===============================================================");
    console.log("                      TEST SUMMARY                             ");
    console.log("===============================================================");
    const passed = testResults.filter((r) => r.pass).length;
    const total = testResults.length;
    console.log(`TOTAL: ${total} | PASSED: ${passed} | FAILED: ${total - passed}`);

    if (passed !== total) {
      console.error("\x1b[31mSOME TESTS FAILED!\x1b[0m");
      process.exitCode = 1;
    } else {
      console.log("\x1b[32mALL UI AND LICENSE VERIFICATIONS PASSED 100%!\x1b[0m");
    }
  } catch (err) {
    console.error("Verification execution error:", err);
    process.exitCode = 1;
  } finally {
    if (browser) {
      console.log("Closing Playwright Chromium...");
      await browser.close().catch(() => {});
    }
    if (viteServer) {
      console.log("Closing Vite dev server...");
      await viteServer.close().catch(() => {});
    }
  }
}

run();
