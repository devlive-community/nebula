<template>
  <div class="min-h-screen flex flex-col font-sans antialiased bg-white dark:bg-slate-950 text-slate-800 dark:text-slate-200">
    <SiteHeader/>
    <main class="flex-1">
      <RouterView v-slot="{ Component, route }">
        <DocLayout v-if="route.meta.doc && route.meta.docType === 'release'">
          <component :is="Component"/>
        </DocLayout>
        <BlogLayout v-else-if="route.meta.doc && route.meta.docType === 'blog'">
          <component :is="Component"/>
        </BlogLayout>
        <component :is="Component" v-else/>
      </RouterView>
    </main>
    <SiteFooter/>
  </div>
</template>

<script setup lang="ts">
import {useHead} from '@unhead/vue'
import SiteHeader from './components/SiteHeader.vue'
import SiteFooter from './components/SiteFooter.vue'
import DocLayout from './layouts/DocLayout.vue'
import BlogLayout from './layouts/BlogLayout.vue'

const SITE = 'https://nebula.devlive.org'
const OG_IMAGE = `${SITE}/og-image.png`

// 全站默认分享元信息(社交平台抓取):带网站 LOGO 卡片。具体页面(如博客)会覆盖标题 / 描述 / URL。
useHead({
  meta: [
    {property: 'og:site_name', content: 'Nebula'},
    {property: 'og:type', content: 'website'},
    {property: 'og:image', content: OG_IMAGE},
    {property: 'og:image:width', content: '1200'},
    {property: 'og:image:height', content: '630'},
    {name: 'twitter:card', content: 'summary_large_image'},
    {name: 'twitter:image', content: OG_IMAGE},
    {name: 'twitter:title', content: 'Nebula — 多云对象存储管理器'},
  ],
})
</script>
