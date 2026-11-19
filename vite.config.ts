import { defineConfig, type Plugin } from "vite";
import react from "@vitejs/plugin-react";
import { resolve, basename, extname, join } from "node:path";
import {
  existsSync,
  mkdirSync,
  readFileSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { randomUUID } from "node:crypto";
import { execFile, spawn } from "node:child_process";
import { promisify } from "node:util";
import { createHopOverrides } from "./hop-overrides";
import { handleFontAssetRequest } from "./src/hop-host/core/font-asset-middleware";
import { isValidHwpxPackage } from "./src/hop-host/core/hwpx-package";
import { copyFileAtomically } from "./src/hop-host/core/atomic-file";
import { ActiveHttpRequests } from "./src/hop-host/core/active-http-requests";
import {
  normalizeLlmTimeoutMs,
  readBoundedResponseText,
  serializeBoundedRequestBody,
  validateLlmEndpoint,
} from "./src/hop-host/core/llm-http-bridge";

const execFileAsync = promisify(execFile);

const ROOT_DIR = __dirname;
const HOP_HOST_DIR = resolve(ROOT_DIR, "src/hop-host");
const UPSTREAM_SRC_DIR = resolve(ROOT_DIR, "third_party/rhwp/rhwp-studio/src");
const WASM_DIR = resolve(ROOT_DIR, "public/vendor/rhwp");
const FONTS_DIR = resolve(ROOT_DIR, "assets/fonts");
const STAGING_DIR = resolve(ROOT_DIR, ".hop-staging");
const SCRIPTS_DIR = resolve(ROOT_DIR, "resources/claw-hwp/scripts");

const MAX_ATTACHMENT_BYTES = 128 * 1024 * 1024;
const activeHttpRequests = new ActiveHttpRequests();

function hasZipLocalFileHeader(bytes: Uint8Array): boolean {
  return (
    bytes.byteLength >= 4 &&
    bytes[0] === 0x50 &&
    bytes[1] === 0x4b &&
    bytes[2] === 0x03 &&
    bytes[3] === 0x04
  );
}

function runClawScriptWithStdin(
  scriptPath: string,
  payloadJson: string,
  timeoutMs = 30_000
): Promise<{ ok: boolean; stdout_json: unknown; stderr: string }> {
  return new Promise((resolvePromise, rejectPromise) => {
    const child = spawn(process.execPath, [scriptPath], {
      stdio: ["pipe", "pipe", "pipe"],
    });
    const stdoutChunks: Buffer[] = [];
    const stderrChunks: Buffer[] = [];

    const timer = setTimeout(() => {
      child.kill("SIGKILL");
      rejectPromise(
        new Error("claw 실행 타임아웃: 30초 내에 프로세스가 응답하지 않았습니다.")
      );
    }, timeoutMs);

    child.stdout.on("data", (chunk) => stdoutChunks.push(Buffer.from(chunk)));
    child.stderr.on("data", (chunk) => stderrChunks.push(Buffer.from(chunk)));
    child.on("error", (err) => {
      clearTimeout(timer);
      rejectPromise(err);
    });
    child.on("close", (code) => {
      clearTimeout(timer);
      const stdoutStr = Buffer.concat(stdoutChunks).toString("utf8").trim();
      const stderrStr = Buffer.concat(stderrChunks).toString("utf8");
      let parsed: any;
      try {
        parsed = JSON.parse(stdoutStr);
      } catch {
        parsed = { status: "error", raw_stdout: stdoutStr, stderr: stderrStr };
      }
      const ok =
        code === 0 &&
        (parsed?.status === "success" || parsed?.ok === true);
      resolvePromise({
        ok,
        stdout_json: parsed,
        stderr: stderrStr,
      });
    });

    child.stdin.write(payloadJson);
    child.stdin.end();
  });
}

let mockDefaultHwpAppEnabled = false;

async function handleBridgeCommand(
  cmd: string,
  args: Record<string, any>
): Promise<unknown> {
  mkdirSync(STAGING_DIR, { recursive: true });

  switch (cmd) {
    case "get_startup_file":
      return process.env.HOP_STARTUP_FILE || null;

    case "pick_local_file":
      return null;

    case "list_local_fonts":
      return [];

    case "read_local_font": {
      const p = resolve(ROOT_DIR, String(args.path || ""));
      return Array.from(readFileSync(p));
    }

    case "open_document_session": {
      const rawPath = String(args.filePath ?? args.file_path ?? "");
      const srcPath = resolve(ROOT_DIR, rawPath);
      if (!existsSync(srcPath)) {
        throw new Error(`파일 읽기 실패: ${rawPath}`);
      }
      const bytes = new Uint8Array(readFileSync(srcPath));
      const hasZip = hasZipLocalFileHeader(bytes);
      const requestedHwpx = extname(srcPath).toLowerCase() === ".hwpx";
      let isHwpx = false;
      if (hasZip) {
        if (!isValidHwpxPackage(bytes)) {
          throw new Error(
            "유효한 HWPX(OWPML) 패키지가 아닙니다. 일반 ZIP 또는 손상된 문서는 열 수 없습니다."
          );
        }
        isHwpx = true;
      } else {
        if (requestedHwpx) {
          throw new Error(
            "HWPX 파일의 ZIP/OWPML 패키지 구조가 손상되었거나 올바르지 않습니다."
          );
        }
        isHwpx = false;
      }
      const ext = isHwpx ? "hwpx" : "hwp";
      const stagingPath = join(STAGING_DIR, `staging_${randomUUID()}.${ext}`);
      writeFileSync(stagingPath, bytes);
      return {
        original_path: rawPath,
        staging_path: stagingPath,
        file_name: basename(srcPath) || `untitled.${ext}`,
        is_hwpx: isHwpx,
        bytes: Array.from(bytes),
      };
    }

    case "create_blank_session": {
      const isHwpx = Boolean(args.isHwpx ?? args.is_hwpx);
      const ext = isHwpx ? "hwpx" : "hwp";
      const stagingPath = join(STAGING_DIR, `new_${randomUUID()}.${ext}`);
      return {
        original_path: "",
        staging_path: stagingPath,
        file_name: `새 문서.${ext}`,
        is_hwpx: isHwpx,
        bytes: [],
      };
    }

    case "read_file_bytes": {
      const rawPath = String(args.filePath ?? args.file_path ?? "");
      const target = resolve(ROOT_DIR, rawPath);
      return Array.from(readFileSync(target));
    }

    case "write_file_bytes": {
      const rawPath = String(args.filePath ?? args.file_path ?? "");
      const target = resolve(ROOT_DIR, rawPath);
      const bytes = Uint8Array.from(args.bytes || []);
      writeFileSync(target, bytes);
      return null;
    }

    case "read_attachment_bytes": {
      const rawPath = String(args.filePath ?? args.file_path ?? "");
      const target = resolve(ROOT_DIR, rawPath);
      const st = statSync(target);
      if (!st.isFile()) {
        throw new Error(`첨부 경로가 일반 파일이 아닙니다: ${rawPath}`);
      }
      if (st.size > MAX_ATTACHMENT_BYTES) {
        throw new Error(
          `첨부 파일은 128 MiB 이하만 처리할 수 있습니다: ${rawPath}`
        );
      }
      return Array.from(readFileSync(target));
    }

    case "commit_staging_to_original": {
      const stagingPath = resolve(
        ROOT_DIR,
        String(args.stagingPath ?? args.staging_path ?? "")
      );
      const originalRaw = String(args.originalPath ?? args.original_path ?? "").trim();
      const targetPath = originalRaw
        ? resolve(ROOT_DIR, originalRaw)
        : join(
            STAGING_DIR,
            `saved_${randomUUID()}${extname(stagingPath) || ".hwp"}`
          );
      copyFileAtomically(stagingPath, targetPath);
      return targetPath;
    }

    case "inspect_document": {
      const rawPath = String(args.filePath ?? args.file_path ?? "");
      const target = resolve(ROOT_DIR, rawPath);
      const script = join(SCRIPTS_DIR, "extract_text.js");
      const { stdout } = await execFileAsync(
        process.execPath,
        [script, "--inspect", "--with-cell-text", target],
        { maxBuffer: 20 * 1024 * 1024 }
      );
      return JSON.parse(stdout);
    }

    case "extract_markdown": {
      const rawPath = String(args.filePath ?? args.file_path ?? "");
      const target = resolve(ROOT_DIR, rawPath);
      const script = join(SCRIPTS_DIR, "extract_text.js");
      const { stdout } = await execFileAsync(
        process.execPath,
        [script, "--format", "markdown", target],
        { maxBuffer: 20 * 1024 * 1024 }
      );
      return stdout;
    }

    case "execute_claw_ops": {
      const isHwpx = Boolean(args.isHwpx ?? args.is_hwpx);
      const payloadJson = String(args.payloadJson ?? args.payload_json ?? "{}");
      const script = join(SCRIPTS_DIR, isHwpx ? "hwpx-edit.js" : "create.js");
      return await runClawScriptWithStdin(script, payloadJson);
    }

    case "call_llm_http": {
      const req = args.req || {};
      const url = validateLlmEndpoint(String(req.url || ""));
      const bodyStr = serializeBoundedRequestBody(req.body_json);
      const timeoutMs = normalizeLlmTimeoutMs(req.timeout_ms);
      const requestId = req.request_id ? String(req.request_id) : undefined;
      const controller = new AbortController();
      if (requestId) {
        activeHttpRequests.register(requestId, controller);
      }
      const timer = setTimeout(() => controller.abort(), timeoutMs);
      try {
        const headers: Record<string, string> = {
          "Content-Type": "application/json",
          ...(req.headers || {}),
        };
        const resp = await fetch(url, {
          method: "POST",
          headers,
          body: bodyStr,
          signal: controller.signal,
        });
        const text = await readBoundedResponseText(resp);
        let parsedBody: unknown;
        try {
          parsedBody = JSON.parse(text);
        } catch {
          parsedBody = { raw_text: text };
        }
        return {
          status: resp.status,
          body: parsedBody,
        };
      } finally {
        clearTimeout(timer);
        if (requestId) {
          activeHttpRequests.cleanup(requestId, controller);
        }
      }
    }

    case "cancel_llm_request": {
      const requestId = String(args.requestId ?? args.request_id ?? "");
      return activeHttpRequests.cancel(requestId);
    }

    case "get_hwp_default_app_status": {
      return {
        isDefault: mockDefaultHwpAppEnabled,
        platform: process.platform,
        supported: true,
      };
    }

    case "set_hwp_default_app": {
      const enabled = Boolean(args.enabled);
      mockDefaultHwpAppEnabled = enabled;
      return {
        isDefault: mockDefaultHwpAppEnabled,
        platform: process.platform,
        supported: true,
      };
    }

    default:
      throw new Error(`Unknown bridge command: ${cmd}`);
  }
}

function createHopClawDevPlugin(): Plugin {
  const attachMiddlewares = (middlewares: any) => {
    middlewares.use("/fonts", (req: any, res: any, next: any) => {
      handleFontAssetRequest(FONTS_DIR, req.url, res, next);
    });

    middlewares.use("/__hop_claw_api/", (req: any, res: any, next: any) => {
      if (req.method !== "POST") {
        next();
        return;
      }
      const cmd = decodeURIComponent((req.url || "/").replace(/^\/+/, "").split("?")[0]);
      const chunks: Buffer[] = [];
      req.on("data", (chunk: Buffer) => chunks.push(Buffer.from(chunk)));
      req.on("end", async () => {
        res.setHeader("Content-Type", "application/json");
        try {
          const rawBody = Buffer.concat(chunks).toString("utf8");
          const args = rawBody ? JSON.parse(rawBody) : {};
          const result = await handleBridgeCommand(cmd, args);
          res.statusCode = 200;
          res.end(JSON.stringify({ result }));
        } catch (err: any) {
          res.statusCode = 200;
          res.end(
            JSON.stringify({
              __hop_error: err instanceof Error ? err.message : String(err),
            })
          );
        }
      });
    });
  };

  return {
    name: "hop-claw-dev-bridge",
    configureServer(server) {
      attachMiddlewares(server.middlewares);
    },
    configurePreviewServer(server) {
      attachMiddlewares(server.middlewares);
    },
  };
}

export default defineConfig({
  plugins: [react(), createHopClawDevPlugin()],
  define: {
    __APP_VERSION__: JSON.stringify("0.8.7"),
    __HOP_VERSION__: JSON.stringify("0.2.2"),
  },
  resolve: {
    alias: [
      ...createHopOverrides(HOP_HOST_DIR),
      {
        find: /^@\/upstream\/(.*)$/,
        replacement: `${resolve(HOP_HOST_DIR, "upstream")}/$1`,
      },
      {
        find: /^@\/(.*)$/,
        replacement: `${UPSTREAM_SRC_DIR}/$1`,
      },
      {
        find: /^@upstream\/(.*)$/,
        replacement: `${UPSTREAM_SRC_DIR}/$1`,
      },
      {
        find: /^@wasm\/(.*)$/,
        replacement: `${WASM_DIR}/$1`,
      },
    ],
  },
  server: {
    port: 3000,
    host: "0.0.0.0",
  },
  test: {
    include: ["src/**/*.test.ts", "src/**/*.test.tsx"],
    exclude: ["node_modules/**", "dist/**", "third_party/**"],
  },
} as any);
