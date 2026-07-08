<template>
  <div class="relative mx-auto h-[380px] w-full max-w-md">
    <!-- 连接线 -->
    <svg class="absolute inset-0 h-full w-full" preserveAspectRatio="none">
      <line v-for="n in nodes" :key="n.name"
            x1="50%" y1="50%" :x2="`${n.x}%`" :y2="`${n.y}%`"
            :stroke="n.active ? 'rgba(129,140,248,.7)' : 'rgba(148,163,184,.25)'"
            stroke-width="1.5" :stroke-dasharray="n.active ? '0' : '4 4'"/>
    </svg>

    <!-- 中心:Nebula -->
    <div class="absolute left-1/2 top-1/2 -translate-x-1/2 -translate-y-1/2 flex flex-col items-center gap-2">
      <div class="grid place-items-center h-20 w-20 rounded-2xl bg-white/5 ring-1 ring-white/15 backdrop-blur shadow-glow">
        <img src="/logo.svg" alt="Nebula" class="h-12 w-12"/>
      </div>
      <span class="text-sm font-semibold text-white">Nebula</span>
    </div>

    <!-- 各家云 -->
    <div v-for="n in nodes" :key="n.name"
         class="absolute -translate-x-1/2 -translate-y-1/2 whitespace-nowrap rounded-full border px-3 py-1.5 text-xs backdrop-blur"
         :class="n.active
           ? 'border-brand-400/60 bg-brand-500/15 text-brand-100'
           : 'border-white/10 bg-white/5 text-slate-400'"
         :style="{left: `${n.x}%`, top: `${n.y}%`}">
      <span class="inline-flex items-center gap-1.5">
        <span class="h-1.5 w-1.5 rounded-full" :class="n.active ? 'bg-brand-400' : 'bg-slate-500'"></span>
        {{ n.name }}
        <span class="ml-1 text-[10px]" :class="n.active ? 'text-brand-300/80' : 'text-slate-500'">
          {{ n.active ? '已接入' : '规划中' }}
        </span>
      </span>
    </div>
  </div>
</template>

<script setup lang="ts">
const nodes = [
  {name: '阿里云 OSS', active: true, x: 50, y: 7},
  {name: '腾讯云 COS', active: false, x: 90, y: 28},
  {name: '华为云 OBS', active: true, x: 84, y: 78},
  {name: 'AWS S3', active: false, x: 50, y: 94},
  {name: 'MinIO', active: false, x: 16, y: 78},
  {name: 'Cloudflare R2', active: false, x: 10, y: 28}
]
</script>
