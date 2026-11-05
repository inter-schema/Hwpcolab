import { describe, expect, it, vi, beforeEach } from 'vitest';

const { loadWebFontsMock } = vi.hoisted(() => ({
  loadWebFontsMock: vi.fn(),
}));

vi.mock('@/core/font-loader', () => ({
  loadWebFonts: loadWebFontsMock,
}));

vi.mock('@/upstream/core', () => ({
  userSettings: {
    getAllFontSets: () => [],
  },
}));

vi.mock('@/core/local-fonts', () => ({
  getLocalFonts: () => [],
}));

vi.mock('./custom-select', () => ({
  getCustomSelectRoot: () => null,
  syncCustomSelect: () => undefined,
}));

import { Toolbar } from './toolbar';

describe('Toolbar upstream keyboard behavior', () => {
  it('activates format buttons by keyboard without double dispatching a mouse click', () => {
    const listeners = new Map<string, (event: { detail?: number; preventDefault: () => void }) => void>();
    const button = { addEventListener: (type: string, listener: never) => listeners.set(type, listener) };
    const otherButton = { addEventListener: vi.fn() };
    const dispatch = vi.fn();
    const toolbar = {
      btnBold: button, btnItalic: otherButton, btnUnderline: otherButton, btnStrike: otherButton,
      dispatcher: { dispatch },
    };
    const setup = (Toolbar.prototype as unknown as Record<string, (this: object) => void>).setupFormatButtons;
    setup.call(toolbar);
    listeners.get('mousedown')!({ preventDefault: vi.fn() });
    listeners.get('click')!({ detail: 1, preventDefault: vi.fn() });
    expect(dispatch).toHaveBeenCalledTimes(1);
    listeners.get('click')!({ detail: 0, preventDefault: vi.fn() });
    expect(dispatch).toHaveBeenCalledTimes(2);
    expect(dispatch).toHaveBeenLastCalledWith('format:bold');
  });
});

type Deferred = {
  promise: Promise<void>;
  resolve: () => void;
};

function createDeferred(): Deferred {
  let resolve: () => void = () => undefined;
  const promise = new Promise<void>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

function createToolbarHarness() {
  const emit = vi.fn();
  const wasm = {
    findOrCreateFontId: vi.fn((name: string) => (name === 'Newest' ? 202 : 101)),
    findOrCreateFontIdForLang: vi.fn((lang: number) => lang + 1000),
  };

  const toolbar = {
    eventBus: { emit },
    fontLang: { value: 'all' },
    wasm,
    lastFontFamilies: ['A', 'B', 'C', 'D', 'E', 'F', 'G'],
    fontApplyRequestId: 0,
    beginFontApplyRequest: (Toolbar.prototype as unknown as Record<string, () => number>).beginFontApplyRequest,
    isLatestFontApplyRequest: (
      Toolbar.prototype as unknown as Record<string, (requestId: number) => boolean>
    ).isLatestFontApplyRequest,
  };

  return { toolbar, emit, wasm };
}

describe('Toolbar font application sequencing', () => {
  beforeEach(() => {
    loadWebFontsMock.mockReset();
  });

  it('ignores stale single-font selections when a newer selection finishes first', async () => {
    const first = createDeferred();
    const second = createDeferred();
    loadWebFontsMock
      .mockImplementationOnce(() => first.promise)
      .mockImplementationOnce(() => second.promise);

    const { toolbar, emit, wasm } = createToolbarHarness();
    const applyFontSelection = (
      Toolbar.prototype as unknown as Record<string, (this: object, name: string) => Promise<void>>
    ).applyFontSelection;

    const firstRun = applyFontSelection.call(toolbar, 'Older');
    const secondRun = applyFontSelection.call(toolbar, 'Newest');

    second.resolve();
    await secondRun;
    first.resolve();
    await firstRun;

    expect(loadWebFontsMock).toHaveBeenNthCalledWith(1, ['Older']);
    expect(loadWebFontsMock).toHaveBeenNthCalledWith(2, ['Newest']);
    expect(wasm.findOrCreateFontIdForLang).toHaveBeenCalledTimes(7);
    for (let lang = 0; lang < 7; lang++) {
      expect(wasm.findOrCreateFontIdForLang).toHaveBeenNthCalledWith(lang + 1, lang, 'Newest');
    }
    expect(emit).toHaveBeenCalledTimes(1);
    expect(emit).toHaveBeenCalledWith('format-char', {
      fontIds: [1000, 1001, 1002, 1003, 1004, 1005, 1006],
    });
  });

  it('prevents an older font-set apply from overwriting a newer single-font selection', async () => {
    const first = createDeferred();
    const second = createDeferred();
    loadWebFontsMock
      .mockImplementationOnce(() => first.promise)
      .mockImplementationOnce(() => second.promise);

    const { toolbar, emit, wasm } = createToolbarHarness();
    const applyFontSelection = (
      Toolbar.prototype as unknown as Record<string, (this: object, name: string) => Promise<void>>
    ).applyFontSelection;
    const applyFontSet = (
      Toolbar.prototype as unknown as Record<string, (this: object, fontSet: Record<string, string>) => Promise<void>>
    ).applyFontSet;

    const olderFontSet = {
      name: 'Older Set',
      korean: 'A',
      english: 'B',
      chinese: 'C',
      japanese: 'D',
      other: 'E',
      symbol: 'F',
      user: 'G',
    };

    const firstRun = applyFontSet.call(toolbar, olderFontSet);
    const secondRun = applyFontSelection.call(toolbar, 'Newest');

    second.resolve();
    await secondRun;
    first.resolve();
    await firstRun;

    expect(loadWebFontsMock).toHaveBeenNthCalledWith(1, ['A', 'B', 'C', 'D', 'E', 'F', 'G']);
    expect(loadWebFontsMock).toHaveBeenNthCalledWith(2, ['Newest']);
    expect(wasm.findOrCreateFontIdForLang).toHaveBeenCalledTimes(7);
    for (let lang = 0; lang < 7; lang++) {
      expect(wasm.findOrCreateFontIdForLang).toHaveBeenNthCalledWith(lang + 1, lang, 'Newest');
    }
    expect(emit).toHaveBeenCalledTimes(1);
    expect(emit).toHaveBeenCalledWith('format-char', {
      fontIds: [1000, 1001, 1002, 1003, 1004, 1005, 1006],
    });
  });

  it('sanitizes blocked single-font selections before applying them across all 7 languages', async () => {
    loadWebFontsMock.mockResolvedValue(undefined);
    const { toolbar, emit, wasm } = createToolbarHarness();
    const applyFontSelection = (
      Toolbar.prototype as unknown as Record<string, (this: object, name: string) => Promise<void>>
    ).applyFontSelection;

    await applyFontSelection.call(toolbar, 'HY헤드라인M');

    expect(loadWebFontsMock).toHaveBeenCalledWith(['함초롬돋움']);
    expect(wasm.findOrCreateFontIdForLang).toHaveBeenCalledTimes(7);
    expect(wasm.findOrCreateFontIdForLang).toHaveBeenNthCalledWith(1, 0, '함초롬돋움');
    expect(wasm.findOrCreateFontIdForLang).toHaveBeenNthCalledWith(2, 1, '함초롬돋움');
    expect(emit).toHaveBeenCalledWith('format-char', {
      fontIds: [1000, 1001, 1002, 1003, 1004, 1005, 1006],
    });
  });

  it('applies a per-language font even when lastFontFamilies was cleared after initFontDropdown', async () => {
    loadWebFontsMock.mockResolvedValue(undefined);
    const { toolbar, emit, wasm } = createToolbarHarness();
    toolbar.fontLang.value = '1';
    (toolbar as { lastFontFamilies?: string[] }).lastFontFamilies = undefined;
    (toolbar as { fontName?: { value: string } }).fontName = { value: '맑은 고딕' };
    const applyFontSelection = (
      Toolbar.prototype as unknown as Record<string, (this: object, name: string) => Promise<void>>
    ).applyFontSelection;

    await applyFontSelection.call(toolbar, 'D2Coding');

    expect(wasm.findOrCreateFontIdForLang).toHaveBeenNthCalledWith(1, 1, 'D2Coding');
    expect(wasm.findOrCreateFontIdForLang).toHaveBeenNthCalledWith(2, 0, '맑은 고딕');
    expect(emit).toHaveBeenCalledWith('format-char', {
      fontIds: [1000, 1001, 1002, 1003, 1004, 1005, 1006],
    });
  });

  it('automatically displays active script font when fontLang is all (대표)', () => {
    const getDisplayFontFamily = (
      Toolbar.prototype as unknown as Record<string, (this: object, props: Record<string, unknown>) => string | undefined>
    ).getDisplayFontFamily;
    const toolbar = { fontLang: { value: 'all' } };
    const mixedFamilies = ['함초롬바탕', 'Times New Roman', '바탕', 'MS Mincho', '함초롬바탕', 'Wingdings', '함초롬바탕'];

    expect(
      getDisplayFontFamily.call(toolbar, {
        fontFamily: '함초롬바탕',
        fontFamilies: mixedFamilies,
        activeFontLang: 1,
      }),
    ).toBe('Times New Roman');
    expect(
      getDisplayFontFamily.call(toolbar, {
        fontFamily: '함초롬바탕',
        fontFamilies: mixedFamilies,
        activeFontLang: 0,
      }),
    ).toBe('함초롬바탕');
  });

  it('preserves existing non-selected language font names when applying one language', async () => {
    loadWebFontsMock.mockResolvedValue(undefined);
    const { toolbar, wasm } = createToolbarHarness();
    toolbar.fontLang.value = '2';
    toolbar.lastFontFamilies = ['HY헤드라인M', 'B', 'C', 'D', 'E', 'F', 'G'];
    const applyFontSelection = (
      Toolbar.prototype as unknown as Record<string, (this: object, name: string) => Promise<void>>
    ).applyFontSelection;

    await applyFontSelection.call(toolbar, '휴먼명조');

    expect(loadWebFontsMock).toHaveBeenCalledWith(['함초롬바탕']);
    expect(wasm.findOrCreateFontIdForLang).toHaveBeenNthCalledWith(1, 2, '함초롬바탕');
    expect(wasm.findOrCreateFontIdForLang).toHaveBeenNthCalledWith(2, 0, 'HY헤드라인M');
  });

  it('sanitizes blocked font-set entries before applying language font IDs', async () => {
    loadWebFontsMock.mockResolvedValue(undefined);
    const { toolbar, emit, wasm } = createToolbarHarness();
    const applyFontSet = (
      Toolbar.prototype as unknown as Record<string, (this: object, fontSet: Record<string, string>) => Promise<void>>
    ).applyFontSet;

    await applyFontSet.call(toolbar, {
      name: 'Mixed Set',
      korean: 'HY헤드라인M',
      english: 'Pretendard',
      chinese: '휴먼명조',
      japanese: '나눔고딕',
      other: 'HCI Poppy',
      symbol: 'D2Coding',
      user: '맑은 고딕',
    });

    expect(loadWebFontsMock).toHaveBeenCalledWith([
      '함초롬돋움',
      'Pretendard',
      '함초롬바탕',
      '나눔고딕',
      '함초롬돋움',
      'D2Coding',
      '맑은 고딕',
    ]);
    expect(wasm.findOrCreateFontIdForLang).toHaveBeenNthCalledWith(1, 0, '함초롬돋움');
    expect(wasm.findOrCreateFontIdForLang).toHaveBeenNthCalledWith(3, 2, '함초롬바탕');
    expect(wasm.findOrCreateFontIdForLang).toHaveBeenNthCalledWith(5, 4, '함초롬돋움');
    expect(emit).toHaveBeenCalledWith('format-char', {
      fontIds: [1000, 1001, 1002, 1003, 1004, 1005, 1006],
    });
  });

  it('invalidates pending font applications when rebuilding the font dropdown for a new document', () => {
    const previousDocument = globalThis.document;
    const replaceChildren = vi.fn();
    const appendChild = vi.fn();
    const toolbar = {
      fontApplyRequestId: 7,
      lastFontFamilies: ['Old', 'Fonts'],
      fontName: {
        replaceChildren,
        appendChild,
      },
      populateFontSetOptions: vi.fn(),
      populateLocalFontOptions: vi.fn(),
      beginFontApplyRequest: (
        Toolbar.prototype as unknown as Record<string, () => number>
      ).beginFontApplyRequest,
    };
    const createElement = vi.fn(() => ({ value: '', textContent: '' }));
    (globalThis as { document?: unknown }).document = { createElement };
    const initFontDropdown = (
      Toolbar.prototype as unknown as Record<string, (this: object, docFonts?: string[]) => void>
    ).initFontDropdown;

    try {
      initFontDropdown.call(toolbar, ['DocFont']);

      expect(toolbar.fontApplyRequestId).toBe(8);
      expect(toolbar.lastFontFamilies).toBeUndefined();
      expect(replaceChildren).toHaveBeenCalledOnce();
      expect(appendChild).toHaveBeenCalledWith(expect.objectContaining({ value: 'DocFont' }));
    } finally {
      (globalThis as { document?: unknown }).document = previousDocument;
    }
  });
});
