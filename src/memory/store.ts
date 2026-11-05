import {
  normalizeMemoryBody,
  parseMemoryMarkdown,
  serializeMemoryMarkdown,
} from "./frontmatter.ts";
import { scanAndRedactPii } from "./piiPatterns.ts";
import type { MemoryDoc, MemoryMeta, MemoryStore } from "./types.ts";

export const MEM_INDEX_KEY = "hop_mem_index";
export const MEM_DOC_PREFIX = "hop_mem_";
export const MAX_REVISIONS = 5;

class InMemoryStorage implements Storage {
  private map = new Map<string, string>();

  get length(): number {
    return this.map.size;
  }

  clear(): void {
    this.map.clear();
  }

  getItem(key: string): string | null {
    return this.map.has(key) ? this.map.get(key)! : null;
  }

  key(index: number): string | null {
    return Array.from(this.map.keys())[index] ?? null;
  }

  removeItem(key: string): void {
    this.map.delete(key);
  }

  setItem(key: string, value: string): void {
    this.map.set(key, String(value));
  }
}

function escapeRegex(str: string): string {
  return str.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function getStorageKeys(storage: Storage): string[] {
  const keys: string[] = [];
  for (let i = 0; i < storage.length; i++) {
    const k = storage.key(i);
    if (k !== null) keys.push(k);
  }
  return keys;
}

function safeSetItem(storage: Storage, key: string, value: string): void {
  try {
    storage.setItem(key, value);
  } catch (err: any) {
    if (
      err?.name === "QuotaExceededError" ||
      err?.code === 22 ||
      err?.name === "NS_ERROR_DOM_QUOTA_REACHED"
    ) {
      throw new Error(
        "로컬 저장 공간(localStorage)이 부족하여 작성 선호사항을 저장할 수 없습니다. 불필요한 메모리를 삭제하거나 .md 파일로 내보낸 뒤 정리해 주세요."
      );
    }
    throw err;
  }
}

export function createMemoryStore(storage?: Storage): MemoryStore {
  const resolvedStorage =
    storage ??
    (typeof localStorage !== "undefined"
      ? localStorage
      : new InMemoryStorage());

  return {
    async list(): Promise<MemoryMeta[]> {
      const raw = resolvedStorage.getItem(MEM_INDEX_KEY);
      if (!raw) return [];
      try {
        const parsed = JSON.parse(raw);
        if (Array.isArray(parsed)) {
          return parsed.sort(
            (a, b) =>
              new Date(b.updatedAt).getTime() - new Date(a.updatedAt).getTime()
          );
        }
      } catch {
        // ignore parse error, return empty
      }
      return [];
    },

    async get(id: string): Promise<MemoryDoc | null> {
      const raw = resolvedStorage.getItem(`${MEM_DOC_PREFIX}${id}`);
      if (!raw) return null;
      try {
        const parsed = parseMemoryMarkdown(raw);
        return parsed.doc;
      } catch {
        return null;
      }
    },

    async save(
      doc: MemoryDoc,
      options?: { recordRevision?: boolean }
    ): Promise<void> {
      // 1) 강제 비식별화 및 본문 정규화
      const sanitizedName = scanAndRedactPii(doc.meta.name).text;
      const sanitizedDesc = scanAndRedactPii(doc.meta.description).text;
      const sanitizedBody = normalizeMemoryBody(scanAndRedactPii(doc.body).text);

      const existing = await this.get(doc.meta.id);
      let nextRevision: number;

      // 2) 리비전 보관 처리
      if (existing && options?.recordRevision !== false) {
        safeSetItem(
          resolvedStorage,
          `${MEM_DOC_PREFIX}${doc.meta.id}_rev${existing.meta.revision}`,
          serializeMemoryMarkdown(existing)
        );
        nextRevision = existing.meta.revision + 1;

        // 리비전 수 제한 (최대 5개 유지)
        const currentRevs = await this.listRevisions(doc.meta.id);
        if (currentRevs.length > MAX_REVISIONS) {
          // currentRevs는 내림차순 정렬이므로 끝에서부터 제거 (가장 작은/오래된 리비전)
          const excessRevs = currentRevs.slice(MAX_REVISIONS);
          for (const rev of excessRevs) {
            resolvedStorage.removeItem(
              `${MEM_DOC_PREFIX}${doc.meta.id}_rev${rev}`
            );
          }
        }
      } else if (existing) {
        nextRevision = existing.meta.revision;
      } else {
        nextRevision = Math.max(1, doc.meta.revision || 1);
      }

      const now = new Date().toISOString();
      const docToSave: MemoryDoc = {
        meta: {
          ...doc.meta,
          name: sanitizedName,
          description: sanitizedDesc,
          revision: nextRevision,
          createdAt: doc.meta.createdAt || now,
          updatedAt: now,
          sourceDocs: doc.meta.sourceDocs || [],
          masked: true,
        },
        body: sanitizedBody,
      };

      // 마크다운 직렬화 저장
      safeSetItem(
        resolvedStorage,
        `${MEM_DOC_PREFIX}${docToSave.meta.id}`,
        serializeMemoryMarkdown(docToSave)
      );

      // 인덱스 갱신
      const indexList = await this.list();
      const existingIdx = indexList.findIndex((m) => m.id === docToSave.meta.id);
      if (existingIdx >= 0) {
        indexList[existingIdx] = docToSave.meta;
      } else {
        indexList.push(docToSave.meta);
      }
      indexList.sort(
        (a, b) =>
          new Date(b.updatedAt).getTime() - new Date(a.updatedAt).getTime()
      );
      safeSetItem(resolvedStorage, MEM_INDEX_KEY, JSON.stringify(indexList));
    },

    async delete(id: string): Promise<void> {
      resolvedStorage.removeItem(`${MEM_DOC_PREFIX}${id}`);

      // 모든 리비전 키 삭제
      const escapedId = escapeRegex(id);
      const revPattern = new RegExp(`^${escapeRegex(MEM_DOC_PREFIX)}${escapedId}_rev\\d+$`);
      const allKeys = getStorageKeys(resolvedStorage);
      for (const k of allKeys) {
        if (revPattern.test(k)) {
          resolvedStorage.removeItem(k);
        }
      }

      // 인덱스에서 제거
      const indexList = await this.list();
      const filtered = indexList.filter((m) => m.id !== id);
      safeSetItem(resolvedStorage, MEM_INDEX_KEY, JSON.stringify(filtered));
    },

    async listRevisions(id: string): Promise<number[]> {
      const escapedId = escapeRegex(id);
      const revPattern = new RegExp(`^${escapeRegex(MEM_DOC_PREFIX)}${escapedId}_rev(\\d+)$`);
      const allKeys = getStorageKeys(resolvedStorage);
      const revs: number[] = [];

      for (const k of allKeys) {
        const match = k.match(revPattern);
        if (match) {
          revs.push(parseInt(match[1], 10));
        }
      }

      // 내림차순 정렬 (최신 리비전 먼저)
      return revs.sort((a, b) => b - a);
    },

    async getRevision(id: string, rev: number): Promise<MemoryDoc | null> {
      const raw = resolvedStorage.getItem(`${MEM_DOC_PREFIX}${id}_rev${rev}`);
      if (!raw) return null;
      try {
        const parsed = parseMemoryMarkdown(raw);
        return parsed.doc;
      } catch {
        return null;
      }
    },

    async rollbackToRevision(id: string, rev: number): Promise<MemoryDoc> {
      const existing = await this.get(id);
      if (!existing) {
        throw new Error(`작성 선호사항을 찾을 수 없습니다: ${id}`);
      }
      const target = await this.getRevision(id, rev);
      if (!target) {
        throw new Error(`리비전 ${rev}를 찾을 수 없습니다: ${id}`);
      }

      await this.save(
        {
          meta: {
            ...existing.meta,
            description: target.meta.description,
            updatedAt: new Date().toISOString(),
          },
          body: target.body,
        },
        { recordRevision: true }
      );

      const rolledBack = await this.get(id);
      if (!rolledBack) {
        throw new Error(`롤백 후 작성 선호사항 조회 실패: ${id}`);
      }
      return rolledBack;
    },

    async exportMarkdown(id: string): Promise<string> {
      const doc = await this.get(id);
      if (!doc) {
        throw new Error(`작성 선호사항을 찾을 수 없습니다: ${id}`);
      }
      // PII 재검증
      const reCheckedDoc: MemoryDoc = {
        meta: {
          ...doc.meta,
          name: scanAndRedactPii(doc.meta.name).text,
          description: scanAndRedactPii(doc.meta.description).text,
          masked: true,
        },
        body: normalizeMemoryBody(scanAndRedactPii(doc.body).text),
      };
      return serializeMemoryMarkdown(reCheckedDoc);
    },

    async importMarkdown(
      rawMd: string
    ): Promise<{ doc: MemoryDoc; warnings: string[] }> {
      const parsed = parseMemoryMarkdown(rawMd);
      const nameScan = scanAndRedactPii(parsed.doc.meta.name);
      const descScan = scanAndRedactPii(parsed.doc.meta.description);
      const bodyScan = scanAndRedactPii(parsed.doc.body);

      const existing = await this.get(parsed.doc.meta.id);
      const finalId = existing ? `mem_${crypto.randomUUID()}` : parsed.doc.meta.id;

      const warnings: string[] = [];
      const totalCount =
        nameScan.totalCount + descScan.totalCount + bodyScan.totalCount;
      if (totalCount > 0) {
        warnings.push(
          `가져온 파일에서 개인정보 패턴 ${totalCount}건을 자동 비식별화했습니다.`
        );
      }

      const sanitizedDoc: MemoryDoc = {
        meta: {
          ...parsed.doc.meta,
          id: finalId,
          name: nameScan.text,
          description: descScan.text,
          sourceDocs: parsed.doc.meta.sourceDocs || [],
          masked: true,
        },
        body: normalizeMemoryBody(bodyScan.text),
      };

      await this.save(sanitizedDoc, { recordRevision: false });
      return { doc: sanitizedDoc, warnings };
    },
  };
}
