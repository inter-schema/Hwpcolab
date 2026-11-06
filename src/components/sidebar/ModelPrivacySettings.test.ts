// @vitest-environment happy-dom
import React, { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { ModelPrivacySettings } from "./ModelPrivacySettings";
import { useDocStore } from "../../store/useDocStore";

// Configure React act environment for happy-dom
(globalThis as any).IS_REACT_ACT_ENVIRONMENT = true;

describe("ModelPrivacySettings Component", () => {
  let container: HTMLDivElement;
  let root: Root;

  beforeEach(() => {
    vi.useFakeTimers();
    localStorage.clear();
    container = document.createElement("div");
    document.body.appendChild(container);
    root = createRoot(container);

    useDocStore.setState({
      settingsModalOpen: true,
      provider: "baze",
      apiKey: "test-baze-key",
      model: "gpt-6-luna",
      customEndpoint: "",
      autoMaskPii: true,
      reasoningEffort: "high",
      agentLoopMode: "legacy",
      customName: "사용자 지정",
      customSupportsTools: true,
      customSendReasoning: false,
    });
  });

  afterEach(() => {
    act(() => {
      root.unmount();
    });
    container.remove();
    vi.useRealTimers();
  });

  it("renders with responsive height classes and sticky footer", async () => {
    await act(async () => {
      root.render(React.createElement(ModelPrivacySettings));
    });

    const modalBackdrop = container.querySelector(".fixed.inset-0");
    expect(modalBackdrop).not.toBeNull();

    // Verify modal card has max-h-[88vh] and flex-col
    const modalContent = container.querySelector(".max-h-\\[88vh\\]");
    expect(modalContent).not.toBeNull();
    expect(modalContent?.className).toContain("overflow-hidden");
    expect(modalContent?.className).toContain("flex-col");

    // Verify header has shrink-0
    const header = modalContent?.querySelector(".border-b");
    expect(header?.className).toContain("shrink-0");

    // Verify body has flex-1 overflow-y-auto min-h-0
    const body = modalContent?.querySelector(".overflow-y-auto");
    expect(body?.className).toContain("flex-1");
    expect(body?.className).toContain("min-h-0");

    // Verify footer has shrink-0 and sticky bottom-0
    const footer = modalContent?.querySelector(".sticky.bottom-0");
    expect(footer).not.toBeNull();
    expect(footer?.className).toContain("shrink-0");

    // Verify save button exists with id
    const saveBtn = container.querySelector("#settings-save-button");
    expect(saveBtn).not.toBeNull();
    expect(saveBtn?.textContent).toContain("설정 저장");
  });

  it("shows save feedback and closes modal after timeout", async () => {
    await act(async () => {
      root.render(React.createElement(ModelPrivacySettings));
    });

    const saveBtn = container.querySelector("#settings-save-button") as HTMLButtonElement;
    expect(saveBtn).not.toBeNull();

    await act(async () => {
      saveBtn.click();
    });

    // Feedback should show immediately
    expect(saveBtn.textContent).toContain("저장 완료!");
    expect(saveBtn.className).toContain("bg-emerald-600");

    // After 600ms, modal should be closed
    await act(async () => {
      vi.advanceTimersByTime(650);
    });

    expect(useDocStore.getState().settingsModalOpen).toBe(false);
  });
});
