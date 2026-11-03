import { copyFileSync, existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { basename, extname, isAbsolute, join, relative, resolve } from "node:path";
import { spawn, execFileSync } from "node:child_process";
import { randomUUID } from "node:crypto";
import { defineConfig, type Plugin } from "vite";
import react from "@vitejs/plugin-react";
import { createHopOverrides } from "./hop-overrides";
import { isValidHwpxPackage } from "./src/hop-host/core/hwpx-package";
import { copyFileAtomically } from "./src/hop-host/core/atomic-file";
import { resolveFontAssetPath } from "./src/hop-host/core/font-asset-path";

const rootDir = __dirname;
const hopSrc = resolve(rootDir, "src/hop-host");
const upstreamSrc = resolve(rootDir, "third_party/rhwp/rhwp-studio/src");
const wasmDir = resolve(rootDir, "vendor/rhwp-core");
const fontsDir = resolve(rootDir, "assets/fonts");
const stagingDir = resolve(rootDir, ".hop-staging");
const clawScriptsDir = resolve(rootDir, "resources/claw-hwp/scripts");

function collectFontFiles(dir: string): string[] {
  if (!existsSync(dir)) return [];
  const entries = readdirSync(dir, { withFileTypes: true });
  const files: string[] = [];
  for (const entry of entries) {
    const fullPath = join(dir, entry.name);
    if (entry.isDirectory()) {
      files.push(...collectFontFiles(fullPath));
    } else if (entry.isFile() && extname(entry.name).toLowerCase() === ".woff2") {
      files.push(fullPath);
    }
  }
  return files;
}

function hopFontAssets(): Plugin {
  return {
    name: "hop-font-assets",
    configureServer(server) {
      server.middlewares.use("/fonts", (req, res, next) => {
        const candidate = resolveFontAssetPath(fontsDir, req.url ?? "/");
        if (!candidate) {
          next();
          return;
        }
        res.setHeader("Content-Type", "font/woff2");
        res.end(readFileSync(candidate));
      });
    },
    writeBundle(options) {
      const outDir = options.dir ? resolve(rootDir, options.dir) : resolve(rootDir, "dist");
      for (const source of collectFontFiles(fontsDir)) {
        const target = resolve(outDir, "fonts", relative(fontsDir, source));
        mkdirSync(resolve(target, ".."), { recursive: true });
        copyFileSync(source, target);
      }
    },
  };
}

function resolveLocalPath(p: string): string {
  if (!p) return "";
  return isAbsolute(p) ? p : resolve(rootDir, p);
}

function runClawStdin(scriptName: string, stdinJson: string): Promise<{ ok: boolean; stdout_json: any; stderr: string }> {
  return new Promise((resolvePromise, rejectPromise) => {
    const scriptPath = resolve(clawScriptsDir, scriptName);
    const child = spawn("node", [scriptPath], { stdio: ["pipe", "pipe", "pipe"] });
    const outChunks: Buffer[] = [];
    const errChunks: Buffer[] = [];
    child.stdout.on("data", (d) => outChunks.push(Buffer.from(d)));
    child.stderr.on("data", (d) => errChunks.push(Buffer.from(d)));
    child.on("error", (err) => rejectPromise(err));
    child.on("close", (code) => {
      const stdoutStr = Buffer.concat(outChunks).toString("utf8").trim();
      const stderrStr = Buffer.concat(errChunks).toString("utf8");
      let parsed: any;
      try {
        parsed = JSON.parse(stdoutStr);
      } catch {
        parsed = { status: "error", raw_stdout: stdoutStr, stderr: stderrStr };
      }
      const ok = code === 0 && (parsed?.status === "success" || parsed?.ok === true);
      resolvePromise({ ok, stdout_json: parsed, stderr: stderrStr });
    });
    child.stdin.write(stdinJson, "utf8");
    child.stdin.end();
  });
}

const activeHttpRequests = new Map<string, AbortController>();

function localClawBridgePlugin(): Plugin {
  return {
    name: "hop-local-claw-bridge",
    configureServer(server) {
      server.middlewares.use("/__hop_claw_api/", async (req, res) => {
        const cmd = decodeURIComponent((req.url ?? "").split("?")[0].replace(/^\/+/, ""));
        const chunks: Buffer[] = [];
        req.on("data", (c) => chunks.push(Buffer.from(c)));
        req.on("end", async () => {
          res.setHeader("Content-Type", "application/json; charset=utf-8");
          try {
            const bodyStr = Buffer.concat(chunks).toString("utf8");
            const args = bodyStr ? JSON.parse(bodyStr) : {};
            mkdirSync(stagingDir, { recursive: true });

            let result: any = null;

            switch (cmd) {
              case "open_document_session": {
                const srcPath = resolveLocalPath(args.filePath || args.file_path);
                if (!existsSync(srcPath)) {
                  throw new Error(`파일을 찾을 수 없습니다: ${srcPath}`);
                }
                const bytes = readFileSync(srcPath);
                const hasZipSignature = bytes.length >= 4
                  && bytes[0] === 0x50 && bytes[1] === 0x4b
                  && bytes[2] === 0x03 && bytes[3] === 0x04;
                const requestedHwpx = extname(srcPath).toLowerCase() === ".hwpx";
                const isHwpx = hasZipSignature
                  ? isValidHwpxPackage(new Uint8Array(bytes))
                  : false;
                if (hasZipSignature && !isHwpx) {
                  throw new Error("유효한 HWPX(OWPML) 패키지가 아닙니다. 일반 ZIP 또는 손상된 문서는 열 수 없습니다.");
                }
                if (requestedHwpx && !isHwpx) {
                  throw new Error("HWPX 파일의 ZIP/OWPML 패키지 구조가 손상되었거나 올바르지 않습니다.");
                }
                const ext = isHwpx ? "hwpx" : "hwp";
                const stagingPath = resolve(stagingDir, `staging_${randomUUID()}.${ext}`);
                writeFileSync(stagingPath, bytes);
                result = {
                  original_path: srcPath,
                  staging_path: stagingPath,
                  file_name: basename(srcPath),
                  is_hwpx: isHwpx,
                  bytes: Array.from(bytes),
                };
                break;
              }

              case "create_blank_session": {
                const isHwpx = Boolean(args.isHwpx ?? args.is_hwpx);
                const ext = isHwpx ? "hwpx" : "hwp";
                const stagingPath = resolve(stagingDir, `new_${randomUUID()}.${ext}`);
                result = {
                  original_path: "",
                  staging_path: stagingPath,
                  file_name: `새 문서.${ext}`,
                  is_hwpx: isHwpx,
                  bytes: [],
                };
                break;
              }

              case "read_file_bytes":
              case "read_attachment_bytes": {
                const target = resolveLocalPath(args.filePath || args.file_path);
                const bytes = readFileSync(target);
                result = Array.from(bytes);
                break;
              }

              case "write_file_bytes": {
                const target = resolveLocalPath(args.filePath || args.file_path);
                writeFileSync(target, Buffer.from(args.bytes || []));
                result = null;
                break;
              }

              case "commit_staging_to_original": {
                const stg = resolveLocalPath(args.stagingPath || args.staging_path);
                const orig = args.originalPath || args.original_path;
                const target = orig && orig.trim() ? resolveLocalPath(orig) : resolve(stagingDir, `saved_${basename(stg)}`);
                copyFileAtomically(stg, target);
                result = target;
                break;
              }

              case "get_startup_file": {
                const defaultLessonSample = resolve(rootDir, "samples/(날짜)(교시)(과목명)교수.학습과정안(이름).hwp");
                const defaultOfficialSample = resolve(rootDir, "samples/demo_official_report.hwp");
                if (existsSync(defaultLessonSample)) {
                  result = defaultLessonSample;
                } else if (existsSync(defaultOfficialSample)) {
                  result = defaultOfficialSample;
                } else {
                  result = null;
                }
                break;
              }

              case "inspect_document": {
                const target = resolveLocalPath(args.filePath || args.file_path);
                const script = resolve(clawScriptsDir, "extract_text.js");
                const stdout = execFileSync("node", [script, "--inspect", "--with-cell-text", target], {
                  encoding: "utf8",
                  maxBuffer: 20 * 1024 * 1024,
                });
                result = JSON.parse(stdout);
                break;
              }

              case "extract_markdown": {
                const target = resolveLocalPath(args.filePath || args.file_path);
                const script = resolve(clawScriptsDir, "extract_text.js");
                const stdout = execFileSync("node", [script, "--format", "markdown", target], {
                  encoding: "utf8",
                  maxBuffer: 20 * 1024 * 1024,
                });
                result = stdout;
                break;
              }

              case "execute_claw_ops": {
                const isHwpx = Boolean(args.isHwpx ?? args.is_hwpx);
                const payloadJson = String(args.payloadJson ?? args.payload_json ?? "{}");
                const scriptName = isHwpx ? "hwpx-edit.js" : "create.js";
                result = await runClawStdin(scriptName, payloadJson);
                break;
              }

              case "call_llm_http": {
                const reqObj = args.req || args;
                const reqId = reqObj.request_id || reqObj.requestId;
                const controller = new AbortController();
                if (reqId) {
                  activeHttpRequests.set(reqId, controller);
                }
                const timeoutMs = reqObj.timeout_ms || reqObj.timeoutMs || 180000;
                const timer = setTimeout(() => controller.abort(), timeoutMs);

                try {
                  const resp = await fetch(reqObj.url, {
                    method: "POST",
                    headers: reqObj.headers || { "Content-Type": "application/json" },
                    body: JSON.stringify(reqObj.body_json),
                    signal: controller.signal,
                  });
                  clearTimeout(timer);
                  const status = resp.status;
                  const text = await resp.text();
                  let parsedBody: any;
                  try {
                    parsedBody = JSON.parse(text);
                  } catch {
                    parsedBody = { raw_text: text };
                  }
                  result = { status, body: parsedBody };
                } catch (fetchErr: any) {
                  clearTimeout(timer);
                  if (fetchErr.name === "AbortError" || controller.signal.aborted) {
                    throw new Error("LLM 요청이 사용자에 의해 취소되었거나 타임아웃되었습니다.");
                  }
                  throw fetchErr;
                } finally {
                  if (reqId) {
                    activeHttpRequests.delete(reqId);
                  }
                }
                break;
              }

              case "cancel_llm_request": {
                const reqId = args.request_id || args.requestId;
                if (reqId && activeHttpRequests.has(reqId)) {
                  activeHttpRequests.get(reqId)!.abort();
                  activeHttpRequests.delete(reqId);
                  result = true;
                } else {
                  result = false;
                }
                break;
              }

              default:
                throw new Error(`Unknown command: ${cmd}`);
            }

            res.statusCode = 200;
            res.end(JSON.stringify({ result }));
          } catch (err: any) {
            res.statusCode = 200;
            res.end(JSON.stringify({ __hop_error: err?.message || String(err) }));
          }
        });
      });
    },
  };
}

export default defineConfig({
  plugins: [react(), hopFontAssets(), localClawBridgePlugin()],
  clearScreen: false,
  resolve: {
    alias: [
      ...createHopOverrides(hopSrc),
      {
        find: "@wasm/rhwp.js",
        replacement: resolve(wasmDir, "rhwp.js"),
      },
      {
        find: "@/upstream",
        replacement: resolve(hopSrc, "upstream"),
      },
      {
        find: "@upstream",
        replacement: upstreamSrc,
      },
      {
        find: "@",
        replacement: upstreamSrc,
      },
    ],
  },
  server: {
    port: 1420,
    strictPort: true,
    fs: {
      allow: [rootDir, hopSrc, upstreamSrc, wasmDir, fontsDir],
    },
    watch: {
      ignored: ["**/src-tauri/**", "**/.hop-staging/**"],
    },
  },
  optimizeDeps: {
    exclude: ["@wasm/rhwp.js"],
  },
});
