#!/usr/bin/env bash
#
# 一键发布 Nebula 新版本。
#
# 做什么:预检 → 推送当前分支 → 打 tag `vX.Y.Z` 并推送 → 触发
# .github/workflows/release.yml(多平台构建 + 签名 updater 产物 + 建**草稿** Release)。
# 加 --publish 时,等构建成功后把草稿转为正式发布(此后 latest.json 生效,自动更新可用)。
#
# 用法:
#   scripts/release.sh                 # 用 tauri.conf.json 里的当前版本发布
#   scripts/release.sh 1.4.0           # 指定版本发布
#   scripts/release.sh 1.4.0 --watch   # 打完 tag 后跟踪构建日志
#   scripts/release.sh 1.4.0 --publish # 构建成功后自动把草稿转正式发布(含 --watch)
#   scripts/release.sh -y 1.4.0        # 跳过确认
#
# 前置条件:
#   - 版本号已改好(scripts/bump-version.sh <X.Y.Z>)且已提交
#   - 发布日志 docs/src/content/release/<X.Y.Z>.md 已写好且已提交
#   - 仓库 Secrets 配好 TAURI_SIGNING_PRIVATE_KEY / _PASSWORD(否则 updater 产物不签名)
#
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

# ── 解析参数 ───────────────────────────────────────────────────────────────
VERSION=""
ASSUME_YES=0
WATCH=0
PUBLISH=0
ALLOW_DIRTY=0

usage() {
  sed -n '3,19p' "$0" | sed 's/^# \{0,1\}//'
  exit "${1:-0}"
}

while [ $# -gt 0 ]; do
  case "$1" in
    -y|--yes)          ASSUME_YES=1 ;;
    --watch)           WATCH=1 ;;
    --publish)         PUBLISH=1; WATCH=1 ;;
    --allow-dirty)     ALLOW_DIRTY=1 ;;
    -h|--help)         usage 0 ;;
    -*)                echo "未知参数:$1" >&2; usage 1 ;;
    *)
      if [ -n "$VERSION" ]; then echo "只接受一个版本参数" >&2; usage 1; fi
      VERSION="$1"
      ;;
  esac
  shift
done

die() { echo "✗ $*" >&2; exit 1; }
note() { echo "→ $*"; }

# ── 版本号:参数优先,否则取 tauri.conf.json 当前值 ─────────────────────────
conf_version() {
  grep -m1 '"version"' app/src-tauri/tauri.conf.json \
    | sed -E 's/.*"version"[[:space:]]*:[[:space:]]*"([^"]+)".*/\1/'
}
[ -n "$VERSION" ] || VERSION="$(conf_version)"
printf '%s' "$VERSION" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$' \
  || die "版本号非法:'$VERSION'(应形如 1.4.0)"
TAG="v$VERSION"

# ── 预检 ───────────────────────────────────────────────────────────────────
# 1) 四处版本号必须一致,且等于要发布的版本
check_field() { # <file> <grep-pattern> <sed-extract>
  local got; got="$(grep -m1 "$2" "$1" | sed -E "$3")"
  [ "$got" = "$VERSION" ] || die "版本不一致:$1 是 $got,期望 $VERSION(先跑 scripts/bump-version.sh $VERSION)"
}
check_field app/src-tauri/tauri.conf.json '"version"' 's/.*"version"[[:space:]]*:[[:space:]]*"([^"]+)".*/\1/'
check_field app/package.json              '"version"' 's/.*"version"[[:space:]]*:[[:space:]]*"([^"]+)".*/\1/'
check_field Cargo.toml                     '^version = ' 's/^version = "([^"]+)".*/\1/'
check_field app/src-tauri/Cargo.toml       '^version = ' 's/^version = "([^"]+)".*/\1/'

# 2) 发布日志必须存在且已被 git 跟踪(打 tag 后 CI 检出该 commit 时要读它当 Release 正文)
NOTES="docs/src/content/release/${VERSION}.md"
[ -f "$NOTES" ] || die "缺少发布日志:$NOTES(先写好发布说明)"
git ls-files --error-unmatch "$NOTES" >/dev/null 2>&1 \
  || die "$NOTES 尚未提交;请先 git add && git commit 后再发布"

# 3) 工作区必须干净(发布须可从已提交状态复现)
if [ "$ALLOW_DIRTY" -eq 0 ] && [ -n "$(git status --porcelain)" ]; then
  die "工作区有未提交改动;请先提交或 stash(如确需忽略,加 --allow-dirty)"
fi

# 4) tag 不能已存在(本地或远端)
git rev-parse -q --verify "refs/tags/$TAG" >/dev/null && die "本地已存在 tag $TAG"
if git ls-remote --exit-code --tags origin "refs/tags/$TAG" >/dev/null 2>&1; then
  die "远端已存在 tag $TAG(该版本已发过?)"
fi

BRANCH="$(git rev-parse --abbrev-ref HEAD)"

# 5) 签名 Secret 提示(仅提示,不阻断——列 secret 需要管理员权限,可能失败)
if command -v gh >/dev/null 2>&1 && gh auth status >/dev/null 2>&1; then
  if secrets="$(gh secret list 2>/dev/null)"; then
    printf '%s' "$secrets" | grep -q TAURI_SIGNING_PRIVATE_KEY \
      || echo "⚠ 未检测到 Secret TAURI_SIGNING_PRIVATE_KEY;updater 产物将不被签名,自动更新会失效。"
  fi
else
  [ "$PUBLISH" -eq 1 ] && die "--publish 需要已登录的 gh CLI(gh auth login)"
  [ "$WATCH" -eq 1 ] && echo "⚠ 未检测到已登录的 gh,--watch 将不可用。"
fi

# ── 确认 ───────────────────────────────────────────────────────────────────
echo
echo "即将发布:"
echo "  版本   $VERSION  (tag $TAG)"
echo "  分支   $BRANCH → origin/$BRANCH"
echo "  正文   $NOTES"
echo "  动作   推分支 + 打 tag → 触发 CI 多平台构建,建草稿 Release$([ "$PUBLISH" -eq 1 ] && echo ',构建成功后自动转正式发布')"
echo
if [ "$ASSUME_YES" -eq 0 ]; then
  printf "确认发布?[y/N] "
  read -r reply
  case "$reply" in [yY]|[yY][eE][sS]) ;; *) die "已取消" ;; esac
fi

# ── 执行 ───────────────────────────────────────────────────────────────────
note "推送分支 $BRANCH…"
git push origin "$BRANCH"

note "打 tag $TAG 并推送…"
git tag -a "$TAG" -m "Nebula $TAG"
git push origin "$TAG"

echo "✓ 已推送 tag $TAG,CI 发布流程已触发。"
echo "  Actions: https://github.com/devlive-community/nebula/actions/workflows/release.yml"

# ── 可选:跟踪构建 / 自动转正式发布 ────────────────────────────────────────
if [ "$WATCH" -eq 1 ] && command -v gh >/dev/null 2>&1 && gh auth status >/dev/null 2>&1; then
  note "等待 CI 登记本次运行…"
  run_id=""
  for _ in $(seq 1 20); do
    run_id="$(gh run list --workflow release.yml --branch "$TAG" \
              --limit 1 --json databaseId -q '.[0].databaseId' 2>/dev/null || true)"
    [ -n "$run_id" ] && break
    sleep 3
  done
  if [ -z "$run_id" ]; then
    echo "⚠ 未能定位到运行,请到 Actions 页面手动查看。"
  else
    note "跟踪运行 #$run_id(Ctrl-C 可退出跟踪,不影响 CI)…"
    if gh run watch "$run_id" --exit-status; then
      echo "✓ 构建成功。"
      if [ "$PUBLISH" -eq 1 ]; then
        note "把草稿 Release $TAG 转为正式发布…"
        gh release edit "$TAG" --draft=false --latest
        echo "✓ $TAG 已正式发布:https://github.com/devlive-community/nebula/releases/tag/$TAG"
        echo "  自动更新已对存量用户生效(latest.json 已可访问)。"
      else
        echo "→ 到 Releases 页面复核草稿 Nebula $TAG,确认无误后点 Publish。"
      fi
    else
      die "CI 构建失败;请查看日志:gh run view $run_id --log-failed"
    fi
  fi
else
  echo
  echo "接下来:"
  echo "  1. 在 Actions 里等三平台(macOS/Linux/Windows)构建完成"
  echo "  2. 打开草稿 Release「Nebula $TAG」复核安装包与说明"
  echo "  3. 点 Publish release —— 发布后 latest.json 生效,自动更新对存量用户可用"
  echo
  echo "  (下次可用 scripts/release.sh $VERSION --publish 让脚本等构建成功后自动转正式发布)"
fi
