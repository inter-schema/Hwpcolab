import { WasmBridge } from '@/upstream/core';

export function isTauriRuntime(): boolean {
  return false;
}

export function createBridge(): WasmBridge {
  return new WasmBridge();
}
