# Nebula 官网

Nebula 的官方落地页,React + Vite + 自定义 CSS。

```bash
cd website
pnpm install
pnpm dev      # 本地预览
pnpm build    # 产出到 dist/
```

推送到 `main` / `dev` 且改动 `website/**` 时,CI(`.github/workflows/website.yml`)会自动
构建并部署到 GitHub Pages。`vite.config.ts` 用相对 `base`,根路径与项目子路径部署皆可。
