import {defineConfig} from 'vite'
import vue from '@vitejs/plugin-vue'
import Markdown from 'unplugin-vue-markdown/vite'
import anchor from 'markdown-it-anchor'
import {fileURLToPath, URL} from 'node:url'

// 官网:Vue 单文件组件 + Markdown(发布日志/博客)渲染为 Vue 组件,vite-ssg 静态预渲染
export default defineConfig({
  // 站点部署在自定义域名根(nebula.devlive.org),用绝对根路径:相对 './' 会让
  // /blog/<slug> 这类深层子页把资源解析到 /blog/assets、/blog/logo.svg 从而 404。
  base: '/',
  plugins: [
    Markdown({
      exposeFrontmatter: true,
      markdownItOptions: {html: true, linkify: true},
      markdownItSetup(md) {
        md.use(anchor, {permalink: anchor.permalink.headerLink()})
      }
    }),
    vue({include: [/\.vue$/, /\.md$/]})
  ],
  resolve: {
    alias: {'@': fileURLToPath(new URL('./src', import.meta.url))}
  },
  ssgOptions: {
    formatting: 'minify'
  }
})
