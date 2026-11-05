import { describe, expect, it, vi } from 'vitest';
import { InputHandler } from '@/upstream/editor';
import { onCompositionStart, onInput, onCompositionEnd } from '@upstream/engine/input-handler-text';
import { resolveUniformFontIds } from '@/core/font-ids';

describe('Pending char shape across multi-stroke Korean IME & empty space', () => {
  it('preserves pending 7-lang fontIds across multi-jamo Korean IME composition (ㅎ->하->한->한글) and subsequent English typing', () => {
    let bodyText = '';
    const appliedFormats: Array<{ from: number; to: number; json: string }> = [];
    const emittedFormats: unknown[] = [];

    let cursorPos = { sectionIndex: 0, paragraphIndex: 0, charOffset: 0 };
    const wasm = {
      findOrCreateFontId: vi.fn(() => 1),
      findOrCreateFontIdForLang: vi.fn((lang: number) => 200 + lang),
      applyCharFormat: vi.fn((_sec: number, _para: number, from: number, to: number, propsJson: string) => {
        appliedFormats.push({ from, to, json: propsJson });
      }),
      getCharPropertiesAt: vi.fn(() => ({
        fontFamily: '함초롬바탕',
        fontFamilies: Array(7).fill('함초롬바탕'),
        fontSize: 1000,
        bold: false,
      })),
      getTextRange: vi.fn((_sec: number, _para: number, from: number, count: number) =>
        Array.from(bodyText).slice(from, from + count).join(''),
      ),
    };

    const proto = InputHandler.prototype as unknown as Record<string, (...args: any[]) => any>;
    const host: Record<string, any> = {
      active: true,
      wasm,
      eventBus: {
        emit: vi.fn((evt: string, payload: unknown) => {
          if (evt === 'cursor-format-changed') emittedFormats.push(payload);
        }),
      },
      cursor: {
        isInHeaderFooter: () => false,
        isInFootnote: () => false,
        hasSelection: () => false,
        getSelectionOrdered: () => null,
        getPosition: () => ({ ...cursorPos }),
        moveTo: (next: typeof cursorPos) => {
          cursorPos = { ...next };
        },
        getRect: () => ({ pageIndex: 0, x: 0, y: 0, height: 16 }),
      },
      caret: {
        hideComposition: vi.fn(),
      },
      textarea: { value: '' },
      isComposing: false,
      compositionAnchor: null,
      compositionLength: 0,
      pendingCharShape: null,
      pendingCharShapeAnchor: null,
      stagePendingCharShape: proto.stagePendingCharShape,
      getPendingCharShape: proto.getPendingCharShape,
      applyPendingCharShapeToRange: proto.applyPendingCharShapeToRange,
      advancePendingCharShapeAnchor: proto.advancePendingCharShapeAnchor,
      getNonEmptySelection: proto.getNonEmptySelection,
      getCharPropertiesAtCursor: proto.getCharPropertiesAtCursor,
      emitCursorFormatState: vi.fn(function (this: any) {
        this.eventBus.emit('cursor-format-changed', this.getCharPropertiesAtCursor());
      }),
      getTextAt: (_pos: { charOffset: number }, count: number) =>
        Array.from(bodyText).slice(_pos.charOffset, _pos.charOffset + count).join(''),
      canInsertTextInFormMode: () => true,
      canDeleteTextInFormMode: () => true,
      resetRawTextMutationEffects: vi.fn(),
      consumeRawTextMutationBeforeCursor: () => false,
      afterTextInputEdit: vi.fn(),
      updateCaret: vi.fn(),
      replaceTextAtRaw: (pos: { charOffset: number }, deleteCount: number, text: string) => {
        const chars = Array.from(bodyText);
        chars.splice(pos.charOffset, deleteCount, ...Array.from(text));
        bodyText = chars.join('');
      },
      executeOperation: vi.fn((desc: any) => {
        if (desc.kind === 'command' && desc.command.type === 'insertText') {
          const before = { ...cursorPos };
          const chars = Array.from(bodyText);
          chars.splice(before.charOffset, 0, ...Array.from(desc.command.text as string));
          bodyText = chars.join('');
          const after = { ...before, charOffset: before.charOffset + Array.from(desc.command.text as string).length };
          const fmt = desc.command.getCharFormat?.();
          if (fmt) {
            wasm.applyCharFormat(0, 0, before.charOffset, after.charOffset, JSON.stringify(fmt));
          }
          cursorPos = after;
          host.advancePendingCharShapeAnchor(before, after);
        }
      }),
    };

    const fontIds = resolveUniformFontIds(wasm, '맑은 고딕')!;
    host.stagePendingCharShape({ fontIds });

    // Toolbar receives pending font immediately in empty space
    expect(emittedFormats.at(-1)).toMatchObject({
      fontFamily: '맑은 고딕',
      fontFamilies: Array(7).fill('맑은 고딕'),
    });

    // Compose 1st Korean syllable: ㅎ -> 하 -> 한
    onCompositionStart.call(host);
    for (const step of ['ㅎ', '하', '한']) {
      host.textarea.value = step;
      onInput.call(host);
    }
    onCompositionEnd.call(host);

    // Pending char shape MUST still be alive for 2nd Korean syllable: ㄱ -> 그 -> 글
    expect(host.getPendingCharShape()).toEqual({ fontIds });
    onCompositionStart.call(host);
    for (const step of ['ㄱ', '그', '글']) {
      host.textarea.value = step;
      onInput.call(host);
    }
    onCompositionEnd.call(host);

    // And still alive for non-IME English typing: 'A'
    expect(host.getPendingCharShape()).toEqual({ fontIds });
    host.textarea.value = 'A';
    onInput.call(host);

    expect(bodyText).toBe('한글A');
    // Every single stroke (all 6 IME updates + 'A') received the 7-lang fontIds
    expect(appliedFormats).toHaveLength(7);
    expect(appliedFormats[2]).toEqual({
      from: 0,
      to: 1,
      json: JSON.stringify({ fontIds }),
    });
    expect(appliedFormats[5]).toEqual({
      from: 1,
      to: 2,
      json: JSON.stringify({ fontIds }),
    });
    expect(appliedFormats[6]).toEqual({
      from: 2,
      to: 3,
      json: JSON.stringify({ fontIds }),
    });
  });
});
