#!/bin/sh
set -eu

unset CDPATH
HERE="$(cd -- "$(dirname -- "$0")" && pwd)"
UPSTREAM=""
OUT_DIR="$HERE/../../dist/client-archives"
DRY_RUN=0

usage() {
  cat <<'USAGE'
用法：build-archives.sh --upstream <DIR> [选项]

选项：
  --upstream <DIR>   官方 netbird 归档所在目录（netbird_<版本>_<os>_<arch>.tar.gz）
  --out <DIR>        产物输出目录，默认 dist/client-archives
  --dry-run          只打印计划，不读写产物
  --help             显示本帮助

产物：warp-ztna_<包版本>_<os>_<arch>.tar.gz ×4 + checksums.txt
USAGE
}

die() {
  printf '错误：%s\n' "$*" >&2
  exit 1
}

parse_args() {
  while [ $# -gt 0 ]; do
    case "$1" in
      --upstream)
        [ $# -ge 2 ] || die "--upstream 缺少取值"
        UPSTREAM="$2"
        shift 2
        ;;
      --out)
        [ $# -ge 2 ] || die "--out 缺少取值"
        OUT_DIR="$2"
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
  [ -n "$UPSTREAM" ] || die "需要 --upstream <DIR>"
}

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

VERSION="$(tr -d '[:space:]' < "$HERE/VERSION")"
NETBIRD_VERSION="$(tr -d '[:space:]' < "$HERE/NETBIRD_VERSION")"

parse_args "$@"

if [ "$DRY_RUN" -eq 0 ]; then
  [ -d "$UPSTREAM" ] || die "找不到归档目录：$UPSTREAM"
  mkdir -p "$OUT_DIR"
fi

for pair in linux_amd64 linux_arm64 darwin_amd64 darwin_arm64 windows_amd64 windows_arm64; do
  os="${pair%%_*}"
  arch="${pair#*_}"
  upstream="$UPSTREAM/netbird_${NETBIRD_VERSION}_${os}_${arch}.tar.gz"
  expected="$(sed -n "s/^${pair}=//p" "$HERE/NETBIRD_SHA256")"
  artifact="warp-ztna_${VERSION}_${os}_${arch}.tar.gz"
  [ -n "$expected" ] || die "NETBIRD_SHA256 缺少 $pair"
  if [ "$DRY_RUN" -eq 1 ]; then
    printf '[dry-run] %s：校验 sha256=%s → %s/%s\n' "$upstream" "$expected" "$OUT_DIR" "$artifact"
    continue
  fi
  [ -f "$upstream" ] || die "缺少上游归档：$upstream"
  actual="$(sha256_of "$upstream")"
  [ "$actual" = "$expected" ] || die "上游归档校验失败：$upstream 期望 ${expected}，实际 $actual"

  STAGE="$(mktemp -d)"
  tar -xzf "$upstream" -C "$STAGE"
  bin="netbird"
  if [ -f "$STAGE/netbird.exe" ]; then bin="netbird.exe"; fi
  [ -f "$STAGE/$bin" ] || die "上游归档里没有 netbird：$upstream"
  cp "$HERE/README.md" "$STAGE/README.md"
  chmod 0755 "$STAGE/$bin"

  set -- "$bin" README.md
  for license in LICENSES LICENSE LICENSE.txt COPYING; do
    if [ -e "$STAGE/$license" ]; then
      set -- "$@" "$license"
    fi
  done
  tar -czf "$OUT_DIR/$artifact" -C "$STAGE" "$@"
  rm -rf "$STAGE"
  printf '产出 %s\n' "$artifact"
done

if [ "$DRY_RUN" -eq 1 ]; then
  printf '[dry-run] 汇总 %s/checksums.txt\n' "$OUT_DIR"
  exit 0
fi

CHECKSUMS="$OUT_DIR/checksums.txt"
: > "$CHECKSUMS"
for artifact in "$OUT_DIR"/warp-ztna_*.tar.gz; do
  [ -f "$artifact" ] || continue
  name="${artifact##*/}"
  printf '%s  %s\n' "$(sha256_of "$artifact")" "$name" >> "$CHECKSUMS"
done
printf '产出 %s\n' "$CHECKSUMS"
