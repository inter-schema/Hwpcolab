// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { CommandDef } from '@/upstream/commands';

const { MockTableCreateDialog, getMockState, mockApplyHumanNestedTableInsert } = vi.hoisted(() => {
  const mockApply = vi.fn().mockResolvedValue(undefined);
  const state = {
    isHwpx: true,
    tablesMeta: [
      {
        tableIndex: 0,
        sec: 0,
        para: 5,
        ctrl: 0,
        rowCount: 4,
        colCount: 5,
        cells: [
          { idx: 0, row: 1, col: 2, rowSpan: 1, colSpan: 1, text: '' },
        ],
      },
    ],
    applyHumanNestedTableInsert: mockApply,
  };

  class MockDialog {
    static lastInstance: MockDialog | null = null;
    onApply: any = null;
    show = vi.fn();
    constructor() {
      MockDialog.lastInstance = this;
    }
  }

  return {
    MockTableCreateDialog: MockDialog,
    getMockState: () => state,
    mockApplyHumanNestedTableInsert: mockApply,
  };
});

vi.mock('@/upstream/commands', () => ({
  tableCommands: [
    { id: 'table:create', label: '표 만들기', opensDialog: true, canExecute: (ctx: any) => ctx.hasDocument && !ctx.inTable, execute: vi.fn() },
    { id: 'table:cell-props', label: '셀 속성', execute: vi.fn() },
  ],
}));

vi.mock('@/ui/table-create-dialog', () => ({
  TableCreateDialog: MockTableCreateDialog,
}));

vi.mock('../../../store/useDocStore', () => ({
  useDocStore: {
    getState: getMockState,
  },
}));

import { tableCommands } from './table';

function command(id: string): CommandDef | undefined {
  return tableCommands.find((c) => c.id === id);
}

describe('table command overrides', () => {
  beforeEach(() => {
    MockTableCreateDialog.lastInstance = null;
    getMockState().isHwpx = true;
    mockApplyHumanNestedTableInsert.mockClear();
    globalThis.alert = vi.fn();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('overrides table:create so canExecute is true even when inTable is true', () => {
    const cmd = command('table:create');
    expect(cmd).toBeDefined();

    expect(cmd?.canExecute?.({ hasDocument: true, inTable: true } as any)).toBe(true);
    expect(cmd?.canExecute?.({ hasDocument: true, inTable: false } as any)).toBe(true);
    expect(cmd?.canExecute?.({ hasDocument: false, inTable: false } as any)).toBe(false);
  });

  it('opens TableCreateDialog for outer table when cursor is outside table', () => {
    const cmd = command('table:create');
    const inputHandler = {
      getCursorPosition: vi.fn(() => ({
        sectionIndex: 0,
        paragraphIndex: 2,
        charOffset: 0,
        parentParaIndex: undefined,
      })),
      executeOperation: vi.fn(),
      textarea: { focus: vi.fn() },
    };
    const services = {
      getInputHandler: () => inputHandler,
      wasm: {
        createTable: vi.fn(() => ({ ok: true, paraIdx: 2, controlIdx: 0 })),
      },
    };

    cmd?.execute(services as any);
    expect(MockTableCreateDialog.lastInstance).not.toBeNull();
    expect(MockTableCreateDialog.lastInstance?.show).toHaveBeenCalled();
  });

  it('rejects nested table creation if not HWPX format', () => {
    const alertSpy = vi.spyOn(globalThis, 'alert').mockImplementation(() => {});
    getMockState().isHwpx = false;

    const cmd = command('table:create');
    const inputHandler = {
      getCursorPosition: vi.fn(() => ({
        sectionIndex: 0,
        paragraphIndex: 0,
        charOffset: 0,
        parentParaIndex: 5,
        controlIndex: 0,
        cellIndex: 0,
      })),
    };
    const services = {
      getInputHandler: () => inputHandler,
      wasm: {
        getCellInfo: vi.fn(() => ({ row: 1, col: 2 })),
      },
    };

    cmd?.execute(services as any);
    expect(alertSpy).toHaveBeenCalledWith(
      expect.stringContaining('HWPX 형식에서 지원됩니다')
    );
  });

  it('opens TableCreateDialog and invokes applyHumanNestedTableInsert when inTable on HWPX', async () => {
    const cmd = command('table:create');
    const textareaFocus = vi.fn();
    const inputHandler = {
      getCursorPosition: vi.fn(() => ({
        sectionIndex: 0,
        paragraphIndex: 0,
        charOffset: 0,
        parentParaIndex: 5,
        controlIndex: 0,
        cellIndex: 0,
      })),
      textarea: { focus: textareaFocus },
    };
    const services = {
      getInputHandler: () => inputHandler,
      wasm: {
        getCellInfo: vi.fn(() => ({ row: 1, col: 2 })),
      },
    };

    cmd?.execute(services as any);
    expect(MockTableCreateDialog.lastInstance).not.toBeNull();
    expect(MockTableCreateDialog.lastInstance?.show).toHaveBeenCalled();
    expect(typeof MockTableCreateDialog.lastInstance?.onApply).toBe('function');

    // Simulate user submitting dialog
    await MockTableCreateDialog.lastInstance?.onApply(3, 4);

    expect(mockApplyHumanNestedTableInsert).toHaveBeenCalledWith(
      0, // tableIndex
      1, // row
      2, // col
      3, // rows
      4  // cols
    );
    expect(textareaFocus).toHaveBeenCalled();
  });
});
