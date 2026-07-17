import {createI18n} from 'vue-i18n'

export const SUPPORTED_LOCALES = ['zh', 'en'] as const
export type Locale = (typeof SUPPORTED_LOCALES)[number]

const messages = {
  zh: {
    nav: {download: '下载', blog: '博客', releases: '发布日志'},
    hero: {
      badge: 'v{version} 已发布',
      title1: '一个界面,管理你的',
      title2: '多云对象存储',
      subtitle: '在一个原生桌面应用里管理阿里云 OSS、腾讯云 COS、华为云 OBS、AWS S3、Cloudflare R2、七牛 Kodo、MinIO —— 浏览、上传下载、分享、跨云迁移,一站搞定。轻量、快速,macOS / Windows / Linux 原生三端。',
      download: '立即下载',
      github: '在 GitHub 查看'
    },
    preview: {
      tag: '界面预览',
      title: '像本地文件管理器一样管云',
      desc: '左侧切换云账号,右侧逐层浏览桶与目录;拖拽上传、批量下载、分片进度都在同一个窗口里完成。'
    },
    features: {
      tag: '核心优势',
      title: '一个应用,搞定云上文件',
      items: {
        multicloud: {title: '多云统一', desc: '一套界面管理多云对象存储,连 Bucket 的新建 / 删除都统一;跨区域的桶自动路由到各自的区域,不用手动切;新增厂商实现一个接口即可,界面零改动。'},
        browse: {title: '高效浏览', desc: '目录逐层展开、列表 / 网格视图、排序过滤、常去位置一键收藏跳转、Cmd/Ctrl+K 命令面板秒跳账号与收藏、直接执行刷新 / 上传 / 新建等命令(空输入先列最近访问)、Rust 加速的图片浏览器 / 编辑器(独立窗口打开单张,后端解码缩放 + 磁盘缓存;缩放 / 平移 / 旋转 / EXIF;编辑裁剪 / 旋转 / 翻转 / 灰度 / 反相 / 亮度对比度,实时预览并存回云端)、视频 / 音频 / PDF / 文本代码预览、文件夹 / Bucket 大小统计与存储类型分布;对象详情含存储类型、真实 Content-Type(可改)与可读写的对象标签,单个对象或整个文件夹都能转换存储层、取回归档。'},
        upload: {title: '上传无忧', desc: '拖拽文件或整个文件夹、多选上传;大文件并发分片、断点续传,内容未变的文件自动秒传跳过,可随时取消,进度、并发、失败重试(可一键重试全部失败)尽在掌握;还能一键清理桶里残留的未完成分片上传,回收白白计费的存储。'},
        download: {title: '流式下载', desc: '边下边写,大文件不占内存;支持断点续传、整文件夹递归下载,本地已有且一致的文件自动跳过,每个任务独立进度(含速度与剩余时间),可设全局带宽限速。'},
        migrate: {title: '跨云迁移', desc: '把对象或整个文件夹从一个账号搬到另一个账号,任意云到任意云;同账号走服务端复制,跨账号自动中转;同账号内还能整目录复制 / 移动 / 重命名到任意目录;多选对象可批量重命名(加前缀 / 后缀 / 查找替换,先预览后执行)。'},
        integrity: {title: '完整性校验', desc: '下载内容算 MD5 与远端 ETag 比对,一键确认文件是否在传输中损坏;逻辑只依赖 ETag,对每家云通用。'},
        share: {title: '一键分享', desc: '为对象生成预签名临时链接,有效期可配置,复制即分享;还能生成预签名上传链接,别人无需密钥凭链接直接 PUT 上传;或把对象设为公开读,拿一个不会过期的永久公共直链(还可给账号配自定义域名 / CDN,直链走你自己的域名)。'},
        secure: {title: '安全省心', desc: '密钥存入系统钥匙串,元信息、设置与传输列表存本地 SQLite(重启后未完成的传输可续),中英双语、明暗主题、快捷键、右键菜单俱全。'},
        update: {title: '自动更新', desc: '应用内检测新版本,一键下载安装并校验更新签名,始终用上最新特性。'}
      }
    },
    download: {
      title: '下载 Nebula',
      latestPrefix: '当前最新版本',
      latestSuffix: ',详见',
      releaseNotes: '发布说明',
      allVersions: '在 GitHub 查看所有版本'
    },
    stats: {
      vendors: '可扩展', vendorsLabel: '云厂商接入',
      transfer: '并发', transferLabel: '多任务传输',
      platforms: '3', platformsLabel: '跨平台支持',
      open: '100%', openLabel: '开源免费'
    },
    releaseList: {title: '发布日志', intro: '每个版本的更新内容如下,点击查看详情。'},
    footer: {download: '下载', blog: '博客', releases: '发布日志'},
    notFound: {
      title: '页面未找到',
      description: '抱歉,您访问的页面不存在或已被移动',
      goHome: '返回首页',
      goBack: '返回上页',
      quickLinks: '您可能想访问:'
    }
  },
  en: {
    nav: {download: 'Download', blog: 'Blog', releases: 'Releases'},
    hero: {
      badge: 'v{version} released',
      title1: 'One interface for your',
      title2: 'multi-cloud storage',
      subtitle: 'Manage Aliyun OSS, Tencent COS, Huawei OBS, AWS S3, Cloudflare R2, Qiniu Kodo and MinIO in one native desktop app — browse, upload, download, share and migrate across clouds, all in one place. Lightweight and fast, native on macOS, Windows and Linux.',
      download: 'Download now',
      github: 'View on GitHub'
    },
    preview: {
      tag: 'Preview',
      title: 'Manage the cloud like a local file manager',
      desc: 'Switch cloud accounts on the left, drill into buckets and folders on the right; drag-to-upload, batch download and chunked progress all in one window.'
    },
    features: {
      tag: 'Why Nebula',
      title: 'One app for your cloud files',
      items: {
        multicloud: {title: 'Multi-cloud', desc: 'Manage object storage from one UI, down to creating and deleting buckets; buckets in other regions are routed to their own region automatically; add a vendor by implementing a single trait — no UI changes.'},
        browse: {title: 'Fast browsing', desc: 'Drill into folders, list / grid views, sort and filter, bookmark and jump to frequent locations, a Cmd/Ctrl+K command palette to jump to any account or bookmark and run commands (recent locations listed first), a Rust-accelerated image viewer / editor that opens each image in its own window (backend decode + downscale + disk cache; zoom / pan / rotate / EXIF; edit with crop / rotate / flip / grayscale / invert / brightness / contrast and save back to the cloud), image / video / audio / PDF / text-code previews,folder / bucket size stats and a storage-class breakdown; object details incl. storage class, the real Content-Type (editable) and read/write object tags, with tier transition and archive restore for a single object or a whole folder.'},
        upload: {title: 'Effortless upload', desc: 'Drag files or whole folders, multi-select; large files upload in concurrent chunks, resume after interruption, unchanged files are skipped instantly, cancel anytime, with progress and retries (retry all failed at once); and clean up orphaned incomplete multipart uploads in a bucket to reclaim storage you are silently billed for.'},
        download: {title: 'Streaming download', desc: 'Stream to disk without buffering huge files; resumable downloads, whole-folder recursive download, files already present and identical are skipped, per-task progress (with speed and ETA), and an optional global bandwidth cap.'},
        migrate: {title: 'Cross-cloud migration', desc: 'Move objects or entire folders from one account to another, any cloud to any cloud; same-account uses server-side copy, cross-account relays automatically, and a whole folder can be copied / moved / renamed to any directory within an account; selected objects can be batch-renamed (add prefix / suffix / find & replace, previewed before applying).'},
        integrity: {title: 'Integrity check', desc: 'Hash the downloaded bytes and compare against the remote ETag to confirm a file survived transfer intact; keyed only on ETag, so it works for every cloud.'},
        share: {title: 'One-click share', desc: 'Generate presigned temporary links with a configurable expiry — copy and share; also generate presigned upload links so others can PUT a file with no credentials; or make an object public and copy a permanent, non-expiring direct link — optionally through a custom domain / CDN configured per account.'},
        secure: {title: 'Secure by design', desc: 'Secrets in the system keyring; metadata, settings and the transfer list in local SQLite (unfinished transfers resume after a restart); English / Chinese UI, theming, shortcuts and context menus included.'},
        update: {title: 'Auto-update', desc: 'Detect new versions in-app and install with one click, verified by update signatures — always on the latest.'}
      }
    },
    download: {
      title: 'Download Nebula',
      latestPrefix: 'Latest version',
      latestSuffix: '. See the',
      releaseNotes: 'release notes',
      allVersions: 'Browse all releases on GitHub'
    },
    stats: {
      vendors: 'Pluggable', vendorsLabel: 'Cloud vendors',
      transfer: 'Concurrent', transferLabel: 'Transfers',
      platforms: '3', platformsLabel: 'Platforms',
      open: '100%', openLabel: 'Open source'
    },
    releaseList: {title: 'Releases', intro: 'Update notes for each version. Click to view details.'},
    footer: {download: 'Download', blog: 'Blog', releases: 'Releases'},
    notFound: {
      title: 'Page Not Found',
      description: 'Sorry, the page you are looking for does not exist or has been moved',
      goHome: 'Go Home',
      goBack: 'Go Back',
      quickLinks: 'You might want to visit:'
    }
  }
}

const stored = (): Locale => {
  if (typeof localStorage !== 'undefined') {
    const v = localStorage.getItem('locale')
    if (v === 'zh' || v === 'en') return v
  }
  return 'zh'
}

export const i18n = createI18n({
  legacy: false,
  locale: stored(),
  fallbackLocale: 'zh',
  messages
})

export const setLocale = (l: Locale) => {
  i18n.global.locale.value = l
  if (typeof localStorage !== 'undefined') localStorage.setItem('locale', l)
  if (typeof document !== 'undefined') document.documentElement.lang = l === 'zh' ? 'zh-CN' : 'en'
}
