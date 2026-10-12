#!/bin/sh
set -eu

unset CDPATH
HERE="$(cd -- "$(dirname -- "$0")" && pwd)"
VERSION="$(tr -d '[:space:]' < "$HERE/../warp-ztna/VERSION")"
APP=""
NODE_PKG=""
ARCH=""
NOTES="$HERE/dmg/安装说明.txt"
OUT_DIR="$HERE/../../dist/client-macos"
VOLNAME="Warp ZTNA $VERSION"
DRY_RUN=0

usage() {
  cat <<'USAGE'
用法：build-dmg.sh --app <Warp ZTNA.app> --nodepkg <WarpZTNA-Service-<v>-<arch>.pkg> --arch <arm64|amd64> [选项]

选项：
  --app <PATH>       Tauri 产出的 Warp ZTNA.app
  --nodepkg <PATH>   build-macpkg.sh 产出的引擎包
  --arch <ARCH>      arm64 或 amd64
  --notes <PATH>     安装说明文件，默认 agent/warp-ztna-app/dmg/安装说明.txt
  --out <DIR>        产物输出目录，默认 dist/client-macos
  --volname <NAME>   卷名，默认 Warp ZTNA <包版本>
  --dry-run          只打印计划，不读写产物
  --help             显示本帮助

产物：WarpZTNA-App-<包版本>-<arch>.dmg（未签名；内含 app + Service 包 + 安装说明 + Applications 软链）
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
      --app)
        [ $# -ge 2 ] || die "--app 缺少取值"
        APP="$2"
        shift 2
        ;;
      --nodepkg)
        [ $# -ge 2 ] || die "--nodepkg 缺少取值"
        NODE_PKG="$2"
        shift 2
        ;;
      --arch)
        [ $# -ge 2 ] || die "--arch 缺少取值"
        ARCH="$2"
        shift 2
        ;;
      --notes)
        [ $# -ge 2 ] || die "--notes 缺少取值"
        NOTES="$2"
        shift 2
        ;;
      --out)
        [ $# -ge 2 ] || die "--out 缺少取值"
        OUT_DIR="$2"
        shift 2
        ;;
      --volname)
        [ $# -ge 2 ] || die "--volname 缺少取值"
        VOLNAME="$2"
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
  [ -n "$APP" ] || die "需要 --app <Warp ZTNA.app>"
  [ -n "$NODE_PKG" ] || die "需要 --nodepkg <PATH>"
  [ -n "$ARCH" ] || die "需要 --arch <arm64|amd64>"
  case "$ARCH" in
    arm64|amd64) ;;
    *) die "--arch 只能是 arm64 或 amd64" ;;
  esac
  DMG_NAME="WarpZTNA-App-${VERSION}-${ARCH}.dmg"
}

print_plan() {
  printf '[dry-run] ditto %s <stage>/Warp ZTNA.app\n' "$APP"
  printf '[dry-run] cp %s <stage>/\n' "$NODE_PKG"
  printf '[dry-run] cp %s <stage>/安装说明.txt\n' "$NOTES"
  printf '[dry-run] ln -s /Applications <stage>/Applications\n'
  printf '[dry-run] xattr -cr <stage>\n'
  printf '[dry-run] hdiutil create -volname "%s" -fs HFS+ -format ULFO -ov %s/%s\n' "$VOLNAME" "$OUT_DIR" "$DMG_NAME"
}

verify_app() {
  [ -d "$APP" ] || die "找不到 app：$APP"
  codesign --verify --deep --strict "$APP" >/dev/null 2>&1 || die "app 未通过签名校验：${APP}（tauri 的 ad-hoc 签名没有生效？）"
}

verify_node_pkg() {
  [ -f "$NODE_PKG" ] || die "找不到引擎包：$NODE_PKG"
  case "${NODE_PKG##*/}" in
    *-"$ARCH".pkg) ;;
    *) die "引擎包与 --arch 不匹配：$NODE_PKG 不是 $ARCH 的包" ;;
  esac
}

stage() {
  STAGE="$(mktemp -d)"
  trap 'rm -rf "$STAGE"' EXIT INT TERM
  ditto "$APP" "$STAGE/Warp ZTNA.app"
  cp "$NODE_PKG" "$STAGE/"
  cp "$NOTES" "$STAGE/安装说明.txt"
  ln -s /Applications "$STAGE/Applications"
  xattr -cr "$STAGE"
}

make_dmg() {
  mkdir -p "$OUT_DIR"
  hdiutil create -volname "$VOLNAME" -srcfolder "$STAGE" -fs HFS+ -format ULFO -ov "$OUT_DIR/$DMG_NAME"
  xattr -cr "$OUT_DIR/$DMG_NAME"
  hdiutil verify "$OUT_DIR/$DMG_NAME" >/dev/null
}

main() {
  parse_args "$@"
  if [ "$DRY_RUN" -eq 1 ]; then
    print_plan
    exit 0
  fi
  [ "$(uname -s)" = "Darwin" ] || die "本脚本只能在 macOS 上运行"
  need_cmd ditto
  need_cmd hdiutil
  need_cmd codesign
  verify_app
  verify_node_pkg
  stage
  make_dmg
  printf '产出 %s/%s\n' "$OUT_DIR" "$DMG_NAME"
}

main "$@"
