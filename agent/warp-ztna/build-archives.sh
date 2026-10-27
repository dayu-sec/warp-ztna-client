#!/bin/sh
set -eu

unset CDPATH
HERE="$(cd -- "$(dirname -- "$0")" && pwd)"
UPSTREAM=""
OUT_DIR="$HERE/../../dist/client-archives"
WINTUN_ZIP=""
DRY_RUN=0

usage() {
  cat <<'USAGE'
用法：build-archives.sh --upstream <DIR> --wintun-zip <FILE> [选项]

选项：
  --upstream <DIR>    官方 netbird 归档所在目录（netbird_<版本>_<os>_<arch>.tar.gz）
  --wintun-zip <FILE> wintun 发布 zip（Windows 归档必需；--dry-run 时可不给）
  --out <DIR>         产物输出目录，默认 dist/client-archives
  --dry-run           只打印计划，不读写产物
  --help              显示本帮助

产物：warp-ztna_<包版本>_<os>_<arch>.tar.gz ×6 + checksums.txt
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
      --wintun-zip)
        [ $# -ge 2 ] || die "--wintun-zip 缺少取值"
        WINTUN_ZIP="$2"
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
WINTUN_SHA="$(tr -d '[:space:]' < "$HERE/WINTUN_SHA256")"

parse_args "$@"

if [ "$DRY_RUN" -eq 0 ]; then
  [ -d "$UPSTREAM" ] || die "找不到归档目录：$UPSTREAM"
  [ -n "$WINTUN_ZIP" ] || die "非 dry-run 需要 --wintun-zip（Windows 归档要带 wintun.dll）"
  [ -f "$WINTUN_ZIP" ] || die "找不到 wintun zip：$WINTUN_ZIP"
  command -v unzip >/dev/null 2>&1 || die "缺少命令：unzip"
  wintun_actual="$(sha256_of "$WINTUN_ZIP")"
  [ "$wintun_actual" = "$WINTUN_SHA" ] || die "wintun zip 校验失败：$WINTUN_ZIP 期望 ${WINTUN_SHA}，实际 $wintun_actual"
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
    if [ "$os" = "windows" ]; then
      printf '[dry-run]   + wintun.dll（sha256=%s）\n' "$WINTUN_SHA"
    fi
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

  if [ "$os" = "windows" ]; then
    case "$arch" in
      amd64) wintun_arch=amd64 ;;
      arm64) wintun_arch=arm64 ;;
      *) die "不支持的 Windows 架构：$arch" ;;
    esac
    unzip -p "$WINTUN_ZIP" "wintun/bin/$wintun_arch/wintun.dll" > "$STAGE/wintun.dll"
    [ -s "$STAGE/wintun.dll" ] || die "wintun zip 里没有 wintun/bin/$wintun_arch/wintun.dll"
    license_entry="$(unzip -Z1 "$WINTUN_ZIP" | awk 'tolower($0) ~ /(^|\/)license\.txt$/ { print; exit }')"
    [ -n "$license_entry" ] || die "wintun zip 里没有 LICENSE.txt"
    unzip -p "$WINTUN_ZIP" "$license_entry" > "$STAGE/wintun-LICENSE.txt"
    [ -s "$STAGE/wintun-LICENSE.txt" ] || die "wintun LICENSE 解包为空"
    set -- "$bin" wintun.dll wintun-LICENSE.txt README.md
  else
    set -- "$bin" README.md
  fi
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
