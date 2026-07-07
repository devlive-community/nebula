import type {RouteRecordRaw} from 'vue-router'
import Home from './pages/Home.vue'
import Download from './pages/Download.vue'
import ReleaseList from './pages/ReleaseList.vue'
import BlogList from './pages/BlogList.vue'
import NotFound from './pages/NotFound.vue'
import {releaseRoutes} from './content/releases'
import {blogRoutes} from './content/blogs'

export const routes: RouteRecordRaw[] = [
  {path: '/', component: Home, meta: {title: 'Nebula — 多云对象存储管理器'}},
  {path: '/download', component: Download, meta: {title: '下载 Nebula'}},
  {path: '/release', component: ReleaseList, meta: {title: '发布日志'}},
  {path: '/blog', component: BlogList, meta: {title: '技术博客'}},
  ...releaseRoutes.map(r => ({...r, meta: {doc: true, docType: 'release'}})),
  ...blogRoutes.map(r => ({...r, meta: {doc: true, docType: 'blog'}})),
  {path: '/:pathMatch(.*)*', component: NotFound, meta: {title: '页面未找到'}}
]
