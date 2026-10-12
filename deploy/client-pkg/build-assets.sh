#!/bin/sh
set -eu

unset CDPATH
HERE="$(cd -- "$(dirname -- "$0")" && pwd)"
VERSION="$(tr -d '[:space:]' < "$HERE/../../agent/warp-ztna/VERSION")"
DIST=""
OUT=""
DRY_RUN=0

usage() {
  cat <<'USAGE'
用法：build-assets.sh --dist <DIR> --out <DIR> [选项]

把客户端 release 产物收拢为 GitHub Release 的扁平资产目录：
归档、各平台安装包、四个安装脚本，以及合并全部产物的 checksums.txt。

选项：
  --dist <DIR>        CI 的 dist 目录（含 client-archives / client-macos / client-windows / client-linux / client-pkg）
  --out <DIR>         资产输出目录（扁平）
  --version <x.y.z>   包版本，默认读 agent/warp-ztna/VERSION
  --dry-run           只打印计划，不读写产物
  --help              显示本帮助
USAGE
}

die() {
  printf '错误：%s\n' "$*" >&2
  exit 1
}

need_cmd() {
  command -v "$1" >/dev/null 2>&1 || die "缺少命令：$1"
}

parse_args() {
  while [ $# -gt 0 ]; do
    case "$1" in
      --dist)
        [ $# -ge 2 ] || die "--dist 缺少取值"
        DIST="$2"
        shift 2
        ;;
      --out)
        [ $# -ge 2 ] || die "--out 缺少取值"
        OUT="$2"
        shift 2
        ;;
      --version)
        [ $# -ge 2 ] || die "--version 缺少取值"
        VERSION="$2"
        shift 2
        ;;
      --dry-run)
        DRY_RUN=1
        shift
        ;;
      --help|-h)
        usage
        exit 0
        ;;
      *)
        usage >&2
        die "未知参数：$1"
        ;;
    esac
  done
  [ -n "$DIST" ] || die "需要 --dist <DIR>"
  [ -n "$OUT" ] || die "需要 --out <DIR>"
  [ -n "$VERSION" ] || die "版本为空"
}

version_from_archives() {
  for f in "$DIST"/client-archives/warp-ztna_*_*_*.tar.gz; do
    [ -f "$f" ] || continue
    printf '%s\n' "$(basename -- "$f")" | sed -n 's/^warp-ztna_\([^_]*\)_[^_]*_[^_]*\.tar\.gz$/\1/p'
    return 0
  done
}

print_plan() {
  printf '[dry-run] 版本 %s\n' "$VERSION"
  printf '[dry-run] 输入 %s 的 client-archives / client-macos / client-windows / client-linux / client-pkg\n' "$DIST"
  printf '[dry-run] 输出 %s（扁平资产）：\n' "$OUT"
  printf '  warp-ztna_%s_<系统>_<架构>.tar.gz ×6\n' "$VERSION"
  printf '  WarpZTNA-App-%s-<架构>.dmg、WarpZTNA-Service-%s-<架构>.pkg、WarpZTNA-App-%s-amd64.msi、.deb、.rpm\n' "$VERSION" "$VERSION" "$VERSION"
  printf '  install.sh、install-app.sh、install-app-linux.sh、install.ps1\n'
  printf '  checksums.txt（全部资产的 sha256，裸文件名）\n'
}

hash_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

place_one() {
  src="$1"
  dest="$2"
  [ -f "$src" ] || die "找不到输入文件：$src"
  cp "$src" "$dest"
}

place_matching() {
  dir="$1"
  pattern="$2"
  for f in "$dir"/$pattern; do
    [ -f "$f" ] || continue
    printf '%s\n' "$f"
  done
}

main() {
  parse_args "$@"
  if [ "$DRY_RUN" -eq 1 ]; then
    print_plan
    exit 0
  fi
  for d in client-archives client-macos client-windows client-linux client-pkg; do
    [ -d "$DIST/$d" ] || die "找不到输入目录：$DIST/$d"
  done
  derived="$(version_from_archives)"
  if [ -n "$derived" ]; then
    VERSION="$derived"
  else
    die "client-archives 里没有 warp-ztna_<版本>_<系统>_<架构>.tar.gz"
  fi
  need_cmd cp
  rm -rf "$OUT"
  mkdir -p "$OUT"

  for f in $(place_matching "$DIST/client-archives" '*.tar.gz') \
    $(place_matching "$DIST/client-macos" '*.dmg') \
    $(place_matching "$DIST/client-macos" '*.pkg') \
    $(place_matching "$DIST/client-windows" '*.msi') \
    $(place_matching "$DIST/client-linux" '*.deb') \
    $(place_matching "$DIST/client-linux" '*.rpm'); do
    place_one "$f" "$OUT/"
  done

  place_one "$DIST/client-pkg/linux/install.sh" "$OUT/install.sh"
  place_one "$DIST/client-pkg/macos/install-app.sh" "$OUT/install-app.sh"
  place_one "$DIST/client-pkg/linux/install-app.sh" "$OUT/install-app-linux.sh"
  place_one "$DIST/client-pkg/windows/install.ps1" "$OUT/install.ps1"

  : > "$OUT/checksums.txt"
  for f in "$OUT"/*; do
    name="$(basename -- "$f")"
    [ "$name" = "checksums.txt" ] && continue
    printf '%s  %s\n' "$(hash_of "$f")" "$name" >> "$OUT/checksums.txt"
  done

  printf '产出 %s（%s 个资产，%s 条校验和）\n' "$OUT" "$(find "$OUT" -type f | wc -l | tr -d ' ')" "$(wc -l < "$OUT/checksums.txt" | tr -d ' ')"
}

main "$@"
