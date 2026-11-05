// tests/e2e_writing_memory.mjs
// Playwright E2E Test (Web Mode) for:
// 1. 시작화면 별개 버튼 확인 (#home-writing-memory-btn 및 #header-writing-memory-btn)
// 2. 다중 문서 수동 생성 + 마스킹 + 미리보기 확정 전 미저장 검증
// 3. 열린 문서 트리거(#sidebar-create-memory-from-doc-btn) 및 독립 메모리 생성 검증
// 4. AI로 다듬기(섹션별 부분 수락/거절) + 리비전 롤백 검증
// 5. AI 채팅창 선택적 메모리 활성화 및 수정 요청 검증

import http from "node:http";
import path from "node:path";
import fs from "node:fs";
import os from "node:os";
import { chromium } from "playwright";
import { createServer as createViteServer } from "vite";

const rootDir = process.cwd();
const shotDir =
  process.env.HOP_E2E_SCREENSHOT_DIR ||
  path.join(os.tmpdir(), "hop-e2e-writing-memory");
fs.mkdirSync(shotDir, { recursive: true });

const results = [];
function check(name, pass, detail = "") {
  results.push({
    name,
    pass: Boolean(pass),
    detail: String(detail).slice(0, 400),
  });
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

async function closeModal(page) {
  const modal = page.locator("#writing-memory-modal");
  if ((await modal.count()) > 0) {
    const closeBtn = page.locator("#writing-memory-modal button[title='닫기']");
    if ((await closeBtn.count()) > 0 && (await closeBtn.isVisible())) {
      await closeBtn.click();
    } else {
      await page.evaluate(() => {
        if (window.useDocStore) window.useDocStore.getState().closeMemoryModal();
      });
    }
    await page.waitForTimeout(400);
  }
}

async function startMockLlmServer() {
  const capturedRequests = [];
  let nextMemoryResponse = null;

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

      res.writeHead(200, {
        "Content-Type": "application/json",
        "Access-Control-Allow-Origin": "*",
      });

      const messages = jsonBody.messages || [];
      const systemPrompt = jsonBody.systemPrompt || "";
      const lastMsg = messages[messages.length - 1] || {};
      const lastContent =
        typeof lastMsg.content === "string"
          ? lastMsg.content
          : JSON.stringify(lastMsg.content || "");

      // 1. Is this memory generation or refinement?
      const isMemoryTask =
        jsonBody.tools?.length === 0 ||
        jsonBody.toolChoice === "none" ||
        systemPrompt.includes("한국어 공문서·보고서·업무 양식") ||
        lastContent.includes("마크다운 스타일 가이드") ||
        lastContent.includes("현재 작성 선호사항 가이드");

      if (isMemoryTask) {
        if (nextMemoryResponse) {
          const resp =
            typeof nextMemoryResponse === "function"
              ? nextMemoryResponse(jsonBody)
              : nextMemoryResponse;
          nextMemoryResponse = null;
          res.end(JSON.stringify(resp));
          return;
        }

        // Default memory generation response
        res.end(
          JSON.stringify({
            id: "chatcmpl-mock-gen-default",
            object: "chat.completion",
            choices: [
              {
                index: 0,
                message: {
                  role: "assistant",
                  content: `## 문서 유형·양식
- 공문서 표준 양식

## 어투·문체
- 간결한 개조식 문체

## 용어·표기
- 행정 표준 표기

## 서식·표 규칙
- 표 서식 표준

## 구조(제목 체계)
- 1. -> 가.

## 금지·지양
- 모호한 표현 지양

## 예시 문구(일반화됨)
- 예시 문구 표준`,
                },
                finish_reason: "stop",
              },
            ],
          })
        );
        return;
      }

      // 2. Chat edit agent turn
      const hasToolResult = messages.some(
        (m) =>
          m.role === "tool" ||
          (Array.isArray(m.content) &&
            m.content.some((c) => c.type === "tool_result"))
      );

      if (!hasToolResult) {
        // Turn 1: Call inspect_layout
        res.end(
          JSON.stringify({
            id: "chatcmpl-mock-turn1",
            object: "chat.completion",
            choices: [
              {
                index: 0,
                message: {
                  role: "assistant",
                  content: "문서 레이아웃을 확인합니다.",
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
      } else {
        // Turn 2: Propose HWP edits
        res.end(
          JSON.stringify({
            id: "chatcmpl-mock-turn2",
            object: "chat.completion",
            choices: [
              {
                index: 0,
                message: {
                  role: "assistant",
                  content: "선택된 작성 선호사항에 맞춰 수정을 제안합니다.",
                  tool_calls: [
                    {
                      id: "call_propose_2",
                      type: "function",
                      function: {
                        name: "propose_hwp_edits",
                        arguments: JSON.stringify({
                          summary: "작성 선호사항 기반 첫 문단 수정 제안",
                          diffItems: [
                            {
                              title: "첫 문단 선호사항 적용",
                              scope: "paragraph",
                              beforeText: "공문서 원문",
                              afterText: "개정 공문서 표준 반영 문단",
                              unifiedOps: [
                                {
                                  type: "replace_text",
                                  find: "공문서",
                                  text: "개정 공문서 표준 반영 문단",
                                },
                              ],
                            },
                          ],
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
    });
  });

  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const port = server.address().port;
  return {
    port,
    server,
    capturedRequests,
    clearRequests: () => {
      capturedRequests.length = 0;
    },
    setNextMemoryResponse: (fnOrObj) => {
      nextMemoryResponse = fnOrObj;
    },
  };
}

async function run() {
  console.log("===============================================================");
  console.log("  E2E Playwright Suite: Writing Memory Safety & Lifecycle     ");
  console.log("===============================================================\n");

  const mockServer = await startMockLlmServer();
  const mockEndpoint = `http://127.0.0.1:${mockServer.port}/v1/chat/completions`;
  console.log(`[Mock LLM] Listening at ${mockEndpoint}`);

  let viteServer = null;
  let browser = null;

  try {
    viteServer = await createViteServer();
    await viteServer.listen();
    const appUrl = (
      viteServer.resolvedUrls?.local?.[0] || "http://localhost:1420/"
    ).replace(/\/$/, "");
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

    // AI Provider 설정 (사용자 지정 Mock LLM 연결)
    console.log("\n--- Setting up Custom Mock Provider ---");
    await page.locator("#header-ai-settings-btn").click();
    await page.waitForSelector(".fixed.inset-0", { timeout: 5000 });
    await page.locator("button", { hasText: "사용자 지정" }).first().click();
    await page.waitForTimeout(300);

    const endpointInput = page.locator(
      'input[placeholder="http://127.0.0.1:8000/v1/chat/completions"]'
    );
    const modelInput = page.locator('input[placeholder="모델 식별자"]');
    const saveBtn = page.locator("button", { hasText: "설정 저장" });

    await endpointInput.fill(mockEndpoint);
    await modelInput.fill("mock-memory-e2e-model");
    await saveBtn.click();
    await page.waitForTimeout(600);
    check("0.1 Mock LLM Provider 설정 완료", true);

    // ─────────────────────────────────────────────────────────────────────────
    // 시나리오 1: 시작화면 별개 버튼 확인
    // ─────────────────────────────────────────────────────────────────────────
    console.log("\n--- 시나리오 1: 시작화면 별개 버튼 확인 ---");

    const homeMemoryBtn = page.locator("#home-writing-memory-btn");
    const homeBtnExists = (await homeMemoryBtn.count()) > 0;
    check("1.1 #home-writing-memory-btn DOM 존재 확인", homeBtnExists);

    // 시작화면 버튼 클릭 시 모달이 'create_multi' 탭으로 열리는지 확인
    if (homeBtnExists && (await homeMemoryBtn.isVisible())) {
      await homeMemoryBtn.click();
    } else {
      // 이벤트 브리지로 트리거
      await page.evaluate(() => {
        window.dispatchEvent(
          new CustomEvent("hop:open-writing-memory", {
            detail: { tab: "create_multi" },
          })
        );
      });
    }

    await page.waitForSelector("#writing-memory-modal", { timeout: 5000 });
    check("1.2 #writing-memory-modal 표시 확인", true);

    // 여러 문서로 새로 만들기 탭이 열려 있는지 확인
    const createNameInput = page.locator("#memory-create-name-input");
    const isMultiCreateTab = (await createNameInput.count()) > 0;
    check(
      "1.3 모달이 '여러 문서로 새로 만들기' 탭으로 열림 확인",
      isMultiCreateTab
    );

    // 모달 닫기
    await closeModal(page);
    const modalClosed1 = (await page.locator("#writing-memory-modal").count()) === 0;
    check("1.4 모달 닫기 성공", modalClosed1);

    // 상단 #header-writing-memory-btn 클릭 시에도 모달이 열리는지 확인
    await page.locator("#header-writing-memory-btn").click();
    await page.waitForSelector("#writing-memory-modal", { timeout: 5000 });
    const modalOpenFromHeader =
      (await page.locator("#writing-memory-modal").count()) > 0;
    check("1.5 #header-writing-memory-btn 클릭으로 모달 열림 확인", modalOpenFromHeader);

    // ─────────────────────────────────────────────────────────────────────────
    // 시나리오 2: 다중 문서 수동 생성 + 마스킹 + 미리보기 확정 전 미저장 검증
    // ─────────────────────────────────────────────────────────────────────────
    console.log(
      "\n--- 시나리오 2: 다중 문서 수동 생성 + 마스킹 + 미리보기 확정 전 미저장 검증 ---"
    );

    // '여러 문서로 새로 만들기' 탭으로 전환
    await page.locator("#memory-tab-create-multi").click();
    await page.waitForTimeout(300);

    // 임시 디렉터리에 합성 PII가 포함된 2개 샘플 문서 생성
    const tempDir = os.tmpdir();
    const sampleDoc1Path = path.join(tempDir, `sample_pii_doc1_${Date.now()}.txt`);
    const sampleDoc2Path = path.join(tempDir, `sample_pii_doc2_${Date.now()}.md`);

    const piiName = "홍길동";
    const piiRrn = "900101-1234567";
    const piiPhone = "010-5555-6666";
    const piiAddress = "서울특별시 종로구 세종대로 209";

    fs.writeFileSync(
      sampleDoc1Path,
      `[공문 표준 기안서]
기안자: ${piiName}
주민번호: ${piiRrn}
시행일자: 2026-11-05
표준 양식에 따라 작성된 시범 공문서입니다.`,
      "utf-8"
    );

    fs.writeFileSync(
      sampleDoc2Path,
      `# 비상연락망 및 사무소 현황
연락처: ${piiPhone}
소재지 주소: ${piiAddress} 정부서울청사
어투는 간결한 개조식을 적용합니다.`,
      "utf-8"
    );

    // 파일 투입
    await page
      .locator("#memory-create-files-input")
      .setInputFiles([sampleDoc1Path, sampleDoc2Path]);
    await page.waitForTimeout(300);

    // 이름 입력
    await page.locator("#memory-create-name-input").fill("교육청 공문 표준");

    // Mock LLM 응답 설정: 고의로 에코/환각 PII 포함
    mockServer.setNextMemoryResponse({
      id: "chatcmpl-mock-gen-echo",
      object: "chat.completion",
      choices: [
        {
          index: 0,
          message: {
            role: "assistant",
            content: `## 문서 유형·양식
- 교육청 공문서 표준

## 어투·문체
- 간결한 개조식 문체

## 용어·표기
- 교육부 표준 행정용어

## 서식·표 규칙
- 표 테두리 실선 0.2mm

## 구조(제목 체계)
- 1. -> 가. -> (1)

## 금지·지양
- 비표준 축약어 지양

## 예시 문구(일반화됨)
작성자: 김철수 (010-7777-8888)`,
          },
          finish_reason: "stop",
        },
      ],
    });

    mockServer.clearRequests();
    await page.locator("#memory-run-generate-btn").click();

    // 미리보기 패널 확인
    await page.waitForSelector("#memory-preview-panel", { timeout: 15000 });
    check("2.1 초안 생성 후 #memory-preview-panel 표시 확인", true);

    // (a) Mock LLM 서버 수신 요청 바디에서 원문 PII 0건 검증
    const outboundReqBodies = mockServer.capturedRequests.map((r) => r.rawBody);
    check(
      "2.2 Mock LLM 요청 수신 확인",
      outboundReqBodies.length >= 1,
      `requests=${outboundReqBodies.length}`
    );

    let outboundLeaked = false;
    for (const b of outboundReqBodies) {
      if (
        b.includes(piiName) ||
        b.includes(piiRrn) ||
        b.includes(piiPhone) ||
        b.includes("세종대로 209")
      ) {
        outboundLeaked = true;
      }
    }
    check(
      "2.3 (a) Mock LLM 요청 바디에 합성 PII 원문(홍길동/주민번호/연락처/주소) 미포함",
      !outboundLeaked
    );

    // (b) 미리보기 배너 노출 및 확정 저장 전 localStorage 미저장 검증
    const bannerVisible =
      (await page.locator("#memory-preview-pii-banner").count()) > 0;
    check("2.4 (b) #memory-preview-pii-banner 노출 확인", bannerVisible);

    const savedIndexBefore = await page.evaluate(() =>
      localStorage.getItem("hop_mem_index")
    );
    const notSavedYet =
      !savedIndexBefore || !savedIndexBefore.includes("교육청 공문 표준");
    check(
      "2.5 (b) [확인 및 저장] 클릭 전 localStorage 미저장 확인",
      notSavedYet
    );

    // (c) 미리보기 본문에서 김철수 및 010-7777-8888이 <이름>, <전화번호>로 치환되었는지 검증
    const previewBodyText = await page
      .locator("#memory-preview-body-textarea")
      .inputValue();
    const hasEchoName = previewBodyText.includes("김철수");
    const hasEchoPhone = previewBodyText.includes("010-7777-8888");
    const hasPlaceholders =
      previewBodyText.includes("<이름>") && previewBodyText.includes("<전화번호>");
    check(
      "2.6 (c) 모델 응답 내 에코 PII 차단 및 플레이스홀더 치환 확인",
      !hasEchoName && !hasEchoPhone && hasPlaceholders,
      `previewBody=${previewBodyText.slice(previewBodyText.indexOf("작성자"))}`
    );

    // [확인 및 저장] 클릭
    await page.locator("#memory-confirm-save-btn").click();
    await page.waitForTimeout(600);

    const savedIndexAfter = await page.evaluate(() =>
      localStorage.getItem("hop_mem_index")
    );
    const isSaved =
      Boolean(savedIndexAfter) && savedIndexAfter.includes("교육청 공문 표준");
    check("2.7 [확인 및 저장] 클릭 후 목록에 '교육청 공문 표준' 저장 확인", isSaved);

    // 모달 닫기
    await closeModal(page);

    // AI 채팅창 선호사항 기본값 "작성 선호사항: 꺼짐" 확인
    const memoryBtnText = await page
      .locator("#sidebar-memory-selector-btn")
      .innerText();
    check(
      "2.8 AI 채팅창 선호사항 기본값 '작성 선호사항: 꺼짐' 확인",
      memoryBtnText.includes("작성 선호사항: 꺼짐"),
      `actualText=${memoryBtnText}`
    );

    // ─────────────────────────────────────────────────────────────────────────
    // 시나리오 3: 열린 문서 트리거(#sidebar-create-memory-from-doc-btn) 및 독립 메모리 생성 검증
    // ─────────────────────────────────────────────────────────────────────────
    console.log(
      "\n--- 시나리오 3: 열린 문서 트리거 및 독립 메모리 생성 검증 ---"
    );

    const sampleHwpxPath = path.resolve(
      rootDir,
      "samples/demo_official_report.hwpx"
    );
    await page
      .locator("#header-hwp-file-input, #file-input")
      .first()
      .setInputFiles(sampleHwpxPath);
    await page.waitForSelector('[data-testid="header-doc-name"]', {
      timeout: 30000,
    });
    await page.waitForTimeout(1000);
    check("3.1 samples/demo_official_report.hwpx 문서 열기 완료", true);

    const openDocTriggerBtn = page.locator(
      "#sidebar-create-memory-from-doc-btn"
    );
    await openDocTriggerBtn.click();
    await page.waitForSelector("#writing-memory-modal", { timeout: 5000 });

    const currentDocRunBtn = page.locator("#memory-run-current-generate-btn");
    const isCurrentDocTab = (await currentDocRunBtn.count()) > 0;
    check(
      "3.2 #sidebar-create-memory-from-doc-btn 클릭 시 '현재 열린 문서로 만들기' 탭 열림",
      isCurrentDocTab
    );

    // 2번째 독립 메모리 생성 Mock LLM 응답
    mockServer.setNextMemoryResponse({
      id: "chatcmpl-mock-gen-doc2",
      object: "chat.completion",
      choices: [
        {
          index: 0,
          message: {
            role: "assistant",
            content: `## 문서 유형·양식
- 공문서 보고서 표준 양식

## 어투·문체
- 공문서 보고서 개조식 어투

## 용어·표기
- 행정 보고 표준 용어

## 서식·표 규칙
- 표 테두리 실선 서식

## 구조(제목 체계)
- I. -> 1. -> 가.

## 금지·지양
- 주관적 서술 지양

## 예시 문구(일반화됨)
- 추진 계획 보고 예시`,
          },
          finish_reason: "stop",
        },
      ],
    });

    await currentDocRunBtn.click();
    await page.waitForSelector("#memory-preview-panel", { timeout: 15000 });
    await page.locator("#memory-confirm-save-btn").click();
    await page.waitForTimeout(600);

    // 2개의 독립 메모리 저장 확인
    const indexJson = await page.evaluate(() =>
      JSON.parse(localStorage.getItem("hop_mem_index") || "[]")
    );
    check(
      "3.3 기존 메모리와 합쳐지지 않고 별도 2번째 독립 메모리 저장 확인",
      indexJson.length === 2,
      `count=${indexJson.length}, names=${indexJson.map((m) => m.name).join(", ")}`
    );

    // ─────────────────────────────────────────────────────────────────────────
    // 시나리오 4: AI로 다듬기(섹션별 부분 수락/거절) + 리비전 롤백 검증
    // ─────────────────────────────────────────────────────────────────────────
    console.log(
      "\n--- 시나리오 4: AI로 다듬기(섹션별 부분 수락/거절) + 리비전 롤백 검증 ---"
    );

    // '교육청 공문 표준' 카드 선택
    const memoryItems = page.locator('[data-testid^="memory-item-"]');
    const itemCount = await memoryItems.count();
    let targetItemId = "";
    for (let i = 0; i < itemCount; i++) {
      const itemText = await memoryItems.nth(i).innerText();
      if (itemText.includes("교육청 공문 표준")) {
        await memoryItems.nth(i).click();
        const testId = await memoryItems.nth(i).getAttribute("data-testid");
        targetItemId = testId.replace("memory-item-", "");
        break;
      }
    }
    check(
      "4.1 '교육청 공문 표준' 메모리 선택",
      Boolean(targetItemId),
      `id=${targetItemId}`
    );

    // [AI로 다듬기] 서브뷰 클릭
    await page.locator("#memory-subview-refine-btn").click();
    await page.waitForTimeout(300);

    // 지시사항 입력
    await page
      .locator("#memory-refine-instruction-input")
      .fill("어투를 더 간결하게 변경");

    // Mock LLM 응답 설정: ## 어투·문체 와 ## 금지·지양 두 섹션만 수정
    mockServer.setNextMemoryResponse({
      id: "chatcmpl-mock-refine-proposal",
      object: "chat.completion",
      choices: [
        {
          index: 0,
          message: {
            role: "assistant",
            content: `## 문서 유형·양식
- 교육청 공문서 표준

## 어투·문체
- 대폭 간결화된 초개조식 문체 반영

## 용어·표기
- 교육부 표준 행정용어

## 서식·표 규칙
- 표 테두리 실선 0.2mm

## 구조(제목 체계)
- 1. -> 가. -> (1)

## 금지·지양
- 절대 금지: 과도한 수식어 사용

## 예시 문구(일반화됨)
작성자: <이름> (<전화번호>)`,
          },
          finish_reason: "stop",
        },
      ],
    });

    await page.locator("#memory-propose-refine-btn").click();
    await page.waitForSelector('[data-testid="refine-toggle-section-5"]', {
      timeout: 15000,
    });
    check("4.2 MemoryDiffViewer diff 생성 및 토글 버튼 노출 확인", true);

    // data-testid="refine-toggle-section-5" (금지·지양) 클릭하여 수락 해제 (거절 상태로 전환)
    await page.locator('[data-testid="refine-toggle-section-5"]').click();
    await page.waitForTimeout(300);

    // [선택한 섹션만 반영하여 저장] 클릭
    await page.locator("#memory-apply-refine-btn").click();
    await page.waitForTimeout(600);

    // 직접 편집 탭으로 돌아와 반영 결과 검증
    await page.locator("#memory-subview-edit-btn").click();
    await page.waitForTimeout(300);

    const section1Text = await page
      .locator('[data-testid="memory-section-textarea-1"]')
      .inputValue();
    const section5Text = await page
      .locator('[data-testid="memory-section-textarea-5"]')
      .inputValue();

    const section1Updated = section1Text.includes(
      "대폭 간결화된 초개조식 문체 반영"
    );
    const section5Kept =
      section5Text.includes("비표준 축약어 지양") &&
      !section5Text.includes("과도한 수식어 사용");

    check("4.3 어투·문체 섹션 반영 확인", section1Updated);
    check("4.4 금지·지양 섹션 기존 내용 유지(거절) 확인", section5Kept);

    // 리비전 v2 확인
    const currentDocMeta = await page.evaluate(
      (id) => {
        const list = JSON.parse(localStorage.getItem("hop_mem_index") || "[]");
        return list.find((m) => m.id === id);
      },
      targetItemId
    );
    check(
      "4.5 리비전 v2 증가 확인",
      currentDocMeta?.revision === 2,
      `revision=${currentDocMeta?.revision}`
    );

    // 리비전 롤백 검증: #memory-subview-revisions-btn 클릭
    await page.locator("#memory-subview-revisions-btn").click();
    await page.waitForSelector('[data-testid="memory-rollback-rev-1"]', {
      timeout: 5000,
    });
    check("4.6 리비전 목록에서 rev.1 노출 확인", true);

    // rev.1로 롤백 클릭
    await page.locator('[data-testid="memory-rollback-rev-1"]').click();
    await page.waitForTimeout(600);

    // 롤백 후 직접 편집 탭에서 v1 본문 복원 및 v3 리비전 확인
    await page.locator("#memory-subview-edit-btn").click();
    await page.waitForTimeout(300);

    const rolledSection1Text = await page
      .locator('[data-testid="memory-section-textarea-1"]')
      .inputValue();
    const isRolledBackToV1 = rolledSection1Text.includes("간결한 개조식 문체");

    const rolledMeta = await page.evaluate(
      (id) => {
        const list = JSON.parse(localStorage.getItem("hop_mem_index") || "[]");
        return list.find((m) => m.id === id);
      },
      targetItemId
    );

    check("4.7 본문 v1 내용 복원 확인", isRolledBackToV1);
    check(
      "4.8 롤백 후 새 리비전 v3 생성 확인",
      rolledMeta?.revision === 3,
      `revision=${rolledMeta?.revision}`
    );

    // ─────────────────────────────────────────────────────────────────────────
    // 시나리오 5: AI 채팅창 선택적 메모리 활성화 및 수정 요청 검증
    // ─────────────────────────────────────────────────────────────────────────
    console.log(
      "\n--- 시나리오 5: AI 채팅창 선택적 메모리 활성화 및 수정 요청 검증 ---"
    );

    // 모달 닫기
    await closeModal(page);

    // 메모리가 모두 꺼진 상태에서 AI 채팅창에 "문단을 다듬어줘" 전송
    mockServer.clearRequests();
    const chatInput = page.locator('[data-testid="ai-chat-input"]');
    const chatSendBtn = page.locator('[data-testid="ai-chat-send-btn"]');

    await chatInput.fill("문단을 다듬어줘");
    await chatSendBtn.click();

    await waitFor(
      async () => {
        const text = await page.locator("#ai-chat-sidebar").innerText();
        return (
          text.includes("제안") ||
          text.includes("수정") ||
          text.includes("반영 문단")
        );
      },
      30000,
      "Chat response 1"
    );

    // Mock LLM 수신 바디(user 메시지)에 <WRITING_MEMORY_DATA가 없는지 검증
    const reqs1 = mockServer.capturedRequests;
    let memoryDataInUserMsg1 = false;
    for (const r of reqs1) {
      const userMsgs = (r.jsonBody.messages || []).filter(
        (m) => m.role === "user"
      );
      for (const m of userMsgs) {
        const contentStr =
          typeof m.content === "string"
            ? m.content
            : JSON.stringify(m.content || "");
        if (contentStr.includes("<WRITING_MEMORY_DATA")) {
          memoryDataInUserMsg1 = true;
        }
      }
      if (r.rawBody.includes("<WRITING_MEMORY_DATA priority=")) {
        memoryDataInUserMsg1 = true;
      }
    }
    check(
      "5.1 메모리 미선택 상태에서 요청 바디에 <WRITING_MEMORY_DATA 미포함 검증",
      !memoryDataInUserMsg1
    );

    // #sidebar-memory-selector-btn 클릭 -> popover 열기
    await page.locator("#sidebar-memory-selector-btn").click();
    await page.waitForSelector("#sidebar-memory-popover", { timeout: 5000 });

    // '교육청 공문 표준' 체크박스 클릭
    const memCheckbox = page.locator(
      `[data-testid="memory-check-${targetItemId}"]`
    );
    await memCheckbox.check();
    await page.waitForTimeout(300);

    // 버튼 텍스트가 "작성 선호사항: 1개 적용 중"으로 바뀌었는지 확인
    const updatedBtnText = await page
      .locator("#sidebar-memory-selector-btn")
      .innerText();
    check(
      "5.2 선호사항 1개 선택 후 버튼 텍스트 '작성 선호사항: 1개 적용 중' 확인",
      updatedBtnText.includes("작성 선호사항: 1개 적용 중"),
      `actual=${updatedBtnText}`
    );

    // popover 닫기
    await page.locator("#sidebar-memory-selector-btn").click();
    await page.waitForTimeout(300);

    // AI 채팅창에 "선택한 작성 선호사항에 맞춰 첫 문단을 수정해줘" 전송
    mockServer.clearRequests();
    await chatInput.fill("선택한 작성 선호사항에 맞춰 첫 문단을 수정해줘");
    await chatSendBtn.click();

    await waitFor(
      async () => {
        const text = await page.locator("#ai-chat-sidebar").innerText();
        return text.includes("첫 문단 선호사항 적용") || text.includes("제안");
      },
      30000,
      "Chat response 2 with memory"
    );

    await page.screenshot({
      path: path.join(shotDir, "02_writing_memory_diff_card.png"),
    });

    // Mock LLM 수신 바디 검증:
    // (a) <WRITING_MEMORY_DATA priority="1" name="교육청 공문 표준"> 포함
    // (b) 체크하지 않은 2번째 메모리는 미포함
    // (c) 원문 PII 0건
    const reqs2 = mockServer.capturedRequests;
    check(
      "5.3 선호사항 적용 후 Mock LLM 요청 수신 (>=1회)",
      reqs2.length >= 1,
      `count=${reqs2.length}`
    );

    let hasSelectedMemoryData = false;
    let hasUnselectedMemoryData = false;
    let leakedPiiInPrompt = false;

    for (const r of reqs2) {
      const userMsgs = (r.jsonBody.messages || []).filter(
        (m) => m.role === "user"
      );
      for (const m of userMsgs) {
        const c =
          typeof m.content === "string"
            ? m.content
            : JSON.stringify(m.content || "");
        if (
          c.includes("<WRITING_MEMORY_DATA") &&
          c.includes('name="교육청 공문 표준"')
        ) {
          hasSelectedMemoryData = true;
        }
        if (c.includes("demo_official_report 양식")) {
          hasUnselectedMemoryData = true;
        }
        if (
          c.includes(piiName) ||
          c.includes(piiRrn) ||
          c.includes(piiPhone) ||
          c.includes("세종대로 209")
        ) {
          leakedPiiInPrompt = true;
        }
      }
      // Also check rawBody for unescaped/escaped variants
      if (
        r.rawBody.includes("<WRITING_MEMORY_DATA") &&
        (r.rawBody.includes('name="교육청 공문 표준"') ||
          r.rawBody.includes('name=\\"교육청 공문 표준\\"'))
      ) {
        hasSelectedMemoryData = true;
      }
    }

    check(
      "5.4 (a) 선택한 '교육청 공문 표준' <WRITING_MEMORY_DATA> 포함 검증",
      hasSelectedMemoryData
    );
    check(
      "5.5 (b) 체크하지 않은 2번째 메모리 미포함 검증",
      !hasUnselectedMemoryData
    );
    check(
      "5.6 (c) 요청 프롬프트 내 원문 PII 0건 검증",
      !leakedPiiInPrompt
    );

    // Diff 제안 카드가 정상 렌더링되었는지 확인
    const diffCardVisible =
      (await page.locator("#ai-chat-sidebar").innerText()).includes(
        "첫 문단 선호사항 적용"
      ) ||
      (await page.locator("#ai-chat-sidebar").innerText()).includes(
        "개정 공문서 표준 반영 문단"
      );
    check("5.7 (d) 채팅창 Diff 제안 카드 렌더링 확인", diffCardVisible);

    // Clean up temporary files
    try {
      fs.unlinkSync(sampleDoc1Path);
      fs.unlinkSync(sampleDoc2Path);
    } catch {}
  } catch (err) {
    console.error("Fatal Error during E2E Run:", err);
    check("E2E Run Execution", false, err?.message || String(err));
  } finally {
    if (browser) await browser.close();
    if (viteServer) await viteServer.close();
    mockServer.server.close();
  }

  console.log(
    "\n==============================================================="
  );
  console.log("  E2E Test Results Summary");
  console.log("===============================================================");
  const passedCount = results.filter((r) => r.pass).length;
  const failedCount = results.filter((r) => !r.pass).length;
  console.log(
    `Total: ${results.length} | Passed: ${passedCount} | Failed: ${failedCount}`
  );

  if (failedCount > 0) {
    console.log("\nFailed Checks:");
    for (const r of results.filter((r) => !r.pass)) {
      console.log(` - ${r.name}: ${r.detail}`);
    }
    process.exit(1);
  } else {
    console.log("\nAll checks passed successfully!");
    process.exit(0);
  }
}

run();
