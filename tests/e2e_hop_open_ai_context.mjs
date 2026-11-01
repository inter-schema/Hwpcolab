// HOP 자체 UI로 연 문서를 AI가 실제로 읽는지 검증하는 E2E.
// 사용법:
//   node tests/e2e_hop_open_ai_context.mjs --target=web   (Vite dev 서버 + Chromium)
//   node tests/e2e_hop_open_ai_context.mjs --target=exe   (release/HOP-Claw.exe 를 WebView2 CDP로 제어)
import { chromium } from "playwright";
import { spawn, execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readdirSync, statSync, writeFileSync } from "node:fs";
import { resolve, join } from "node:path";

const target = (process.argv.find((a) => a.startsWith("--target=")) || "--target=web").split("=")[1];
const rootDir = process.cwd();
const shotDir = resolve(rootDir, `tests/screenshots/ctx_${target}`);
mkdirSync(shotDir, { recursive: true });
const samplePath = resolve(rootDir, "samples/(날짜)(교시)(과목명)교수.학습과정안(이름).hwp");
const extractScript = resolve(rootDir, "resources/claw-hwp/scripts/extract_text.js");

const results = [];
function check(name, pass, detail = "") {
  results.push({ name, pass: Boolean(pass), detail: String(detail).slice(0, 400) });
  console.log(`${pass ? "PASS" : "FAIL"} | ${name} | ${String(detail).slice(0, 200)}`);
}

async function waitFor(fn, timeoutMs, label) {
  const t0 = Date.now();
  while (Date.now() - t0 < timeoutMs) {
    try {
      const v = await fn();
      if (v) return v;
    } catch {}
    await new Promise((r) => setTimeout(r, 400));
  }
  throw new Error(`timeout: ${label}`);
}

function latestStagingFile() {
  const dirs =
    target === "exe"
      ? [join(process.env.LOCALAPPDATA || "", "com.hopclaw.desktop", "hop-staging")]
      : [resolve(rootDir, ".hop-staging")];
  let best = null;
  for (const d of dirs) {
    if (!existsSync(d)) continue;
    for (const f of readdirSync(d)) {
      if (!/\.(hwp|hwpx)$/i.test(f)) continue;
      const p = join(d, f);
      const m = statSync(p).mtimeMs;
      if (!best || m > best.m) best = { p, m };
    }
  }
  return best?.p || null;
}

function extractAll(p) {
  return execFileSync("node", [extractScript, "--inspect", "--with-cell-text", p], {
    encoding: "utf8",
    maxBuffer: 20 * 1024 * 1024,
  });
}

async function askAi(page, prompt) {
  const log = page.locator('[data-testid="agent-exec-log-summary"]');
  const before = await log.count();
  const sidebarBefore = await page.locator("#ai-chat-sidebar").innerText();
  await page.locator('[data-testid="ai-chat-input"]').fill(prompt);
  await page.locator('[data-testid="ai-chat-send-btn"]').click();
  await waitFor(
    async () => {
      if ((await log.count()) > before) return true;
      const t = await page.locator("#ai-chat-sidebar").innerText();
      return t.includes("오류가 발생했습니다") && t !== sidebarBefore;
    },
    90000,
    `AI answer for: ${prompt}`
  );
  await page.waitForTimeout(800);
  const after = await page.locator("#ai-chat-sidebar").innerText();
  return after.replace(sidebarBefore.slice(0, 0), "");
}

async function main() {
  let devServer = null;
  let exeProc = null;
  let browser;
  let page;

  if (target === "web") {
    let up = false;
    try {
      up = (await fetch("http://localhost:1420")).ok;
    } catch {}
    if (!up) {
      devServer = spawn("npx.cmd", ["vite", "--port", "1420"], { cwd: rootDir, shell: true, stdio: "pipe" });
      await waitFor(async () => (await fetch("http://localhost:1420")).ok, 40000, "vite up");
    }
    browser = await chromium.launch({ headless: true });
    const ctx = await browser.newContext({ viewport: { width: 1560, height: 920 } });
    page = await ctx.newPage();
    await page.goto("http://localhost:1420", { waitUntil: "networkidle" });
  } else {
    const exePath = resolve(rootDir, "release/HOP-Claw.exe");
    if (!existsSync(exePath)) throw new Error("release/HOP-Claw.exe 없음");
    exeProc = spawn(exePath, [], {
      cwd: resolve(rootDir, "release"),
      env: { ...process.env, WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: "--remote-debugging-port=9333" },
      stdio: "ignore",
    });
    await waitFor(async () => (await fetch("http://127.0.0.1:9333/json/version")).ok, 40000, "WebView2 CDP up");
    browser = await chromium.connectOverCDP("http://127.0.0.1:9333");
    const ctx = browser.contexts()[0];
    page = ctx.pages()[0] || (await ctx.waitForEvent("page"));
    await page.setViewportSize({ width: 1560, height: 920 }).catch(() => {});
  }

  const llmBodies = [];
  if (target === "web") {
    await page.route("**/__hop_claw_api/call_llm_http", async (route) => {
      llmBodies.push(route.request().postData() || "");
      await route.continue();
    });
  }
  const consoleErrors = [];
  page.on("console", (m) => m.type() === "error" && consoleErrors.push(m.text()));

  await page.waitForSelector("#studio-root #menu-bar .menu-item", { timeout: 30000 });

  // ── 시나리오 1: HOP 자체 '열기' 경로(#file-input)로 문서를 연다 (스토어 경유 X) ──
  // web 모드는 기동 시 샘플 자동 로드가 있으므로, 먼저 빈 문서가 아닌 다른 문서(demo)를 HOP으로 열었다가 샘플을 다시 연다.
  const demo = resolve(rootDir, "samples/demo_official_report.hwp");
  await page.locator("#file-input").setInputFiles(demo);
  await page.waitForTimeout(2500);
  await page.locator("#file-input").setInputFiles(samplePath);
  await waitFor(
    async () => (await page.locator("#status-bar").innerText()).includes("교수.학습과정안"),
    30000,
    "HOP loaded sample via #file-input"
  );
  await page.waitForTimeout(3000);
  await page.screenshot({ path: join(shotDir, "01_opened_via_hop_menu.png") });
  check("S1 HOP #file-input 로 문서 열림(상태바에 파일명)", true);

  // 스테이징 파일이 만들어졌고 실제 표가 추출되는가 (AI 컨텍스트의 원천)
  const staging = await waitFor(() => latestStagingFile(), 15000, "staging file");
  const inspect = JSON.parse(extractAll(staging));
  check("S1 스테이징 파일에 표 1개 추출", inspect.tableCount === 1, `staging=${staging} tableCount=${inspect.tableCount}`);

  // ── 시나리오 2: 요약 질문 → 응답이 문서 내용을 근거로 해야 한다 ──
  const ans1 = await askAi(page, "현재 페이지의 내용을 요약해줘");
  await page.screenshot({ path: join(shotDir, "02_summary_answer.png") });
  const bad = ["표가 0개", "표 0개", "제공되지 않", "찾을 수 없", "문서가 비어"];
  const tokens = ["소수", "나눗셈", "서승우", "교수", "수학"];
  const hasBad = bad.filter((b) => ans1.includes(b));
  const hasTok = tokens.filter((t) => ans1.includes(t));
  check("S2 요약 응답에 '표 0개/제공되지 않음' 류 오답 없음", hasBad.length === 0, `bad=${hasBad.join(",")}`);
  check("S2 요약 응답에 문서 고유 내용(소수/나눗셈/서승우 등) 포함", hasTok.length >= 2, `tokens=${hasTok.join(",")}`);
  if (target === "web") {
    const last = llmBodies[llmBodies.length - 1] || "";
    check("S2 LLM 요청 본문에 '표 개수: 1' 포함", last.includes("표 개수: 1"), last.slice(0, 120));
    check("S2 LLM 요청 본문에 문서 텍스트(서승우) 포함", last.includes("서승우"));
  }

  // ── 시나리오 3: 구체적 편집 요청 → Smart Diff → 적용 → 실제 파일에 반영 ──
  await page.locator('[data-testid="mode-diff-review-btn"]').click();
  await askAi(page, "표에서 지도교사 이름을 '홍길동'으로 바꿔줘. 다른 곳은 건드리지 마.");
  await waitFor(async () => (await page.locator('[data-testid="smart-diff-card"]').count()) > 0, 20000, "smart diff card");
  await page.screenshot({ path: join(shotDir, "03_smart_diff_for_edit.png") });
  const cardText = await page.locator('[data-testid="smart-diff-card"]').first().innerText();
  check("S3 Smart Diff 카드에 홍길동 표시", cardText.includes("홍길동"), cardText.replace(/\n/g, " "));
  await page.locator('[data-testid="smart-diff-banner"] button', { hasText: "모두 적용" }).first().click();
  await page.waitForTimeout(3500);
  await page.screenshot({ path: join(shotDir, "04_after_accept.png") });
  const after = extractAll(latestStagingFile());
  check("S3 스테이징 .hwp 바이너리에 홍길동 반영", after.includes("홍길동"));
  check("S3 서승우 제거됨(덮어쓰기 확인)", !after.includes("서승우"));
  check("S3 표/셀 구조 유지(tableCount=1)", JSON.parse(after).tableCount === 1);
  check("S3 마크다운 별표(**) 잔재 없음", !after.includes("**"));
  const canvasVisible = await page.evaluate(() => {
    const c = document.querySelector("#scroll-container canvas");
    return Boolean(c && c.clientWidth > 300);
  });
  check("S3 HOP 캔버스 유지(재로드 후 렌더링)", canvasVisible);

  check("콘솔 치명 오류 없음(참고)", consoleErrors.filter((e) => /Uncaught|TypeError|ReferenceError/.test(e)).length === 0, consoleErrors.slice(0, 3).join(" | "));

  writeFileSync(join(shotDir, "result.json"), JSON.stringify({ target, results }, null, 2), "utf8");
  console.log(`\n=== ${results.filter((r) => r.pass).length}/${results.length} PASS (target=${target}) ===`);

  await browser.close().catch(() => {});
  devServer?.kill();
  if (exeProc) {
    try {
      execFileSync("taskkill", ["/PID", String(exeProc.pid), "/T", "/F"]);
    } catch {}
  }
  process.exit(results.every((r) => r.pass) ? 0 : 1);
}

main().catch((e) => {
  console.error("[E2E FATAL]", e);
  process.exit(2);
});
