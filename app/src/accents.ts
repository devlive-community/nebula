// 强调色预设:每个定义主色与悬停色。"default" 表示用主题自带的蓝色(不覆盖)。
export type Accent =
  | "default"
  | "purple"
  | "green"
  | "orange"
  | "pink"
  | "teal";

export const ACCENTS: { id: Accent; color: string; hover: string }[] = [
  { id: "default", color: "#4f8cff", hover: "#3d7bf0" },
  { id: "purple", color: "#7c5cff", hover: "#6a49ec" },
  { id: "green", color: "#22c55e", hover: "#16a34a" },
  { id: "orange", color: "#f97316", hover: "#ea580c" },
  { id: "pink", color: "#ec4899", hover: "#db2777" },
  { id: "teal", color: "#14b8a6", hover: "#0d9488" },
];

/** 把强调色应用到文档根;"default" 清除覆盖,回到主题自带主色。 */
export function applyAccent(accent: Accent) {
  const root = document.documentElement;
  if (accent === "default") {
    root.style.removeProperty("--primary");
    root.style.removeProperty("--primary-hover");
    return;
  }
  const a = ACCENTS.find((x) => x.id === accent);
  if (!a) return;
  root.style.setProperty("--primary", a.color);
  root.style.setProperty("--primary-hover", a.hover);
}
