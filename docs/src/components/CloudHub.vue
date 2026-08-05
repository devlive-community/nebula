<template>
  <div class="relative mx-auto h-[400px] w-full max-w-md">
    <!-- 连接线 -->
    <svg class="absolute inset-0 h-full w-full" preserveAspectRatio="none">
      <line v-for="n in placed" :key="n.name"
            x1="50%" y1="50%" :x2="`${n.x}%`" :y2="`${n.y}%`"
            stroke="rgba(129,140,248,.55)" stroke-width="1.5"/>
    </svg>

    <!-- 中心:Nebula -->
    <div class="absolute left-1/2 top-1/2 -translate-x-1/2 -translate-y-1/2 flex flex-col items-center gap-2">
      <div class="grid place-items-center h-20 w-20 rounded-2xl bg-white/5 ring-1 ring-white/15 backdrop-blur shadow-glow">
        <img src="/logo.svg" alt="Nebula" class="h-12 w-12"/>
      </div>
      <span class="text-sm font-semibold text-white">Nebula</span>
    </div>

    <!-- 各家云:统一尺寸的圆形徽章,按品牌色区分,不再用宽度随文字长度变化的胶囊 -->
    <div v-for="n in placed" :key="n.name"
         :title="n.name"
         class="absolute -translate-x-1/2 -translate-y-1/2 flex flex-col items-center gap-1"
         :style="{left: `${n.x}%`, top: `${n.y}%`}">
      <div class="grid h-14 w-14 place-items-center rounded-full text-xs font-semibold backdrop-blur"
           :style="{
             backgroundColor: `${n.color}26`,
             border: `1px solid ${n.color}80`,
             color: n.color
           }">
        {{ n.short }}
      </div>
      <span class="text-[10px] text-slate-400">{{ n.short }}</span>
    </div>
  </div>
</template>

<script setup lang="ts">
import {computed} from 'vue'

// 品牌色照抄 app/src/vendors.ts 里已经定好的值,App 内账号表单和这里用同一套配色。
// docs 是独立的 Vue 项目,不能直接 import app 的 TS 模块,这里以字面量形式重新写一份。
const nodes = [
  {name: '阿里云 OSS', short: 'OSS', color: '#ff6a00'},
  {name: '腾讯云 COS', short: 'COS', color: '#006eff'},
  {name: '华为云 OBS', short: 'OBS', color: '#c7000b'},
  {name: 'AWS S3', short: 'S3', color: '#ff9900'},
  {name: 'MinIO', short: 'Min', color: '#c72e49'},
  {name: '七牛云 Kodo', short: 'Kodo', color: '#12b5a5'},
  {name: 'Cloudflare R2', short: 'R2', color: '#f6821f'},
  {name: 'Backblaze B2', short: 'B2', color: '#e21b24'},
  {name: 'Wasabi', short: 'Was', color: '#22c02e'},
  {name: 'DigitalOcean Spaces', short: 'DO', color: '#0069ff'},
  {name: 'Scaleway Object Storage', short: 'Scw', color: '#4f0599'},
  {name: 'UCloud US3', short: 'US3', color: '#2e5bff'},
  {name: '京东云 OSS', short: 'JD', color: '#e3101e'}
]

// 均匀分布在中心 Nebula 周围(椭圆,顺时针,rx=42% ry=44%)。徽章现在是统一小尺寸的圆形,
// 不再随文字长度变宽,加几家新厂商只需要在上面的 nodes 数组里追加一项,这里的角度
// (360° / nodes.length)会自动重新摊匀,不用再逐个手调坐标。
const placed = computed(() => {
  const rx = 42
  const ry = 44
  const n = nodes.length
  return nodes.map((node, i) => {
    const theta = (i * 2 * Math.PI) / n
    return {
      ...node,
      x: 50 + rx * Math.sin(theta),
      y: 50 - ry * Math.cos(theta)
    }
  })
})
</script>
