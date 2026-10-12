#!/bin/sh
set -eu

unset CDPATH
HERE="$(cd -- "$(dirname -- "$0")" && pwd)"
VERSION="$(tr -d '[:space:]' < "$HERE/VERSION")"
ARCHIVE=""
ARCH=""
OUT_DIR="$HERE/../../dist/client-macos"
DRY_RUN=0

usage() {
  cat <<'USAGE'
用法：build-macpkg.sh --archive <PATH> --arch <arm64|amd64> [选项]

选项：
  --archive <PATH>   build-archives.sh 产出的 darwin 归档
  --arch <ARCH>      arm64 或 amd64
  --out <DIR>        产物输出目录，默认 dist/client-macos
  --dry-run          只打印计划，不读写产物
  --help             显示本帮助

产物：WarpZTNA-Service-<包版本>-<arch>.pkg（未签名；载荷二进制补 ad-hoc 签名）
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
      --archive)
        [ $# -ge 2 ] || die "--archive 缺少取值"
        ARCHIVE="$2"
        shift 2
        ;;
      --arch)
        [ $# -ge 2 ] || die "--arch 缺少取值"
        ARCH="$2"
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
  [ -n "$ARCHIVE" ] || die "需要 --archive <PATH>"
  [ -n "$ARCH" ] || die "需要 --arch <arm64|amd64>"
  case "$ARCH" in
    arm64) HOST_ARCH="arm64" ;;
    amd64) HOST_ARCH="x86_64" ;;
    *) die "--arch 只能是 arm64 或 amd64" ;;
  esac
  PKG_NAME="WarpZTNA-Service-${VERSION}-${ARCH}.pkg"
}

print_plan() {
  printf '[dry-run] codesign --force --sign - <payload>/netbird\n'
  printf '[dry-run] pkgbuild --root <stage>/root --identifier com.dy-sec.warpztna --version %s --install-location / --ownership recommended --scripts %s/pkg/scripts %s\n' "$VERSION" "$HERE" "<stage>/component.pkg"
  printf '[dry-run] cp %s/pkg/Readme.html <stage>/Readme.html\n' "$HERE"
  printf '[dry-run] productbuild --distribution <stage>/Distribution.xml --package-path <stage> --resources <stage> %s/%s\n' "$OUT_DIR" "$PKG_NAME"
  printf '[dry-run] 载荷来源 %s，hostArchitectures %s\n' "$ARCHIVE" "$HOST_ARCH"
}

stage_payload() {
  ROOT="$STAGE/root"
  PAYLOAD="$ROOT/usr/local/lib/warp-ztna"
  mkdir -p "$PAYLOAD"
  tar -xzf "$ARCHIVE" -C "$PAYLOAD"
  [ -f "$PAYLOAD/netbird" ] || die "归档里没有 netbird：$ARCHIVE"
  chmod 0755 "$PAYLOAD/netbird"
  find "$PAYLOAD" -type f ! -name netbird -exec chmod 0644 {} +
  chmod 0755 "$PAYLOAD"
  xattr -cr "$ROOT"
}

sign_payload() {
  codesign --force --sign - "$PAYLOAD/netbird"
  codesign --verify --strict --verbose=2 "$PAYLOAD/netbird"
}

render_distribution() {
  sed -e "s/@VERSION@/$VERSION/g" -e "s/@ARCH@/$HOST_ARCH/g" "$HERE/pkg/Distribution.xml" > "$STAGE/Distribution.xml"
  cp "$HERE/pkg/Readme.html" "$STAGE/Readme.html"
}

build_component() {
  pkgbuild --root "$ROOT" --identifier com.dy-sec.warpztna --version "$VERSION" \
    --install-location / --ownership recommended --scripts "$HERE/pkg/scripts" "$STAGE/component.pkg"
}

build_product() {
  mkdir -p "$OUT_DIR"
  productbuild --distribution "$STAGE/Distribution.xml" --package-path "$STAGE" --resources "$STAGE" "$OUT_DIR/$PKG_NAME"
  xattr -cr "$OUT_DIR/$PKG_NAME"
}

main() {
  parse_args "$@"
  if [ "$DRY_RUN" -eq 1 ]; then
    print_plan
    exit 0
  fi
  [ "$(uname -s)" = "Darwin" ] || die "本脚本只能在 macOS 上运行"
  need_cmd tar
  need_cmd codesign
  need_cmd pkgbuild
  need_cmd productbuild
  [ -f "$ARCHIVE" ] || die "找不到归档：$ARCHIVE"
  STAGE="$(mktemp -d)"
  trap 'rm -rf "$STAGE"' EXIT INT TERM
  stage_payload
  sign_payload
  render_distribution
  build_component
  build_product
  printf '产出 %s/%s\n' "$OUT_DIR" "$PKG_NAME"
}

main "$@"
