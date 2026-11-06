// @vitest-environment happy-dom
import { describe, it, expect, vi, beforeEach } from "vitest";
import { useDocStore } from "../../store/useDocStore";
import { createMemoryStore, type MemoryDoc } from "../../memory/index.ts";
import { HomeScreen } from "../../hop-host/ui/home-screen";

describe("Wave 2 Stream D2: Writing Memory UI and Store Integration Tests", () => {
  beforeEach(() => {
    localStorage.clear();
    useDocStore.setState({
      memoryModalOpen: false,
      memoryModalInitialTab: "list",
      memoriesIndex: [],
      activeMemoryIds: [],
    });
  });

  describe("Store initial state and activeMemoryIds invariant", () => {
    it("has memoryModalOpen === false and activeMemoryIds === [] by default", () => {
      const state = useDocStore.getState();
      expect(state.memoryModalOpen).toBe(false);
      expect(state.memoryModalInitialTab).toBe("list");
      expect(state.memoriesIndex).toEqual([]);
      expect(state.activeMemoryIds).toEqual([]); // Must be initially empty (all off by default)
    });

    it("opens and closes memory modal properly", () => {
      const store = useDocStore.getState();
      store.openMemoryModal("create_multi");
      expect(useDocStore.getState().memoryModalOpen).toBe(true);
      expect(useDocStore.getState().memoryModalInitialTab).toBe("create_multi");

      store.closeMemoryModal();
      expect(useDocStore.getState().memoryModalOpen).toBe(false);
    });
  });

  describe("toggleActiveMemory, moveActiveMemoryPriority, and clearActiveMemories", () => {
    it("toggles active memories in priority order (appends to end when toggled on)", () => {
      const store = useDocStore.getState();

      store.toggleActiveMemory("mem-1");
      expect(useDocStore.getState().activeMemoryIds).toEqual(["mem-1"]);

      store.toggleActiveMemory("mem-2");
      expect(useDocStore.getState().activeMemoryIds).toEqual(["mem-1", "mem-2"]);

      // Toggling an existing id removes it
      store.toggleActiveMemory("mem-1");
      expect(useDocStore.getState().activeMemoryIds).toEqual(["mem-2"]);

      store.toggleActiveMemory("mem-2");
      expect(useDocStore.getState().activeMemoryIds).toEqual([]);
    });

    it("moves priority up and down correctly with bounds checking", () => {
      useDocStore.setState({ activeMemoryIds: ["mem-1", "mem-2", "mem-3"] });
      const store = useDocStore.getState();

      // Move middle element up
      store.moveActiveMemoryPriority("mem-2", "up");
      expect(useDocStore.getState().activeMemoryIds).toEqual(["mem-2", "mem-1", "mem-3"]);

      // Move top element up (should be no-op)
      store.moveActiveMemoryPriority("mem-2", "up");
      expect(useDocStore.getState().activeMemoryIds).toEqual(["mem-2", "mem-1", "mem-3"]);

      // Move top element down
      store.moveActiveMemoryPriority("mem-2", "down");
      expect(useDocStore.getState().activeMemoryIds).toEqual(["mem-1", "mem-2", "mem-3"]);

      // Move bottom element down (should be no-op)
      store.moveActiveMemoryPriority("mem-3", "down");
      expect(useDocStore.getState().activeMemoryIds).toEqual(["mem-1", "mem-2", "mem-3"]);

      // Non-existent id is ignored
      store.moveActiveMemoryPriority("non-existent", "up");
      expect(useDocStore.getState().activeMemoryIds).toEqual(["mem-1", "mem-2", "mem-3"]);
    });

    it("clears all active memories", () => {
      useDocStore.setState({ activeMemoryIds: ["mem-1", "mem-2"] });
      expect(useDocStore.getState().activeMemoryIds.length).toBe(2);

      useDocStore.getState().clearActiveMemories();
      expect(useDocStore.getState().activeMemoryIds).toEqual([]);
    });
  });

  describe("refreshMemoriesIndex auto-cleanup on deleted memory", () => {
    it("filters out deleted memory IDs from activeMemoryIds", async () => {
      const memStore = createMemoryStore();

      const doc1: MemoryDoc = {
        meta: {
          schemaVersion: 1,
          id: "mem-del-1",
          name: "메모리 1",
          description: "설명 1",
          revision: 1,
          createdAt: new Date().toISOString(),
          updatedAt: new Date().toISOString(),
          sourceDocs: [],
          masked: true,
        },
        body: "## 문서 유형·양식\n- 규칙 1",
      };

      const doc2: MemoryDoc = {
        meta: {
          schemaVersion: 1,
          id: "mem-del-2",
          name: "메모리 2",
          description: "설명 2",
          revision: 1,
          createdAt: new Date().toISOString(),
          updatedAt: new Date().toISOString(),
          sourceDocs: [],
          masked: true,
        },
        body: "## 문서 유형·양식\n- 규칙 2",
      };

      await memStore.save(doc1, { recordRevision: false });
      await memStore.save(doc2, { recordRevision: false });

      // Set active memory IDs to both docs
      useDocStore.setState({ activeMemoryIds: ["mem-del-1", "mem-del-2"] });

      // Refresh index
      await useDocStore.getState().refreshMemoriesIndex();
      expect(useDocStore.getState().memoriesIndex.map((m) => m.id)).toContain("mem-del-1");
      expect(useDocStore.getState().memoriesIndex.map((m) => m.id)).toContain("mem-del-2");
      expect(useDocStore.getState().activeMemoryIds).toEqual(["mem-del-1", "mem-del-2"]);

      // Now delete doc1 directly from store
      await memStore.delete("mem-del-1");

      // Refresh index again
      await useDocStore.getState().refreshMemoriesIndex();

      // mem-del-1 must be purged from both memoriesIndex and activeMemoryIds
      const current = useDocStore.getState();
      expect(current.memoriesIndex.map((m) => m.id)).toEqual(["mem-del-2"]);
      expect(current.activeMemoryIds).toEqual(["mem-del-2"]);
    });
  });

  describe("HomeScreen button and actions integration", () => {
    it("renders #home-writing-memory-btn and triggers openWritingMemory on click", () => {
      const container = document.createElement("div");
      const openWritingMemoryMock = vi.fn();

      new HomeScreen(container, {}, {
        openFile: vi.fn(),
        createNewDocument: vi.fn(),
        openWritingMemory: openWritingMemoryMock,
        onDocumentLoaded: vi.fn(),
        setMessage: vi.fn(),
      });

      const memoryBtn = container.querySelector("#home-writing-memory-btn") as HTMLButtonElement;
      expect(memoryBtn).not.toBeNull();
      expect(memoryBtn.id).toBe("home-writing-memory-btn");
      expect(memoryBtn.dataset.testid).toBe("home-writing-memory-btn");
      expect(memoryBtn.textContent).toContain("작성 선호사항 (양식 메모리)");
      expect(memoryBtn.textContent).toContain("사내 공문서·보고서 양식 및 문체 규칙을 분석하여 AI 메모리로 보관");

      memoryBtn.click();
      expect(openWritingMemoryMock).toHaveBeenCalledTimes(1);
    });
  });
});
