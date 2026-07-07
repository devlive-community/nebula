import { Logo } from "./Logo";

const REPO = "https://github.com/devlive-community/nebula";
const RELEASES = `${REPO}/releases/latest`;

const features = [
  {
    title: "多云统一",
    desc: "一套界面管理阿里云 OSS 等对象存储;新增厂商实现一个接口即可,界面零改动。",
  },
  {
    title: "浏览高效",
    desc: "目录逐层展开、列表 / 网格视图、排序过滤、图片视频预览、对象详情一目了然。",
  },
  {
    title: "上传无忧",
    desc: "拖拽文件或整个文件夹、多选上传;大文件自动分片,进度、并发、失败重试尽在掌握。",
  },
  {
    title: "流式下载",
    desc: "边下边写,大文件不占内存;批量下载到指定目录,每个任务独立进度。",
  },
  {
    title: "一键分享",
    desc: "为对象生成预签名临时链接,有效期可配置,复制即分享。",
  },
  {
    title: "安全省心",
    desc: "账号密钥存入系统钥匙串,元信息与设置存本地 SQLite,明暗主题、快捷键、右键菜单俱全。",
  },
];

const platforms = [
  { name: "macOS", note: "Apple Silicon / Intel(.dmg)" },
  { name: "Windows", note: "64 位(.msi / .exe)" },
  { name: "Linux", note: "AppImage / .deb" },
];

export default function App() {
  return (
    <div className="site">
      <header className="nav">
        <a className="nav__brand" href="#top">
          <Logo size={28} />
          <span>Nebula</span>
        </a>
        <nav className="nav__links">
          <a href="#features">功能</a>
          <a href="#download">下载</a>
          <a href={REPO} target="_blank" rel="noreferrer">
            GitHub
          </a>
        </nav>
      </header>

      <main>
        <section className="hero" id="top">
          <div className="hero__glow" />
          <Logo size={92} />
          <h1 className="hero__title">Nebula</h1>
          <p className="hero__tagline">一个界面,管理你的多云对象存储</p>
          <p className="hero__sub">
            跨平台桌面应用 —— 浏览、上传下载、分享、传输管理,一站搞定。
            Rust + Tauri 打造,原生轻量。
          </p>
          <div className="hero__actions">
            <a className="btn btn--primary" href={RELEASES}>
              下载最新版
            </a>
            <a className="btn" href={REPO} target="_blank" rel="noreferrer">
              查看源码
            </a>
          </div>
          <div className="hero__platforms">支持 macOS · Windows · Linux</div>
        </section>

        <section className="features" id="features">
          <h2 className="section__title">为什么选择 Nebula</h2>
          <div className="features__grid">
            {features.map((f) => (
              <div className="feature" key={f.title}>
                <h3>{f.title}</h3>
                <p>{f.desc}</p>
              </div>
            ))}
          </div>
        </section>

        <section className="download" id="download">
          <h2 className="section__title">下载 Nebula</h2>
          <p className="download__hint">选择你的平台,或前往 GitHub Releases 获取全部安装包。</p>
          <div className="download__grid">
            {platforms.map((p) => (
              <a className="platform" key={p.name} href={RELEASES}>
                <span className="platform__name">{p.name}</span>
                <span className="platform__note">{p.note}</span>
                <span className="platform__cta">前往下载 →</span>
              </a>
            ))}
          </div>
        </section>
      </main>

      <footer className="footer">
        <div className="footer__brand">
          <Logo size={20} />
          <span>Nebula</span>
        </div>
        <div className="footer__meta">
          <a href={REPO} target="_blank" rel="noreferrer">
            GitHub
          </a>
          <span>MIT OR Apache-2.0</span>
        </div>
      </footer>
    </div>
  );
}
