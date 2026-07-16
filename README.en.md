[简体中文](README.md) · **English** · [Русский](README.ru.md) · [日本語](README.ja.md) · [한국어](README.ko.md)

# Nebula

**A cross-platform desktop manager for multi-cloud object storage** — manage Aliyun OSS, Tencent COS, Huawei OBS, AWS S3, Cloudflare R2, Qiniu Kodo and MinIO from a single native app, just like a local file manager: browse, upload, download, share and migrate across clouds, all in one place.

Built with Rust + Tauri — lightweight, fast, and native on macOS / Windows / Linux. Each cloud is backed by a hand-written, independently publishable Rust SDK, with no aggregation libraries.

> Website: <https://nebula.devlive.org/> · Download: [GitHub Releases](https://github.com/devlive-community/nebula/releases)

## Supported clouds

| Provider | Status | Signing |
|----------|--------|---------|
| Aliyun OSS | ✅ verified with a real account | V2 (HMAC-SHA1) |
| Huawei OBS | ✅ verified with a real account | V2 (HMAC-SHA1) |
| Tencent COS | ✅ | proprietary q-sign |
| Qiniu Kodo | ✅ | S3-compatible (shares `s3-core`) |
| AWS S3 | ✅ | SigV4 |
| Cloudflare R2 | ✅ | S3-compatible (SigV4) |
| MinIO | ✅ | S3-compatible (http / custom port) |

> Adding a provider = write one `<vendor>` SDK (S3-compatible ones just reuse `s3-core`) + one provider adapter, register it in `app-core` — **no UI changes needed**.

## Features

- **Browse & preview** — drill into folders, list / grid views, sort and filter; image / video / audio / PDF / text-code previews; object details with storage class, the real Content-Type (editable) and read/write object tags; folder / bucket size stats with a per-storage-class breakdown.
- **Upload & download** — drag files or folders, multi-select; concurrent multipart uploads with resume; unchanged files are skipped instantly; streaming downloads without buffering, whole-folder recursive download.
- **Migrate & organize** — move objects or whole folders across accounts and clouds (server-side copy within an account, relayed across accounts); copy / move / rename to any directory within an account; **batch-rename** selected objects (add prefix / suffix / find & replace, previewed before applying).
- **Share & publish** — presigned download / upload links; make an object public and get a **permanent direct link**, optionally through a custom domain / CDN configured per account.
- **Search & batch** — recursive in-bucket search (by name / size / type), multi-select results to change storage class / restore / delete / download; buckets in other regions are routed to their own region automatically.
- **Navigation & productivity** — a **Cmd/Ctrl+K command palette** (type to jump to accounts / bookmarks or run commands), **bookmarks**, **recent locations**; English / Chinese UI, light / dark themes, shortcuts and context menus.
- **Transfer management** — concurrency control, global rate limit, speed / ETA, cancel / resume, restart recovery, retry-all-failed.
- **Storage hygiene** — clean up orphaned incomplete multipart uploads to reclaim silently-billed storage; transition tiers or restore archives for a single object or a whole folder.
- **Accounts & security** — secrets in the system keyring; metadata, settings, bookmarks, recents and UI preferences persisted to local SQLite (across dedicated tables; unfinished transfers resume after a restart).
- **Auto-update** — detect new versions in-app and install with one click, verified by update signatures.

## Architecture & development

Rust + Tauri 2.0, a monorepo with a strictly one-way three-layer architecture: `cloud-core → per-vendor SDKs → provider adapters → app`. Secrets live in the system keyring; async is powered by tokio.

For the full directory layout, SDK playbook and per-vendor spec sheets, see the [primary README (简体中文)](README.md) and [`docs/`](./docs). Running and packaging the app: [app/README.md](./app/README.md).

## License

[MIT](./LICENSE)
