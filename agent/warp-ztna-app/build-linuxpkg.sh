#!/bin/sh
set -eu

unset CDPATH
HERE="$(cd -- "$(dirname -- "$0")" && pwd)"
VERSION="$(tr -d '[:space:]' < "$HERE/../warp-ztna/VERSION")"
DEB=""
ARCHIVE=""
ARCH=""
OUT_DIR="$HERE/../../dist/client-linux"
DRY_RUN=0

usage() {
  cat <<'USAGE'
用法：build-linuxpkg.sh --deb <APP.deb> --archive <warp-ztna_<v>_linux_<arch>.tar.gz> --arch <amd64|arm64> [选项]

以 tauri 产出的 deb（仅桌面应用）为基，注入引擎（netbird）与维护者脚本后重打包，
再经 fpm 生成 rpm。preinst 检测到同类组网客户端时先替换它；postinst 注册并启动
warp-ztna 服务，支持 enroll.conf 预置入网。

选项：
  --deb <PATH>       tauri 产出的 deb
  --archive <PATH>   build-archives.sh 产出的 linux 归档（引擎来源）
  --arch <ARCH>      amd64 或 arm64
  --out <DIR>        产物输出目录，默认 dist/client-linux
  --dry-run          只打印计划，不读写产物
  --help             显示本帮助

产物：WarpZTNA-App-<包版本>-<arch>.deb 与 WarpZTNA-App-<包版本>-<arch>.rpm（均未签名）
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
      --deb)
        [ $# -ge 2 ] || die "--deb 缺少取值"
        DEB="$2"
        shift 2
        ;;
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
  [ -n "$DEB" ] || die "需要 --deb <PATH>"
  [ -n "$ARCHIVE" ] || die "需要 --archive <PATH>"
  [ -n "$ARCH" ] || die "需要 --arch <amd64|arm64>"
  case "$ARCH" in
    amd64) RPM_ARCH="x86_64" ;;
    arm64) RPM_ARCH="aarch64" ;;
    *) die "--arch 只能是 amd64 或 arm64" ;;
  esac
  DEB_NAME="WarpZTNA-App-${VERSION}-${ARCH}.deb"
  RPM_NAME="WarpZTNA-App-${VERSION}-${ARCH}.rpm"
}

print_plan() {
  printf '[dry-run] dpkg-deb -R %s <stage>（解开 tauri APP 包）\n' "$DEB"
  printf '[dry-run] tar -xzf %s -C <stage>/usr/local/lib/warp-ztna（注入引擎）\n' "$ARCHIVE"
  printf '[dry-run] control 追加 Conflicts: netbird, netbird-ui\n'
  printf '[dry-run] 写 <stage>/DEBIAN/{preinst,postinst,prerm}：preinst 先替换同类组网客户端；postinst 注册并启动 warp-ztna 服务；enroll.conf 预置入网\n'
  printf '[dry-run] dpkg-deb -b <stage> %s/%s\n' "$OUT_DIR" "$DEB_NAME"
  printf '[dry-run] fpm -s dir -t rpm -n warp-ztna -v %s --architecture %s --after-install <stage>/DEBIAN/postinst --before-install <stage>/DEBIAN/preinst --before-remove <stage>/DEBIAN/prerm --conflicts netbird --conflicts netbird-ui --rpm-compression gzip --package %s/%s -C <stage> usr\n' "$VERSION" "$RPM_ARCH" "$OUT_DIR" "$RPM_NAME"
  printf '[dry-run] 产物 %s/%s、%s/%s（未签名）\n' "$OUT_DIR" "$DEB_NAME" "$OUT_DIR" "$RPM_NAME"
}

write_maintainer_scripts() {
  cat > "$STAGE/DEBIAN/postinst" <<'POSTINST_EOF'
#!/bin/sh
set -eu
BIN=/usr/local/lib/warp-ztna/netbird
CONF=/etc/warp-ztna/enroll.conf
mkdir -p /var/lib/warp-ztna /etc/warp-ztna /var/log/warp-ztna
chmod 0700 /var/lib/warp-ztna
chmod 0750 /etc/warp-ztna /var/log/warp-ztna
if [ -f /etc/systemd/system/warp-ztna.service ] || [ -f /usr/lib/systemd/system/warp-ztna.service ] || [ -f /lib/systemd/system/warp-ztna.service ]; then
  echo "Warp ZTNA：检测到既有 warp-ztna 服务，按升级处理（state 保留）：停服 → 换二进制 → 重装服务 → 启动。"
  "$BIN" service stop --service warp-ztna || true
  "$BIN" service uninstall --service warp-ztna || true
fi
"$BIN" service install --service warp-ztna --log-file /var/log/warp-ztna/client.log --log-level info --service-env "NB_STATE_DIR=/var/lib/warp-ztna" || { echo "Warp ZTNA：service install 失败（退出码 $?）。" >&2; exit 1; }
"$BIN" service start --service warp-ztna || { echo "Warp ZTNA：service start 失败（退出码 $?）。" >&2; exit 1; }
if [ ! -f "$CONF" ]; then
  echo "Warp ZTNA：未请求入网。装完后的两条引擎命令（原文，不新增命令面）："
  echo "  机器入网：$BIN up --setup-key-file <文件> --management-url <URL>"
  echo "  交互登录：$BIN login --no-browser"
  exit 0
fi
CONF_META="$(stat -c '%U %a' "$CONF")"
case "$CONF_META" in
  "root 600") ;;
  *)
    echo "Warp ZTNA：$CONF 必须是 root 所有且权限 0600（当前 ${CONF_META}），已跳过入网。"
    exit 0
    ;;
esac
SETUP_KEY="$(sed -n 's/^SETUP_KEY=//p' "$CONF" | head -n 1)"
MANAGEMENT_URL="$(sed -n 's/^MANAGEMENT_URL=//p' "$CONF" | head -n 1)"
if [ -z "$SETUP_KEY" ]; then
  echo "Warp ZTNA：$CONF 里没有 SETUP_KEY，已跳过入网。"
  exit 0
fi
if [ -z "$MANAGEMENT_URL" ]; then
  echo "Warp ZTNA：$CONF 里有 SETUP_KEY 但没有 MANAGEMENT_URL，已跳过入网——引擎不带该参数时会打它内置的公有云默认地址（不是本部署的管理服务器），本处签发的 key 在那边无效；补上 MANAGEMENT_URL 后重跑入网命令。"
  exit 0
fi
KEY_TMP="$(mktemp)"
chmod 0600 "$KEY_TMP"
printf '%s\n' "$SETUP_KEY" > "$KEY_TMP"
trap 'rm -f "$KEY_TMP"' EXIT INT TERM
set -- "$BIN" up --setup-key-file "$KEY_TMP" --management-url "$MANAGEMENT_URL"
if "$@"; then
  echo "Warp ZTNA：已按 $CONF 发起入网；用 $BIN status 复查连接状态。"
else
  echo "警告：入网命令未成功（引擎与服务已装好）；请稍后重试 $BIN up，或检查 ${CONF}。"
fi
exit 0
POSTINST_EOF
  cat > "$STAGE/DEBIAN/preinst" <<'PREINST_EOF'
#!/bin/sh
set -eu
if [ -f /etc/systemd/system/warp-ztna.service ] || [ -f /usr/lib/systemd/system/warp-ztna.service ] || [ -f /lib/systemd/system/warp-ztna.service ]; then
  exit 0
fi
if ! command -v netbird >/dev/null 2>&1; then
  exit 0
fi
NB_BIN="$(command -v netbird)"
echo "Warp ZTNA：检测到同类组网客户端（${NB_BIN}），安装将先替换它（停用并移除其服务与命令；其配置与状态目录保留）。"
"$NB_BIN" service stop --service netbird || true
"$NB_BIN" service uninstall --service netbird || true
for unit_dir in /etc/systemd/system /lib/systemd/system /usr/lib/systemd/system; do
  if [ -e "$unit_dir/netbird.service" ]; then
    systemctl disable netbird || true
    rm -f "$unit_dir/netbird.service"
  fi
done
systemctl daemon-reload || true
rm -f "$NB_BIN" /usr/bin/netbird /usr/local/bin/netbird /usr/bin/netbird-ui
rm -f /var/run/netbird.sock
exit 0
PREINST_EOF
  cat > "$STAGE/DEBIAN/prerm" <<'PRERM_EOF'
#!/bin/sh
set -eu
BIN=/usr/local/lib/warp-ztna/netbird
if [ -x "$BIN" ]; then
  "$BIN" service stop --service warp-ztna || true
  "$BIN" service uninstall --service warp-ztna || true
fi
exit 0
PRERM_EOF
  chmod 0755 "$STAGE/DEBIAN/postinst" "$STAGE/DEBIAN/preinst" "$STAGE/DEBIAN/prerm"
}

stage() {
  STAGE="$(mktemp -d)"
  trap 'rm -rf "$STAGE"' EXIT INT TERM
  dpkg-deb -R "$DEB" "$STAGE"
  mkdir -p "$STAGE/usr/local/lib/warp-ztna"
  tar -xzf "$ARCHIVE" -C "$STAGE/usr/local/lib/warp-ztna"
  [ -f "$STAGE/usr/local/lib/warp-ztna/netbird" ] || die "归档里没有 netbird：$ARCHIVE"
  chmod 0755 "$STAGE/usr/local/lib/warp-ztna/netbird"
  find "$STAGE/usr/local/lib/warp-ztna" -type f ! -name netbird -exec chmod 0644 {} +
  sed -i "s/^Version: .*/Version: $VERSION/" "$STAGE/DEBIAN/control"
  if ! grep -q '^Conflicts:' "$STAGE/DEBIAN/control"; then
    printf 'Conflicts: netbird, netbird-ui\n' >> "$STAGE/DEBIAN/control"
  fi
  write_maintainer_scripts
}

build_deb() {
  mkdir -p "$OUT_DIR"
  dpkg-deb -b "$STAGE" "$OUT_DIR/$DEB_NAME"
  printf '产出 %s/%s\n' "$OUT_DIR" "$DEB_NAME"
}

build_rpm() {
  fpm -s dir -t rpm -n warp-ztna -v "$VERSION" --architecture "$RPM_ARCH" \
    --after-install "$STAGE/DEBIAN/postinst" --before-install "$STAGE/DEBIAN/preinst" --before-remove "$STAGE/DEBIAN/prerm" \
    --conflicts netbird --conflicts netbird-ui \
    --rpm-compression gzip --package "$OUT_DIR/$RPM_NAME" \
    -C "$STAGE" usr
  printf '产出 %s/%s\n' "$OUT_DIR" "$RPM_NAME"
}

main() {
  parse_args "$@"
  if [ "$DRY_RUN" -eq 1 ]; then
    print_plan
    exit 0
  fi
  [ "$(uname -s)" = "Linux" ] || die "本脚本只能在 Linux 上运行"
  need_cmd dpkg-deb
  need_cmd tar
  need_cmd fpm
  need_cmd rpmbuild
  [ -f "$DEB" ] || die "找不到 deb：$DEB"
  [ -f "$ARCHIVE" ] || die "找不到归档：$ARCHIVE"
  stage
  build_deb
  build_rpm
}

main "$@"
