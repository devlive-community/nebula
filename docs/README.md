# Nebula 官网 / 文档

本目录既是 Nebula 的**官方站点**,也放**开发文档**:

- 站点:Vue 3 + Vite + vite-ssg + Tailwind + vue-i18n(下面),部署到 GitHub Pages。
- 开发文档:[`sdk-playbook.md`](./sdk-playbook.md)(SDK 开发手册)与 [`sdk/`](./sdk/)(厂商规格卡)—— 松散 Markdown,不参与站点构建。

Nebula 的官方站点,Vue 3 + Vite + vite-ssg + Tailwind + vue-i18n。首页 / 下载 / 博客 /
发布日志,发布日志与博客由 `src/content/**` 下的 Markdown 驱动(vite-ssg 全量静态预渲染)。

```bash
cd website
pnpm install
pnpm dev      # 本地开发(SPA)
pnpm build    # vite-ssg 预渲染到 dist/
```

## 新增内容

- **发布日志**:往 `src/content/release/` 丢一个 `<version>.md`(frontmatter: `title/date/description`),
  下载页与发布日志页自动出现,并生成 `/release/<version>` 静态页。
- **博客**:往 `src/content/blog/` 丢一个 `<slug>.md`(frontmatter: `title/date/author/description/tags`)。

推送到 `main` / `dev` 且改动 `website/**` 时,CI(`.github/workflows/website.yml`)自动构建并
部署到 GitHub Pages。`vite.config.ts` 用相对 `base`,根路径 / 子路径部署皆可。
