#!/usr/bin/env bash
#
# 升级 Nebula 版本号。改 4 处版本号并同步两个 Cargo.lock。
# 内部依赖用 `version = "1"`(见各 crate Cargo.toml),所以不用逐 crate 改。
#
# 用法:
#   scripts/bump-version.sh <X.Y.Z>        # 如 scripts/bump-version.sh 1.3.0
#
set -euo pipefail

VERSION="${1:-}"
if ! printf '%s' "$VERSION" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$'; then
  echo "用法: $0 <X.Y.Z>   (如 $0 1.3.0)" >&2
  exit 1
fi

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

# 1) Rust workspace 包版本 —— 覆盖所有厂商 SDK / core / provider(version.workspace = true)
perl -i -pe 's/^version = "\d+\.\d+\.\d+"$/version = "'"$VERSION"'"/' Cargo.toml
# 2) Tauri app crate 版本
perl -i -pe 's/^version = "\d+\.\d+\.\d+"$/version = "'"$VERSION"'"/' app/src-tauri/Cargo.toml
# 3) tauri.conf.json
perl -i -pe 's/"version": "\d+\.\d+\.\d+"/"version": "'"$VERSION"'"/' app/src-tauri/tauri.conf.json
# 4) app 前端 package.json
perl -i -pe 's/"version": "\d+\.\d+\.\d+"/"version": "'"$VERSION"'"/' app/package.json

echo "版本号已改为 $VERSION,同步 Cargo.lock…"
cargo update --workspace --quiet
( cd app/src-tauri && cargo update --workspace --quiet )

echo
echo "完成。改动:"
git -C "$ROOT" status --porcelain -- Cargo.toml Cargo.lock app/package.json app/src-tauri/Cargo.toml app/src-tauri/Cargo.lock app/src-tauri/tauri.conf.json
echo
echo "接下来:git add -A && git commit -m \"chore: bump version to $VERSION\""
