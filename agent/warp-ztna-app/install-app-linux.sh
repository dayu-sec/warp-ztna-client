#!/bin/sh
set -eu

WARP_VERSION="0.1.1"
BASE_URL="https://github.com/dayu-sec/warp-ztna-client/releases/download"
VERSION="$WARP_VERSION"
DRY_RUN=0

usage() {
  cat <<'USAGE'
用法：install-app.sh [选项]

在 Linux 机器上一条命令完成客户端安装：按本机架构与包管理器取 deb / rpm 并安装。
包内自带引擎与桌面应用，安装时由包内脚本注册并启动 warp-ztna 服务。
本机已有同类组网客户端时先替换它（停用并移除其服务、命令与桌面应用包；state 保留），再安装。

选项：
  --version <x.y.z>  覆盖安装的包版本，默认脚本内嵌版本
  --base-url <URL>   分发主机地址，默认 https://github.com/dayu-sec/warp-ztna-client/releases/download
  --dry-run          只打印计划，不下载不安装
  --help             显示本帮助
USAGE
}

die() {
  printf '错误：%s\n' "$*" >&2
  exit 1
}

note() {
  printf '%s\n' "$*"
}

need_cmd() {
  command -v "$1" >/dev/null 2>&1 || die "缺少命令：$1"
}

parse_args() {
  while [ $# -gt 0 ]; do
    case "$1" in
      --version)
        [ $# -ge 2 ] || die "--version 缺少取值"
        VERSION="$2"
        shift 2
        ;;
      --base-url)
        [ $# -ge 2 ] || die "--base-url 缺少取值"
        BASE_URL="$2"
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
}

detect_arch() {
  case "$(uname -m)" in
    x86_64|amd64) ARCH="amd64" ;;
    arm64|aarch64) ARCH="arm64" ;;
    *) die "不支持的架构：$(uname -m)" ;;
  esac
}

detect_manager() {
  if command -v dpkg >/dev/null 2>&1; then
    PKG_KIND="deb"
  elif command -v rpm >/dev/null 2>&1; then
    PKG_KIND="rpm"
  elif [ "$(uname -s)" = "Linux" ]; then
    die "找不到 dpkg 或 rpm：请按发行版手动下载 deb 或 rpm 安装"
  else
    PKG_KIND="deb"
  fi
}

replace_official() {
  NB_BIN="$(command -v netbird)"
  note "检测到同类组网客户端（${NB_BIN}）：先替换它（停用并移除其服务、CLI 与桌面应用；其配置与状态目录保留），再安装 Warp 客户端。"
  "$NB_BIN" service stop --service netbird || true
  "$NB_BIN" service uninstall --service netbird || true
  for unit_dir in /etc/systemd/system /lib/systemd/system /usr/lib/systemd/system; do
    if [ -e "$unit_dir/netbird.service" ]; then
      systemctl disable netbird || true
      rm -f "$unit_dir/netbird.service"
    fi
  done
  systemctl daemon-reload || true
  if command -v dpkg-query >/dev/null 2>&1; then
    for p in netbird-ui netbird; do
      if dpkg-query -W -f='${Status}' "$p" 2>/dev/null | grep -q 'install ok installed'; then
        DEBIAN_FRONTEND=noninteractive apt-get remove -y "$p" || true
      fi
    done
  elif command -v rpm >/dev/null 2>&1; then
    for p in netbird-ui netbird; do
      if rpm -q "$p" >/dev/null 2>&1; then
        rpm -e "$p" || true
      fi
    done
  fi
  rm -f "$NB_BIN" /usr/bin/netbird /usr/local/bin/netbird /usr/bin/netbird-ui
  rm -f /var/run/netbird.sock
}

print_plan() {
  printf '[dry-run] 架构 %s；安装包类型 %s\n' "$ARCH" "$PKG_KIND"
  printf '[dry-run] 安装包 %s\n' "$PKG_URL"
  printf '[dry-run] 安装包已自带引擎与桌面应用，无需另装引擎\n'
  printf '[dry-run] 若本机已装同类组网客户端：先替换它（停用并移除其服务、CLI 与桌面应用；state 保留），再安装\n'
  if [ "$PKG_KIND" = "deb" ]; then
    printf '[dry-run] DEBIAN_FRONTEND=noninteractive apt-get install -y <临时目录>/%s（无 apt-get 时 dpkg -i）\n' "$PKG_NAME"
  else
    printf '[dry-run] dnf install -y <临时目录>/%s（无 dnf 时 yum，再退 rpm -Uvh）\n' "$PKG_NAME"
  fi
  printf '[dry-run] 包内 postinst 注册并启动 warp-ztna 服务（state /var/lib/warp-ztna）\n'
}

install_package() {
  note "下载客户端安装包：$PKG_URL"
  curl -fsSL --retry 5 --retry-delay 2 --retry-all-errors -C - -o "${WORK}/${PKG_NAME}" "$PKG_URL"
  note "安装 ${PKG_NAME}…"
  if [ "$PKG_KIND" = "deb" ]; then
    if command -v apt-get >/dev/null 2>&1; then
      DEBIAN_FRONTEND=noninteractive apt-get install -y "${WORK}/${PKG_NAME}"
    else
      dpkg -i "${WORK}/${PKG_NAME}"
    fi
  else
    if command -v dnf >/dev/null 2>&1; then
      dnf install -y "${WORK}/${PKG_NAME}"
    elif command -v yum >/dev/null 2>&1; then
      yum install -y "${WORK}/${PKG_NAME}"
    else
      rpm -Uvh "${WORK}/${PKG_NAME}"
    fi
  fi
}

main() {
  parse_args "$@"
  detect_arch
  detect_manager
  BASE="${BASE_URL%/}"
  PKG_NAME="WarpZTNA-App-${VERSION}-${ARCH}.${PKG_KIND}"
  PKG_URL="${BASE}/client-v${VERSION}/${PKG_NAME}"
  if [ "$DRY_RUN" -eq 1 ]; then
    print_plan
    exit 0
  fi
  [ "$(uname -s)" = "Linux" ] || die "本脚本只能在 Linux 上运行"
  [ "$(id -u)" -eq 0 ] || die "需要 root：请用 curl -fsSL <地址> | sudo sh 运行"
  if command -v netbird >/dev/null 2>&1; then
    replace_official
  fi
  need_cmd curl
  need_cmd mktemp
  WORK="$(mktemp -d)"
  trap 'rm -rf "${WORK}"' EXIT INT TERM
  install_package
  note "安装完成：打开 Warp ZTNA 应用，在欢迎页点击登录，用企业域账号完成 SSO。"
}

main "$@"
