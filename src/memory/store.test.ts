import { describe, expect, it } from "vitest";
import {
  createDefaultMemoryBody,
  serializeMemoryMarkdown,
} from "./frontmatter.ts";
import {
  createMemoryStore,
  MAX_REVISIONS,
  MEM_INDEX_KEY,
} from "./store.ts";
import type { MemoryDoc } from "./types.ts";

describe("src/memory/store", () => {
  it("performs CRUD operations properly", () => {
    const store = createMemoryStore();
    const docId = "mem_crud_1";
    const initialDoc: MemoryDoc = {
      meta: {
        schemaVersion: 1,
        id: docId,
        name: "기본 공문서 양식",
        description: "행정기관 공문서 서식",
        revision: 1,
        createdAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
        sourceDocs: [{ name: "doc1.hwpx", sha256_8: "aabbccdd" }],
        masked: true,
      },
      body: createDefaultMemoryBody(),
    };

    return store
      .save(initialDoc)
      .then(() => store.get(docId))
      .then((retrieved) => {
        expect(retrieved).not.toBeNull();
        expect(retrieved!.meta.id).toBe(docId);
        expect(retrieved!.meta.name).toBe("기본 공문서 양식");
        expect(retrieved!.meta.revision).toBe(1);
        return store.list();
      })
      .then((list) => {
        expect(list).toHaveLength(1);
        expect(list[0].id).toBe(docId);
        return store.delete(docId);
      })
      .then(() => store.get(docId))
      .then((afterDel) => {
        expect(afterDel).toBeNull();
        return store.list();
      })
      .then((emptyList) => {
        expect(emptyList).toHaveLength(0);
      });
  });

  it("accumulates revisions on edit and retains only recent 5 revisions on 6th edit", async () => {
    const store = createMemoryStore();
    const docId = "mem_rev_test";
    const baseDoc: MemoryDoc = {
      meta: {
        schemaVersion: 1,
        id: docId,
        name: "개정 테스트",
        description: "버전 관리",
        revision: 1,
        createdAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
        sourceDocs: [],
        masked: true,
      },
      body: createDefaultMemoryBody(),
    };

    await store.save(baseDoc, { recordRevision: false });
    expect((await store.get(docId))!.meta.revision).toBe(1);
    expect(await store.listRevisions(docId)).toEqual([]);

    // 6 updates
    for (let i = 2; i <= 7; i++) {
      const current = (await store.get(docId))!;
      await store.save(
        {
          ...current,
          meta: {
            ...current.meta,
            description: `설명 개정 ${i}`,
          },
        },
        { recordRevision: true }
      );
    }

    const latest = (await store.get(docId))!;
    expect(latest.meta.revision).toBe(7);

    const revisions = await store.listRevisions(docId);
    // Should retain at most MAX_REVISIONS = 5
    expect(revisions.length).toBe(MAX_REVISIONS);
    // Should be sorted descending: [6, 5, 4, 3, 2]
    expect(revisions).toEqual([6, 5, 4, 3, 2]);

    // Check getting revision 6
    const rev6 = await store.getRevision(docId, 6);
    expect(rev6).not.toBeNull();
    expect(rev6!.meta.revision).toBe(6);

    // Old revision 1 should have been purged
    const rev1 = await store.getRevision(docId, 1);
    expect(rev1).toBeNull();
  });

  it("rollbacks to specified revision and preserves pre-rollback state as a new revision", async () => {
    const store = createMemoryStore();
    const docId = "mem_rollback_test";
    const initialDoc: MemoryDoc = {
      meta: {
        schemaVersion: 1,
        id: docId,
        name: "롤백 테스트",
        description: "초기 버전 1",
        revision: 1,
        createdAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
        sourceDocs: [],
        masked: true,
      },
      body: createDefaultMemoryBody(),
    };

    await store.save(initialDoc, { recordRevision: false });

    // Update to v2 with unique body
    const docV2: MemoryDoc = {
      ...initialDoc,
      meta: { ...initialDoc.meta, description: "수정 버전 2" },
      body: createDefaultMemoryBody().replace(
        "## 어투·문체\n- (내용 없음)",
        "## 어투·문체\n- v2 특별 어투"
      ),
    };
    await store.save(docV2, { recordRevision: true });

    // Rollback to revision 1
    const rolledBack = await store.rollbackToRevision(docId, 1);
    expect(rolledBack.meta.revision).toBe(3);
    expect(rolledBack.body).toContain("- (내용 없음)");
    expect(rolledBack.body).not.toContain("v2 특별 어투");

    // Revision 2 must be preserved in revision history
    const rev2 = await store.getRevision(docId, 2);
    expect(rev2).not.toBeNull();
    expect(rev2!.body).toContain("v2 특별 어투");
  });

  it("handles exportMarkdown and importMarkdown with PII redaction and duplicate ID prevention", async () => {
    const store = createMemoryStore();
    const existingDoc: MemoryDoc = {
      meta: {
        schemaVersion: 1,
        id: "mem_export_1",
        name: "기존 메모리 (담당: 홍길동)",
        description: "전화번호: 010-1234-5678",
        revision: 1,
        createdAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
        sourceDocs: [],
        masked: true,
      },
      body: createDefaultMemoryBody().replace(
        "## 어투·문체\n- (내용 없음)",
        "## 어투·문체\n- 문의처: test@example.com, 계좌: 123-456-789012"
      ),
    };

    await store.save(existingDoc, { recordRevision: false });

    const exportedMd = await store.exportMarkdown("mem_export_1");
    expect(exportedMd).toContain("---");
    expect(exportedMd).toContain("<이름>");
    expect(exportedMd).toContain("<전화번호>");
    expect(exportedMd).toContain("<이메일>");
    expect(exportedMd).toContain("<계좌번호>");

    // Now import markdown that has the same ID "mem_export_1"
    const importResult = await store.importMarkdown(exportedMd);
    expect(importResult.warnings.length).toBeGreaterThanOrEqual(0);
    // Since mem_export_1 already exists, importMarkdown should assign a new unique ID
    expect(importResult.doc.meta.id).not.toBe("mem_export_1");
    expect(importResult.doc.meta.id).toMatch(/^mem_/);

    const list = await store.list();
    expect(list.length).toBe(2);
  });

  it("catches QuotaExceededError and throws specific Korean error message", async () => {
    // Mock storage that throws QuotaExceededError on setItem
    const quotaMockStorage: Storage = {
      length: 0,
      clear: () => {},
      getItem: () => null,
      key: () => null,
      removeItem: () => {},
      setItem: () => {
        const err = new Error("Storage full");
        err.name = "QuotaExceededError";
        throw err;
      },
    };

    const store = createMemoryStore(quotaMockStorage);
    const doc: MemoryDoc = {
      meta: {
        schemaVersion: 1,
        id: "mem_quota",
        name: "용량 테스트",
        description: "",
        revision: 1,
        createdAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
        sourceDocs: [],
        masked: true,
      },
      body: createDefaultMemoryBody(),
    };

    await expect(store.save(doc)).rejects.toThrow(
      "로컬 저장 공간(localStorage)이 부족하여 작성 선호사항을 저장할 수 없습니다. 불필요한 메모리를 삭제하거나 .md 파일로 내보낸 뒤 정리해 주세요."
    );
  });
});
