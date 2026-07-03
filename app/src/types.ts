export type EntryKind = "file" | "directory";

/** 与 Rust 端 nebula_provider::Entry 对应。 */
export interface Entry {
  name: string;
  path: string;
  kind: EntryKind;
  size: number;
  last_modified: string | null;
  etag: string | null;
}
