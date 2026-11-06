import { tableCommands as upstreamTableCommands } from '@/upstream/commands';
import type { CommandDef } from '@/upstream/commands';
import { replaceUpstreamCommands } from '../replace-upstream-commands';
import { TableCreateDialog } from '@/ui/table-create-dialog';
import type { TableCreateOptions } from '@/ui/table-create-dialog';
import { useDocStore } from '../../../store/useDocStore';

function safeTableOp(fn: () => void, label: string): void {
  try {
    fn();
  } catch (e) {
    console.error(`[table] ${label} 실패:`, e);
  }
}

const upstreamCreateCommand = upstreamTableCommands.find((c) => c.id === 'table:create');

export const overriddenTableCreateCommand: CommandDef = {
  id: 'table:create',
  label: upstreamCreateCommand?.label ?? '표 만들기',
  icon: 'icon-table',
  opensDialog: true,
  canExecute: (ctx) => ctx.hasDocument,
  execute(services, params) {
    const ih = services.getInputHandler();
    if (!ih) return;
    const pos = ih.getCursorPosition();

    // 1. 커서가 표 외부인 경우: 기존 upstream TableCreateDialog로 본문 표 생성
    if (pos.parentParaIndex === undefined) {
      const dialog = new TableCreateDialog();
      dialog.onApply = (rows, cols, options?: TableCreateOptions) => {
        const ih2 = services.getInputHandler();
        if (!ih2) return;
        safeTableOp(() => ih2.executeOperation({
          kind: 'snapshot',
          operationType: 'createTable',
          operation: (wasm) => {
            const result = options
              ? wasm.createTableEx({
                  sectionIdx: pos.sectionIndex,
                  paraIdx: pos.paragraphIndex,
                  charOffset: pos.charOffset,
                  rowCount: rows,
                  colCount: cols,
                  ...options,
                })
              : wasm.createTable(pos.sectionIndex, pos.paragraphIndex, pos.charOffset, rows, cols);
            if (result.ok) {
              return {
                sectionIndex: pos.sectionIndex,
                paragraphIndex: 0,
                charOffset: 0,
                parentParaIndex: result.paraIdx,
                controlIndex: result.controlIdx,
                cellIndex: 0,
                cellParaIndex: 0,
              };
            }
            return pos;
          },
        }), '표 만들기');
        (ih2 as any).textarea?.focus();
      };
      dialog.show(params?.anchorEl as HTMLElement | undefined);
      return;
    }

    // 2. 커서가 표 내부인 경우: 중첩 표 생성
    let row = 0;
    let col = 0;
    try {
      const cellInfo = services.wasm.getCellInfo(
        pos.sectionIndex,
        pos.parentParaIndex,
        pos.controlIndex ?? 0,
        pos.cellIndex ?? 0
      );
      if (cellInfo && typeof cellInfo.row === 'number') {
        row = cellInfo.row;
        col = cellInfo.col;
      }
    } catch (e) {
      console.warn('[table:create] getCellInfo 조회 실패, 기본 좌표 사용:', e);
    }

    const docState = useDocStore.getState();
    const { isHwpx, tablesMeta } = docState;

    if (!isHwpx) {
      alert('중첩 표는 HWPX 형식에서 지원됩니다. [새 문서 .hwpx]나 HWPX 파일에서 사용해주세요.');
      return;
    }

    // tablesMeta에서 tableIndex 매핑
    const matchedIdx = tablesMeta.findIndex(
      (t) =>
        t.sec === pos.sectionIndex &&
        t.para === pos.parentParaIndex &&
        t.ctrl === (pos.controlIndex ?? 0)
    );
    const tableIndex = matchedIdx !== -1 ? matchedIdx : 0;

    const dialog = new TableCreateDialog();
    dialog.onApply = async (rows, cols) => {
      const targetRows = Math.max(1, Math.min(8, rows));
      const targetCols = Math.max(1, Math.min(8, cols));
      try {
        await useDocStore.getState().applyHumanNestedTableInsert(
          tableIndex,
          row,
          col,
          targetRows,
          targetCols
        );
      } catch (err: any) {
        alert(`중첩 표 생성 실패: ${err?.message || err}`);
      }
      const ih2 = services.getInputHandler();
      (ih2 as any)?.textarea?.focus();
    };
    dialog.show(params?.anchorEl as HTMLElement | undefined);
  },
};

export const tableCommands: CommandDef[] = replaceUpstreamCommands(
  upstreamTableCommands,
  [overriddenTableCreateCommand]
);
