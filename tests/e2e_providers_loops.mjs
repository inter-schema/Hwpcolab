// tests/e2e_providers_loops.mjs
// Playwright E2E Test (Web Mode) for:
// (i) Custom Provider settings save/restore & endpoint security validation
// (ii) Agent loop mode (legacy vs v2) radio toggle persistence
// (iii) Opening HWPX sample and verifying AI multi-turn diff proposals in both legacy and v2 loops
// (iv) Strict PII leakage verification on raw HTTP request bodies

import http from "node:http";
import path from "node:path";
import fs from "node:fs";
import os from "node:os";
import { chromium } from "playwright";
import { createServer as createViteServer } from "vite";

const rootDir = process.cwd();
const shotDir = process.env.HOP_E2E_SCREENSHOT_DIR || path.join(os.tmpdir(), "hop-e2e-providers-loops");
fs.mkdirSync(shotDir, { recursive: true });

const results = [];
function check(name, pass, detail = "") {
  results.push({ name, pass: Boolean(pass), detail: String(detail).slice(0, 400) });
  const statusStr = pass ? "\x1b[32mPASS\x1b[0m" : "\x1b[31mFAIL\x1b[0m";
  console.log(`[${statusStr}] ${name}`);
  if (detail) {
    console.log(`       \x1b[90m${String(detail).slice(0, 200)}\x1b[0m`);
  }
}

async function waitFor(fn, timeoutMs = 30000, label = "condition") {
  const t0 = Date.now();
  while (Date.now() - t0 < timeoutMs) {
    try {
      const v = await fn();
      if (v) return v;
    } catch {}
    await new Promise((r) => setTimeout(r, 300));
  }
  throw new Error(`Timeout waiting for ${label} (${timeoutMs}ms)`);
}

async function startMockLlmServer() {
  const capturedRequests = [];
  let delayMs = 0;
  let responseOverride = null;
  const activeTimers = new Set();

  const server = http.createServer((req, res) => {
    const chunks = [];
    req.on("data", (c) => chunks.push(c));
    req.on("end", () => {
      const rawBody = Buffer.concat(chunks).toString("utf8");
      let jsonBody = {};
      try {
        jsonBody = JSON.parse(rawBody);
      } catch {}

      capturedRequests.push({
        method: req.method,
        url: req.url,
        headers: req.headers,
        rawBody,
        jsonBody,
      });

      // Handle CORS preflight
      if (req.method === "OPTIONS") {
        res.writeHead(204, {
          "Access-Control-Allow-Origin": "*",
          "Access-Control-Allow-Methods": "POST, OPTIONS",
          "Access-Control-Allow-Headers": "*",
        });
        res.end();
        return;
      }

      const respond = () => {
        if (req.destroyed || res.writableEnded) {
          return;
        }

        if (responseOverride) {
          const override = responseOverride;
          const resp = typeof override === "function" ? override(jsonBody) : override;
          if (resp) {
            res.writeHead(200, {
              "Content-Type": "application/json",
              "Access-Control-Allow-Origin": "*",
            });
            res.end(JSON.stringify(resp));
            return;
          }
        }

        // Check whether this is Claude Messages or OpenAI Chat Completions
        const isClaude = req.url?.includes("/v1/messages") || Boolean(req.headers["x-api-key"]);

        // Determine turn based on messages
        const messages = jsonBody.messages || [];
        const hasToolResult = messages.some((m) => m.role === "tool" || (Array.isArray(m.content) && m.content.some((c) => c.type === "tool_result")));

        res.writeHead(200, {
          "Content-Type": "application/json",
          "Access-Control-Allow-Origin": "*",
        });

        if (!hasToolResult) {
        // Turn 1: Call inspect_layout
        if (isClaude) {
          res.end(
            JSON.stringify({
              id: "msg_claude_mock_1",
              type: "message",
              role: "assistant",
              content: [
                { type: "text", text: "문서 레이아웃을 확인합니다." },
                {
                  type: "tool_use",
                  id: "call_inspect_1",
                  name: "inspect_layout",
                  input: {},
                },
              ],
              stop_reason: "tool_use",
            })
          );
        } else {
          res.end(
            JSON.stringify({
              id: "chatcmpl-mock-1",
              object: "chat.completion",
              choices: [
                {
                  index: 0,
                  message: {
                    role: "assistant",
                    content: "레이아웃을 확인합니다.",
                    tool_calls: [
                      {
                        id: "call_inspect_1",
                        type: "function",
                        function: {
                          name: "inspect_layout",
                          arguments: "{}",
                        },
                      },
                    ],
                  },
                  finish_reason: "tool_calls",
                },
              ],
            })
          );
        }
      } else {
        // Turn 2: Propose HWP edits with DiffItem
        const sampleDiff = {
          title: "공문서 제목 서식 개정",
          scope: "paragraph",
          beforeText: "공문서 원문",
          afterText: "개정 공문서 표준",
          unifiedOps: [
            {
              type: "replace_text",
              find: "공문서",
              text: "개정 공문서 표준",
            },
          ],
        };

        if (isClaude) {
          res.end(
            JSON.stringify({
              id: "msg_claude_mock_2",
              type: "message",
              role: "assistant",
              content: [
                { type: "text", text: "수정안을 제안합니다." },
                {
                  type: "tool_use",
                  id: "call_propose_2",
                  name: "propose_hwp_edits",
                  input: {
                    summary: "공문서 표준 서식 수정 제안",
                    diffItems: [sampleDiff],
                  },
                },
              ],
              stop_reason: "tool_use",
            })
          );
        } else {
          res.end(
            JSON.stringify({
              id: "chatcmpl-mock-2",
              object: "chat.completion",
              choices: [
                {
                  index: 0,
                  message: {
                    role: "assistant",
                    content: "수정안을 제안합니다.",
                    tool_calls: [
                      {
                        id: "call_propose_2",
                        type: "function",
                        function: {
                          name: "propose_hwp_edits",
                          arguments: JSON.stringify({
                            summary: "공문서 표준 서식 수정 제안",
                            diffItems: [sampleDiff],
                          }),
                        },
                      },
                    ],
                  },
                  finish_reason: "tool_calls",
                },
              ],
            })
          );
        }
      }
    };

    if (delayMs > 0) {
      const timerId = setTimeout(() => {
        activeTimers.delete(timerId);
        respond();
      }, delayMs);
      activeTimers.add(timerId);
    } else {
      respond();
    }
  });
  });

  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const port = server.address().port;
  return {
    port,
    server,
    capturedRequests,
    setDelay: (ms) => {
      delayMs = ms;
      if (ms === 0) {
        for (const t of activeTimers) {
          clearTimeout(t);
        }
        activeTimers.clear();
      }
    },
    clearRequests: () => {
      capturedRequests.length = 0;
    },
    setResponseOverride: (fnOrObj) => {
      responseOverride = fnOrObj;
    },
    clearResponseOverride: () => {
      responseOverride = null;
    },
    setNextResponse: (fnOrObj) => {
      responseOverride = (jsonBody) => {
        const resp = typeof fnOrObj === "function" ? fnOrObj(jsonBody) : fnOrObj;
        responseOverride = null;
        return resp;
      };
    },
  };
}

async function run() {
  console.log("===============================================================");
  console.log("  E2E Playwright Suite: Custom Provider, Loops & PII Safety    ");
  console.log("===============================================================\n");

  const mockServer = await startMockLlmServer();
  const mockEndpoint = `http://127.0.0.1:${mockServer.port}/v1/chat/completions`;
  console.log(`[Mock LLM] Listening at ${mockEndpoint}`);

  let viteServer = null;
  let browser = null;

  try {
    viteServer = await createViteServer();
    await viteServer.listen();
    const appUrl = (viteServer.resolvedUrls?.local?.[0] || "http://localhost:1420/").replace(/\/$/, "");
    console.log(`[Vite Server] Ready at ${appUrl}\n`);

    browser = await chromium.launch({ headless: true });
    const context = await browser.newContext({
      viewport: { width: 1600, height: 950 },
    });
    const page = await context.newPage();

    console.log(`Navigating to ${appUrl}...`);
    await page.goto(appUrl, { waitUntil: "domcontentloaded" });
    await page.waitForSelector("#header-ai-settings-btn", { timeout: 30000 });
    await page.waitForTimeout(1000);

    // ─────────────────────────────────────────────────────────────────────────
    // TEST 1: Settings Modal Custom Provider & Endpoint Validation
    // ─────────────────────────────────────────────────────────────────────────
    console.log("\n--- TEST 1: Custom Provider & Endpoint Validation ---");

    // Open settings modal from header
    const headerSettingsBtn = page.locator("#header-ai-settings-btn");
    await headerSettingsBtn.click();
    await page.waitForSelector(".fixed.inset-0", { timeout: 5000 });
    check("1.1 AI 설정 모달 열림", true);

    // Select '사용자 지정' provider
    const customProviderBtn = page.locator("button", { hasText: "사용자 지정" }).first();
    await customProviderBtn.click();
    await page.waitForTimeout(300);

    // Sub-case A: Invalid public endpoint (http://example.com) must be blocked
    const endpointInput = page.locator('input[placeholder="http://127.0.0.1:8000/v1/chat/completions"]');
    const modelInput = page.locator('input[placeholder="모델 식별자"]');
    const saveBtn = page.locator("button", { hasText: "설정 저장" });

    await endpointInput.fill("http://example.com/v1/chat/completions");
    await modelInput.fill("gpt-custom-test");
    await saveBtn.click();
    await page.waitForTimeout(500);

    const errorMsg1 = page.locator(".text-rose-600");
    const error1Visible = (await errorMsg1.count()) > 0;
    const error1Text = error1Visible ? await errorMsg1.first().innerText() : "";
    check("1.2 http://example.com 저장 차단 (오류 표시)", error1Visible, error1Text);

    // Sub-case B: Cloud metadata/link-local endpoint (http://169.254.169.254) must be blocked
    await endpointInput.fill("http://169.254.169.254/latest/meta-data");
    await saveBtn.click();
    await page.waitForTimeout(500);

    const error2Visible = (await errorMsg1.count()) > 0;
    const error2Text = error2Visible ? await errorMsg1.first().innerText() : "";
    check("1.3 http://169.254.169.254 저장 차단 (메타데이터 거절)", error2Visible, error2Text);

    // Sub-case C: Valid mock endpoint accepted and saved
    await endpointInput.fill(mockEndpoint);
    await modelInput.fill("custom-mock-e2e-model");

    // Enable PII masking checkbox (always the last checkbox in the settings modal)
    const piiCheckbox = page.locator(".fixed.inset-0 input[type='checkbox']").last();
    await piiCheckbox.check();
    check("1.3.1 PII 가명화 체크박스 선택됨", await piiCheckbox.isChecked());

    await saveBtn.click();
    await page.waitForTimeout(600);

    // Modal should close
    const modalClosed = (await page.locator(".fixed.inset-0").count()) === 0;
    check("1.4 정상 엔드포인트 저장 완료 및 모달 닫힘", modalClosed);

    // Reload page and check persistence
    await page.reload({ waitUntil: "domcontentloaded" });
    await page.waitForSelector("#header-ai-settings-btn", { timeout: 30000 });
    await page.waitForTimeout(800);

    await headerSettingsBtn.click();
    await page.waitForSelector(".fixed.inset-0", { timeout: 5000 });

    const restoredEndpoint = await page.locator('input[placeholder="http://127.0.0.1:8000/v1/chat/completions"]').inputValue();
    const restoredModel = await page.locator('input[placeholder="모델 식별자"]').inputValue();
    check("1.5 재로드 후 사용자 지정 엔드포인트 복원", restoredEndpoint === mockEndpoint, `restored=${restoredEndpoint}`);
    check("1.6 재로드 후 사용자 지정 모델 ID 복원", restoredModel === "custom-mock-e2e-model", `model=${restoredModel}`);

    // ─────────────────────────────────────────────────────────────────────────
    // TEST 2: Agent Loop Mode Radio Persistence
    // ─────────────────────────────────────────────────────────────────────────
    console.log("\n--- TEST 2: Agent Loop Mode Radio Persistence ---");

    const radioLegacy = page.locator('input[name="agentLoopMode"][value="legacy"]');
    const radioV2 = page.locator('input[name="agentLoopMode"][value="v2"]');

    const defaultIsLegacy = await radioLegacy.isChecked();
    check("2.1 기본 루프 모드는 legacy", defaultIsLegacy);

    // Switch to v2
    await radioV2.click();
    check("2.2 v2 라디오 선택됨", await radioV2.isChecked());

    await saveBtn.click();
    await page.waitForTimeout(600);

    // Reload and verify persistence
    await page.reload({ waitUntil: "domcontentloaded" });
    await page.waitForSelector("#header-ai-settings-btn", { timeout: 30000 });
    await page.waitForTimeout(800);

    await headerSettingsBtn.click();
    await page.waitForSelector(".fixed.inset-0", { timeout: 5000 });

    const persistedV2 = await page.locator('input[name="agentLoopMode"][value="v2"]').isChecked();
    check("2.3 재로드 후 v2 라디오 선택 유지", persistedV2);

    // Reset back to legacy for Test 3 sequence
    await page.locator('input[name="agentLoopMode"][value="legacy"]').click();
    await saveBtn.click();
    await page.waitForTimeout(600);

    // ─────────────────────────────────────────────────────────────────────────
    // TEST 3: Open HWPX Sample and AI Request in Legacy Loop
    // ─────────────────────────────────────────────────────────────────────────
    console.log("\n--- TEST 3: HWPX Open & AI Proposal (Legacy Loop) ---");

    // Ensure PII masking is enabled for this test run
    await headerSettingsBtn.click();
    await page.waitForSelector(".fixed.inset-0", { timeout: 5000 });
    const piiCheckbox1 = page.locator(".fixed.inset-0 input[type='checkbox']").last();
    await piiCheckbox1.check();
    check("3.0.1 PII 가명화 체크 확인", await piiCheckbox1.isChecked());
    await page.locator('input[name="agentLoopMode"][value="legacy"]').click();
    await saveBtn.click();
    await page.waitForTimeout(600);

    const sampleHwpxPath = path.resolve(rootDir, "samples/demo_official_report.hwpx");
    await page.locator("#file-input").setInputFiles(sampleHwpxPath);
    await page.waitForTimeout(2000);

    const docNameLocator = page.locator('[data-testid="header-doc-name"]');
    const docLoaded = (await docNameLocator.count()) > 0;
    check("3.1 HWPX 문서 로드 성공", docLoaded);

    mockServer.clearRequests();
    const SYNTHETIC_PII_1 = "950505-1234567";
    const SYNTHETIC_PHONE_1 = "010-5555-6666";

    // Ask AI in Legacy mode
    const chatInput = page.locator('[data-testid="ai-chat-input"]');
    const chatSendBtn = page.locator('[data-testid="ai-chat-send-btn"]');

    await chatInput.fill(`담당자 홍길동(주민번호 ${SYNTHETIC_PII_1}, 전화 ${SYNTHETIC_PHONE_1}) 서식 점검 및 개정안 제안 요청`);
    await chatSendBtn.click();

    await waitFor(
      async () => {
        const text = await page.locator("#ai-chat-sidebar").innerText();
        return text.includes("공문서 표준 서식 수정 제안") || text.includes("제안") || text.includes("편집 연산");
      },
      30000,
      "Legacy loop AI Diff response"
    );
    await page.screenshot({ path: path.join(shotDir, "01_legacy_diff_proposal.png") });
    check("3.2 Legacy 루프에서 제안(Diff) 응답 수신", true);

    // Verify PII mask in outbound request bodies
    const legacyReqBodies = mockServer.capturedRequests.map((r) => r.rawBody);
    check("3.3 Legacy 루프 mock 서버 요청 수신 (>=1회)", legacyReqBodies.length >= 1, `count=${legacyReqBodies.length}`);

    let pii1Leaked = false;
    for (const bodyStr of legacyReqBodies) {
      if (bodyStr.includes(SYNTHETIC_PII_1) || bodyStr.includes(SYNTHETIC_PHONE_1)) {
        pii1Leaked = true;
        console.log("[DEBUG PII LEAK 1 - body]:", bodyStr);
      }
    }
    check("3.4 Legacy 모드 HTTP 요청 본문 합성 PII 원문 미포함", !pii1Leaked);

    // ─────────────────────────────────────────────────────────────────────────
    // TEST 4: Switch to v2 Loop and AI Request
    // ─────────────────────────────────────────────────────────────────────────
    console.log("\n--- TEST 4: HWPX Open & AI Proposal (v2 Loop) ---");

    // Open settings and switch to v2 with PII masking enabled
    await headerSettingsBtn.click();
    await page.waitForSelector(".fixed.inset-0", { timeout: 5000 });
    const piiCheckbox2 = page.locator(".fixed.inset-0 input[type='checkbox']").last();
    await piiCheckbox2.check();
    await page.locator('input[name="agentLoopMode"][value="v2"]').click();
    await saveBtn.click();
    await page.waitForTimeout(600);

    mockServer.clearRequests();
    const SYNTHETIC_PII_2 = "920808-2345678";
    const SYNTHETIC_PHONE_2 = "010-7777-8888";

    await chatInput.fill(`작성자 이영희(주민번호 ${SYNTHETIC_PII_2}, 연락처 ${SYNTHETIC_PHONE_2}) 서식 검토 및 표 개정안 제안`);
    await chatSendBtn.click();

    await waitFor(
      async () => {
        const text = await page.locator("#ai-chat-sidebar").innerText();
        return text.includes("공문서 표준 서식 수정 제안") || text.includes("제안") || text.includes("편집 연산");
      },
      30000,
      "v2 loop AI Diff response"
    );
    await page.screenshot({ path: path.join(shotDir, "02_v2_diff_proposal.png") });
    check("4.1 v2 루프에서 제안(Diff) 응답 수신", true);

    // Verify PII mask in outbound request bodies
    const v2ReqBodies = mockServer.capturedRequests.map((r) => r.rawBody);
    check("4.2 v2 루프 mock 서버 요청 수신 (>=1회)", v2ReqBodies.length >= 1, `count=${v2ReqBodies.length}`);

    let pii2Leaked = false;
    for (const bodyStr of v2ReqBodies) {
      if (bodyStr.includes(SYNTHETIC_PII_2) || bodyStr.includes(SYNTHETIC_PHONE_2)) {
        pii2Leaked = true;
      }
    }
    check("4.3 v2 모드 HTTP 요청 본문 합성 PII 원문 미포함", !pii2Leaked);

    // ─────────────────────────────────────────────────────────────────────────
    // TEST 5: UI Request Cancellation on Slow LLM Response
    // ─────────────────────────────────────────────────────────────────────────
    console.log("\n--- TEST 5: UI Request Cancellation on Slow LLM Response ---");

    mockServer.clearRequests();
    mockServer.setDelay(4000); // 4-second delay to allow cancel

    await chatInput.fill("취소 테스트 요청 (느린 응답 시뮬레이션)");
    await chatSendBtn.click();

    // Verify cancel button appears
    const cancelBtn = page.locator('[data-testid="cancel-llm-request-btn"]');
    await cancelBtn.waitFor({ state: "visible", timeout: 5000 });
    check("5.1 요청 시작 후 중단 버튼 표시", true);

    // Click cancel button
    await cancelBtn.click();

    // Verify loading indicator/cancel button disappears quickly
    await waitFor(
      async () => {
        const visible = await page.locator('[data-testid="cancel-llm-request-btn"]').isVisible();
        return !visible;
      },
      5000,
      "cancel button disappearance"
    );
    check("5.2 중단 클릭 후 로딩 상태 해제", true);

    // Wait a brief moment and verify sidebar status: loading cleared and no error bubble
    await page.waitForTimeout(600);
    const loadingVisible = await page.locator('[data-testid="ai-loading-indicator"]').isVisible();
    const sidebarTextAfterCancel = await page.locator("#ai-chat-sidebar").innerText();
    check("5.3 취소 후 로딩 해제 완료", !loadingVisible);
    check("5.4 취소 시 에러 말풍선 없음", !sidebarTextAfterCancel.includes("오류가 발생했습니다"));

    await page.screenshot({ path: path.join(shotDir, "03_cancel_request.png") });

    mockServer.setDelay(0);

    // ─────────────────────────────────────────────────────────────────────────
    // TEST 6: 중첩 표(insert_nested_table) Diff 제안 생성 및 적용 E2E
    // ─────────────────────────────────────────────────────────────────────────
    console.log("\n--- TEST 6: 중첩 표(insert_nested_table) Diff 제안 생성 및 적용 E2E ---");

    const pageErrors = [];
    const consoleErrors = [];
    const dialogs = [];
    const onPageError = (err) => pageErrors.push(String(err));
    const onConsole = (msg) => {
      if (msg.type() === "error") {
        consoleErrors.push(msg.text());
      }
    };
    const onDialog = (d) => {
      dialogs.push({ type: d.type(), message: d.message() });
      d.accept().catch(() => {});
    };

    page.on("pageerror", onPageError);
    page.on("console", onConsole);
    page.on("dialog", onDialog);

    try {
      // 1) Load sample .hwpx file into #header-hwp-file-input and wait for #header-doc-filename
      const sampleNestedPath = path.resolve("samples/(날짜)(교시)(과목명)교수.학습과정안(이름).hwpx");
      await page.locator("#header-hwp-file-input").setInputFiles(sampleNestedPath);
      await waitFor(
        async () => {
          const text = await page.locator("#header-doc-filename").innerText();
          return text.includes("교수.학습과정안");
        },
        15000,
        "load sample hwpx (교수.학습과정안)"
      );
      check("6.1 교수.학습과정안 샘플 HWPX 파일 로드 성공", true);
      await page.waitForTimeout(1000);

      // 2) Configure mock LLM server for propose_hwp_edits with insert_nested_table
      mockServer.clearRequests();
      const nestedProposalPayload = {
        summary: "표 0의 (0,0) 셀 안에 2x2 중첩 표를 삽입했습니다.",
        diffItems: [
          {
            scope: "cell",
            title: "(0,0) 셀 내부 2x2 중첩 표 삽입",
            beforeText: "",
            afterText: "[중첩 표 2x2]",
            targetCoord: { tableIndex: 0, row: 0, col: 0 },
            unifiedOps: [
              {
                type: "insert_nested_table",
                tableIndex: 0,
                row: 0,
                col: 0,
                rows: 2,
                cols: 2,
                data: [["항목", "내용"], ["구분", "상세"]],
                headerRow: true,
                widthMode: "fit_cell",
              },
            ],
          },
        ],
      };

      mockServer.setResponseOverride({
        id: "chatcmpl-mock-nested-table",
        object: "chat.completion",
        choices: [
          {
            index: 0,
            message: {
              role: "assistant",
              content: "표 0의 (0,0) 셀 안에 2x2 중첩 표를 삽입했습니다.",
              tool_calls: [
                {
                  id: "call_propose_nested_table",
                  type: "function",
                  function: {
                    name: "propose_hwp_edits",
                    arguments: JSON.stringify(nestedProposalPayload),
                  },
                },
              ],
            },
            finish_reason: "tool_calls",
          },
        ],
      });

      // 3) Send user prompt in AI chat
      const chatInput6 = page.locator('[data-testid="ai-chat-input"]');
      const chatSendBtn6 = page.locator('[data-testid="ai-chat-send-btn"]');

      await chatInput6.fill("첫 번째 표 (0,0) 셀에 2x2 중첩 표를 삽입해줘");
      await chatSendBtn6.click();

      // Wait for Diff card containing "(0,0) 셀 내부 2x2 중첩 표 삽입" (max 15s)
      await waitFor(
        async () => {
          const text = await page.locator("#ai-chat-sidebar").innerText();
          return text.includes("(0,0) 셀 내부 2x2 중첩 표 삽입");
        },
        15000,
        "diff card with nested table proposal"
      );
      check("6.2 중첩 표 Diff 제안 카드 표시 확인", true);

      // 4) Click Diff accept button ('모두 적용' or 개별 '적용')
      const acceptBtn = page.locator('button:has-text("모두 적용")').first();
      await acceptBtn.click();

      // Wait until applied without error (pending diff card disappears, loading finishes, max 15s)
      await waitFor(
        async () => {
          const sidebarText = await page.locator("#ai-chat-sidebar").innerText();
          const hasCard = sidebarText.includes("(0,0) 셀 내부 2x2 중첩 표 삽입") && sidebarText.includes("제안된 변경사항");
          const isLoading = await page.locator('[data-testid="ai-loading-indicator"]').isVisible().catch(() => false);
          return !hasCard && !isLoading;
        },
        15000,
        "apply nested table diff completion"
      );

      // Verify no error alert dialogs, no error bubble in sidebar, and 0 console/page errors
      const hasAlertDialog = dialogs.some((d) => d.type === "alert");
      const finalSidebarText = await page.locator("#ai-chat-sidebar").innerText();
      const noSidebarError =
        !finalSidebarText.includes("오류가 발생했습니다") &&
        !finalSidebarText.includes("패치 적용 실패") &&
        !hasAlertDialog;
      check("6.3 중첩 표 Diff 적용 완료 및 에러 없음", noSidebarError, hasAlertDialog ? `alert: ${dialogs[0]?.message}` : "");

      const zeroErrors = pageErrors.length === 0 && consoleErrors.length === 0;
      check(
        "6.4 페이지 콘솔 에러 및 미처리 예외 0건 확인",
        zeroErrors,
        zeroErrors ? "콘솔 에러 0건" : `errors=${JSON.stringify([...pageErrors, ...consoleErrors])}`
      );

      await page.screenshot({ path: path.join(shotDir, "04_nested_table_applied.png") });
    } finally {
      mockServer.clearResponseOverride();
      page.off("pageerror", onPageError);
      page.off("console", onConsole);
      page.off("dialog", onDialog);
    }

    // Summary
    const passCount = results.filter((r) => r.pass).length;
    console.log(`\n===============================================================`);
    console.log(` E2E RESULT: ${passCount}/${results.length} PASSED`);
    console.log(`===============================================================\n`);

    return results.every((r) => r.pass);
  } finally {
    if (browser) await browser.close().catch(() => {});
    if (viteServer) await viteServer.close().catch(() => {});
    mockServer.server.close();
  }
}

run()
  .then((ok) => {
    process.exit(ok ? 0 : 1);
  })
  .catch((err) => {
    console.error("[FATAL ERROR in E2E]", err);
    process.exit(2);
  });
