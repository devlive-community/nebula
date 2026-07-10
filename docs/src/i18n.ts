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
      subtitle: '跨平台桌面应用 —— 浏览、上传下载、分享、传输管理,一站搞定。Rust + Tauri 打造,原生轻量。',
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
        multicloud: {title: '多云统一', desc: '一套界面管理对象存储;新增厂商实现一个接口即可,界面零改动。'},
        browse: {title: '高效浏览', desc: '目录逐层展开、列表 / 网格视图、排序过滤、图片视频预览、对象详情一目了然。'},
        upload: {title: '上传无忧', desc: '拖拽文件或整个文件夹、多选上传;大文件并发分片、断点续传,进度、并发、失败重试尽在掌握。'},
        download: {title: '流式下载', desc: '边下边写,大文件不占内存;支持断点续传、整文件夹递归下载,每个任务独立进度,可设全局带宽限速。'},
        migrate: {title: '跨云迁移', desc: '把对象或整个文件夹从一个账号搬到另一个账号,任意云到任意云;同账号走服务端复制,跨账号自动中转。'},
        integrity: {title: '完整性校验', desc: '下载内容算 MD5 与远端 ETag 比对,一键确认文件是否在传输中损坏;逻辑只依赖 ETag,对每家云通用。'},
        share: {title: '一键分享', desc: '为对象生成预签名临时链接,有效期可配置,复制即分享。'},
        secure: {title: '安全省心', desc: '密钥存入系统钥匙串,元信息与设置存本地 SQLite,明暗主题、快捷键、右键菜单俱全。'},
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
      subtitle: 'A cross-platform desktop app — browse, upload, download, share and manage transfers in one place. Built with Rust + Tauri, native and lightweight.',
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
        multicloud: {title: 'Multi-cloud', desc: 'Manage object storage from one UI; add a vendor by implementing a single trait — no UI changes.'},
        browse: {title: 'Fast browsing', desc: 'Drill into folders, list / grid views, sort and filter, image & video preview, object details at a glance.'},
        upload: {title: 'Effortless upload', desc: 'Drag files or whole folders, multi-select; large files upload in concurrent chunks, resume after interruption, with progress and retries.'},
        download: {title: 'Streaming download', desc: 'Stream to disk without buffering huge files; resumable downloads, whole-folder recursive download, per-task progress, and an optional global bandwidth cap.'},
        migrate: {title: 'Cross-cloud migration', desc: 'Move objects or entire folders from one account to another, any cloud to any cloud; same-account uses server-side copy, cross-account relays automatically.'},
        integrity: {title: 'Integrity check', desc: 'Hash the downloaded bytes and compare against the remote ETag to confirm a file survived transfer intact; keyed only on ETag, so it works for every cloud.'},
        share: {title: 'One-click share', desc: 'Generate presigned temporary links with a configurable expiry — copy and share.'},
        secure: {title: 'Secure by design', desc: 'Secrets in the system keyring, metadata and settings in local SQLite; theming, shortcuts and context menus included.'},
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
