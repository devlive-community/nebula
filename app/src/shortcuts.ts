//! 可自定义的键盘快捷键:动作注册表 + 绑定的解析 / 匹配 / 格式化 / 录制。
//!
//! 绑定用形如 `"Mod+K"`、`"Mod+Shift+S"`、`"Delete"` 的字符串表示。`Mod` 匹配
//! macOS 的 ⌘ 或其它平台的 Ctrl;修饰键还支持 `Shift` / `Alt`。持久化在 SQLite 的
//! `ui_prefs`(key = `shortcuts`,值为 JSON:`动作 id → 绑定串`),仅覆盖用户改过的项。

/** 一个可绑定的动作。`label` 是中文键(经 i18n 翻译)。 */
export interface ShortcutAction {
  id: string;
  label: string;
  /** 默认绑定。 */
  def: string;
}

/** 全部可自定义的动作与默认绑定。 */
export const SHORTCUT_ACTIONS: ShortcutAction[] = [
  { id: "palette", label: "命令面板", def: "Mod+K" },
  { id: "refresh", label: "刷新", def: "Mod+R" },
  { id: "selectAll", label: "全选", def: "Mod+A" },
  { id: "deleteSelected", label: "删除选中", def: "Delete" },
  { id: "parent", label: "上一层", def: "Mod+ArrowUp" },
  { id: "upload", label: "上传", def: "Mod+U" },
  { id: "newFolder", label: "新建文件夹", def: "Mod+Shift+N" },
  { id: "sync", label: "备份 / 同步", def: "Mod+Shift+S" },
];

/** 动作 id → 绑定串。缺省的动作用其默认绑定。 */
export type Bindings = Record<string, string>;

const IS_MAC =
  typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.platform);

/** 合并默认绑定与用户覆盖,得到完整绑定表。 */
export function resolveBindings(custom: Bindings | null | undefined): Bindings {
  const out: Bindings = {};
  for (const a of SHORTCUT_ACTIONS) out[a.id] = custom?.[a.id] || a.def;
  return out;
}

/** 规范化按键名:统一大小写与常见别名。 */
function normKey(key: string): string {
  if (key === " ") return "Space";
  if (key.length === 1) return key.toUpperCase();
  return key;
}

/** 把一次 keydown 事件转成绑定串(录制用);纯修饰键返回 null。 */
export function eventToBinding(e: KeyboardEvent): string | null {
  const k = e.key;
  if (k === "Meta" || k === "Control" || k === "Shift" || k === "Alt") return null;
  const parts: string[] = [];
  if (e.metaKey || e.ctrlKey) parts.push("Mod");
  if (e.shiftKey) parts.push("Shift");
  if (e.altKey) parts.push("Alt");
  parts.push(normKey(k));
  return parts.join("+");
}

/** 判断一次 keydown 是否命中某绑定。 */
export function matchBinding(e: KeyboardEvent, binding: string): boolean {
  const parts = binding.split("+");
  const key = parts[parts.length - 1];
  const needMod = parts.includes("Mod");
  const needShift = parts.includes("Shift");
  const needAlt = parts.includes("Alt");
  const mod = e.metaKey || e.ctrlKey;
  if (needMod !== mod) return false;
  if (needShift !== e.shiftKey) return false;
  if (needAlt !== e.altKey) return false;
  return normKey(e.key) === key;
}

/** 把绑定串格式化成好看的样子(mac 用符号)。 */
export function formatBinding(binding: string): string {
  return binding
    .split("+")
    .map((p) => {
      if (p === "Mod") return IS_MAC ? "⌘" : "Ctrl";
      if (p === "Shift") return IS_MAC ? "⇧" : "Shift";
      if (p === "Alt") return IS_MAC ? "⌥" : "Alt";
      if (p === "ArrowUp") return "↑";
      if (p === "ArrowDown") return "↓";
      if (p === "ArrowLeft") return "←";
      if (p === "ArrowRight") return "→";
      return p;
    })
    .join(IS_MAC ? "" : "+");
}
