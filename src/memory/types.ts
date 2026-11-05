import type { ProviderConfig } from "../services/agent/types.ts";

export const FIXED_MEMORY_SECTIONS = [
  "문서 유형·양식",
  "어투·문체",
  "용어·표기",
  "서식·표 규칙",
  "구조(제목 체계)",
  "금지·지양",
  "예시 문구(일반화됨)",
] as const;

export type MemorySectionTitle = (typeof FIXED_MEMORY_SECTIONS)[number];

export interface SourceDocRef {
  name: string;
  sha256_8: string;
}

export interface MemoryMeta {
  schemaVersion: 1;
  id: string;
  name: string;
  description: string;
  revision: number;
  createdAt: string;
  updatedAt: string;
  sourceDocs: SourceDocRef[];
  masked: true;
}

export interface MemoryDoc {
  meta: MemoryMeta;
  body: string;
}

export interface SectionDiffToken {
  op: -1 | 0 | 1;
  text: string;
}

export interface SectionDiff {
  sectionTitle: MemorySectionTitle;
  beforeContent: string;
  afterContent: string;
  changed: boolean;
  tokens: SectionDiffToken[];
}

export type PiiCategory =
  | "mask_token"
  | "rrn"
  | "corp_reg"
  | "biz_reg"
  | "card"
  | "phone"
  | "email"
  | "account"
  | "passport_driver"
  | "vehicle"
  | "address"
  | "person_name";

export interface PiiRedactionHit {
  category: PiiCategory;
  placeholder: string;
  count: number;
}

export interface ScanAndRedactResult {
  text: string;
  hits: string[];
  hitDetails: PiiRedactionHit[];
  totalCount: number;
}

export interface MemoryStore {
  list(): Promise<MemoryMeta[]>;
  get(id: string): Promise<MemoryDoc | null>;
  save(doc: MemoryDoc, options?: { recordRevision?: boolean }): Promise<void>;
  delete(id: string): Promise<void>;
  listRevisions(id: string): Promise<number[]>;
  getRevision(id: string, rev: number): Promise<MemoryDoc | null>;
  rollbackToRevision(id: string, rev: number): Promise<MemoryDoc>;
  exportMarkdown(id: string): Promise<string>;
  importMarkdown(rawMd: string): Promise<{ doc: MemoryDoc; warnings: string[] }>;
}

export interface GenerateMemoryInputDoc {
  name: string;
  text: string;
}

export interface GenerateMemoryArgs {
  name: string;
  description?: string;
  docs: GenerateMemoryInputDoc[];
  provider: ProviderConfig;
  signal?: AbortSignal;
}

export interface GenerateMemoryResult {
  doc: MemoryDoc;
  warnings: string[];
  inputRedactionCount: number;
  outputRedactionCount: number;
}

export interface ProposeRefinementArgs {
  doc: MemoryDoc;
  instruction?: string;
  extraDocs?: GenerateMemoryInputDoc[];
  provider: ProviderConfig;
  signal?: AbortSignal;
}

export interface ProposeRefinementResult {
  proposedBody: string;
  sectionDiffs: SectionDiff[];
  warnings: string[];
  extraSourceDocs: SourceDocRef[];
}
