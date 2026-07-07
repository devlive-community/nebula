<template>
  <div class="glow-border rounded-2xl animate-float">
    <div class="rounded-2xl border border-white/10 bg-slate-900/80 backdrop-blur-xl shadow-2xl overflow-hidden">
      <!-- 标题栏 -->
      <div class="flex items-center gap-2 px-4 h-10 border-b border-white/10 bg-white/5">
        <span class="w-3 h-3 rounded-full bg-red-400/90"></span>
        <span class="w-3 h-3 rounded-full bg-amber-400/90"></span>
        <span class="w-3 h-3 rounded-full bg-green-400/90"></span>
        <span class="ml-3 text-xs text-slate-400 font-mono">Nebula</span>
        <span class="ml-auto text-[10px] uppercase tracking-wider text-brand-300/80">connected</span>
      </div>

      <div class="flex text-[13px]">
        <!-- 侧栏 -->
        <div class="hidden sm:flex w-32 flex-col gap-1 border-r border-white/10 bg-white/5 p-3">
          <div class="flex items-center gap-2 rounded-md bg-brand-500/20 px-2 py-1.5 text-brand-200">
            <span class="h-1.5 w-1.5 rounded-full bg-brand-400"></span> aliyun-main
          </div>
          <div class="flex items-center gap-2 px-2 py-1.5 text-slate-400">
            <span class="h-1.5 w-1.5 rounded-full bg-slate-500"></span> backup
          </div>
        </div>

        <!-- 文件列表 -->
        <div class="flex-1 min-w-0">
          <div class="flex items-center gap-2 border-b border-white/10 px-4 py-2 text-slate-500">
            <span>devlive-cdn</span><span class="text-slate-600">/</span>
            <span class="text-slate-300">assets</span>
          </div>
          <div v-for="row in rows" :key="row.name"
               class="flex items-center gap-3 px-4 py-2 border-b border-white/5 last:border-0">
            <span class="w-4 text-center" :class="row.dir ? 'text-brand-300' : 'text-slate-500'" v-html="row.dir ? folder : file"></span>
            <span class="flex-1 min-w-0 truncate text-slate-200">{{ row.name }}</span>
            <span class="text-slate-500 tabular-nums">{{ row.size }}</span>
          </div>
          <!-- 传输条 -->
          <div class="px-4 py-2.5 border-t border-white/10 bg-black/20">
            <div class="flex items-center justify-between text-[11px] text-slate-400 mb-1.5">
              <span>上传 video-2026.mp4</span><span class="tabular-nums">68%</span>
            </div>
            <div class="h-1.5 rounded-full bg-white/10 overflow-hidden">
              <div class="h-full w-[68%] rounded-full bg-gradient-to-r from-brand-400 to-violet-400"></div>
            </div>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
const folder =
  '<svg class="w-4 h-4 inline" viewBox="0 0 24 24" fill="currentColor"><path d="M10 4H4a2 2 0 0 0-2 2v12a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-8l-2-2z"/></svg>'
const file =
  '<svg class="w-4 h-4 inline" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/><path d="M14 2v6h6"/></svg>'

const rows = [
  {name: 'images', dir: true, size: '—'},
  {name: 'releases', dir: true, size: '—'},
  {name: 'index.html', dir: false, size: '4.3 KB'},
  {name: 'app-bundle.js', dir: false, size: '1.2 MB'},
  {name: 'video-2026.mp4', dir: false, size: '84 MB'}
]
</script>
