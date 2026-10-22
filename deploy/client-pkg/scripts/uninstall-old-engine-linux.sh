#!/bin/sh
set -eu

PURGE=0
DRY_RUN=0
usage() {
  printf '用法：sudo sh %s [--purge] [--dry-run]\n' "$0"
  printf '清理旧引擎与 Warp 的 warp-ztna 服务残留（替换场景：清干净后装 Warp）。默认保留 state（/var/lib/netbird、/var/lib/warp-ztna）；--purge 连 state、配置与日志一起删。\n'
  printf '先看将执行的动作：加 --dry-run。Warp 以包安装时（app+引擎一体）本脚本不动它的包。\n'
}
for arg in "$@"; do
  case "$arg" in
    --purge) PURGE=1 ;;
    --dry-run) DRY_RUN=1 ;;
    -h|--help) usage; exit 0 ;;
    *) printf '未知参数：%s\n' "$arg" >&2; usage >&2; exit 2 ;;
  esac
done

if [ "$(uname -s)" != "Linux" ]; then
  printf '本脚本只用于 Linux；macOS 用 uninstall-old-engine-macos.sh，Windows 用 uninstall-old-engine-windows.ps1\n' >&2
  exit 1
fi

if [ "$(id -u)" -ne 0 ]; then
  printf '需要 root：sudo sh %s\n' "$0" >&2
  exit 1
fi

say() { printf '%s\n' "$*"; }
run() {
  if [ "$DRY_RUN" -eq 1 ]; then
    say "  [dry-run] $*"
  else
    "$@"
  fi
}

VERB="已删除"
if [ "$DRY_RUN" -eq 1 ]; then VERB="将删除"; fi

WARP_BIN=/usr/local/lib/warp-ztna/netbird
STATE=/var/lib/warp-ztna
CONF=/etc/warp-ztna
LOGDIR=/var/log/warp-ztna
NB_STATE=/var/lib/netbird
NB_CONF=/etc/netbird
NB_INSTALL_CONF=/etc/netbird/install.conf
SOCKET=/var/run/netbird.sock
UNIT_DIRS="/etc/systemd/system /lib/systemd/system /usr/lib/systemd/system"

WARP_UNIT=""
for d in $UNIT_DIRS; do
  if [ -e "$d/warp-ztna.service" ]; then WARP_UNIT="$d/warp-ztna.service"; break; fi
done

NB_UNIT=""
for d in $UNIT_DIRS; do
  if [ -e "$d/netbird.service" ]; then NB_UNIT="$d/netbird.service"; break; fi
done

NB="$(command -v netbird 2>/dev/null || true)"
if [ -z "$NB" ]; then
  for c in /usr/bin/netbird /usr/local/bin/netbird; do
    if [ -x "$c" ]; then NB="$c"; break; fi
  done
fi

NB_MGR=""
if [ -e "$NB_INSTALL_CONF" ]; then
  NB_MGR="$(grep -E '^package_manager=' "$NB_INSTALL_CONF" 2>/dev/null | cut -d= -f2 || true)"
fi

WARP_PKG=""
if command -v dpkg >/dev/null 2>&1 && dpkg -S "$WARP_BIN" >/dev/null 2>&1; then
  WARP_PKG="$(dpkg -S "$WARP_BIN" | cut -d: -f1)"
elif command -v rpm >/dev/null 2>&1 && rpm -q -f "$WARP_BIN" >/dev/null 2>&1; then
  WARP_PKG="$(rpm -q --qf '%{NAME}' -f "$WARP_BIN")"
fi

NB_PKGS=""
NB_PKGS_MGR=""
if command -v dpkg-query >/dev/null 2>&1; then
  for p in netbird netbird-ui; do
    if dpkg-query -W -f='${Status}' "$p" 2>/dev/null | grep -q 'install ok installed'; then
      NB_PKGS="${NB_PKGS:+${NB_PKGS} }${p}"
    fi
  done
  if [ -n "$NB_PKGS" ]; then NB_PKGS_MGR=apt-get; fi
elif command -v rpm >/dev/null 2>&1; then
  for p in netbird netbird-ui; do
    if rpm -q "$p" >/dev/null 2>&1; then
      NB_PKGS="${NB_PKGS:+${NB_PKGS} }${p}"
    fi
  done
  if [ -n "$NB_PKGS" ]; then NB_PKGS_MGR=rpm; fi
fi

say "== 1) 盘点现状 =="
if [ -x "$WARP_BIN" ]; then say "  二进制：${WARP_BIN}"; fi
if [ -n "$NB" ] && [ -e "$NB" ]; then say "  二进制：${NB}"; fi
if [ -n "$WARP_UNIT" ]; then say "  systemd unit：${WARP_UNIT}"; fi
if [ -n "$NB_UNIT" ]; then say "  systemd unit：${NB_UNIT}"; fi
if command -v systemctl >/dev/null 2>&1; then
  say "  服务状态：warp-ztna=$(systemctl is-active warp-ztna 2>&1 | head -1)，netbird=$(systemctl is-active netbird 2>&1 | head -1)"
fi
if [ -n "$WARP_PKG" ]; then say "  Warp ZTNA 引擎服务由包管理器安装：${WARP_PKG}"; fi
if [ -n "$NB_PKGS" ]; then say "  旧引擎包：${NB_PKGS}"; fi
if [ -n "$NB_MGR" ]; then say "  官方安装方式（install.conf）：${NB_MGR}"; fi
if [ -e "$SOCKET" ]; then say "  socket 在：${SOCKET}"; fi
for d in "$STATE" "$NB_STATE" "$NB_CONF"; do
  if [ -e "$d" ]; then say "  目录：${d}"; fi
done

say "== 2) 停用并注销 Warp ZTNA 引擎服务（warp-ztna） =="
if [ -x "$WARP_BIN" ]; then
  run "$WARP_BIN" service stop --service warp-ztna || true
  run "$WARP_BIN" service uninstall --service warp-ztna || true
else
  say "  跳过：未发现 Warp ZTNA 引擎服务（${WARP_BIN}）"
  if [ -n "$WARP_UNIT" ] && command -v systemctl >/dev/null 2>&1; then
    run systemctl stop warp-ztna || true
    run systemctl disable warp-ztna || true
  fi
fi
if [ -n "$WARP_UNIT" ]; then
  run rm -f "$WARP_UNIT"
fi
if [ -n "$WARP_UNIT" ] && command -v systemctl >/dev/null 2>&1; then
  run systemctl daemon-reload || true
fi

say "== 3) 停用并注销旧引擎 =="
if [ -n "$NB" ]; then
  say "  使用：${NB}"
  run "$NB" service stop --service netbird || true
  run "$NB" service uninstall --service netbird || true
else
  say "  未发现旧引擎二进制，跳过服务注销"
  if [ -n "$NB_UNIT" ] && command -v systemctl >/dev/null 2>&1; then
    run systemctl stop netbird || true
    run systemctl disable netbird || true
  fi
fi
if [ -n "$NB_UNIT" ]; then
  run rm -f "$NB_UNIT"
fi
if [ -n "$NB_UNIT" ] && command -v systemctl >/dev/null 2>&1; then
  run systemctl daemon-reload || true
fi

say "== 4) 删除二进制与包 =="
if [ -n "$WARP_PKG" ]; then
  say "  Warp ZTNA 引擎服务由包管理器安装（${WARP_PKG}，app+引擎一体）——本脚本不动它的文件；整包卸载用 uninstall-warp-linux.sh"
else
  run rm -rf /usr/local/lib/warp-ztna
  say "  ${VERB}：/usr/local/lib/warp-ztna"
fi
if [ -n "$NB_PKGS" ]; then
  if [ "$NB_PKGS_MGR" = "apt-get" ]; then
    for pkg in $NB_PKGS; do
      run env DEBIAN_FRONTEND=noninteractive apt-get remove -y "$pkg" || true
    done
  else
    for pkg in $NB_PKGS; do
      run rpm -e "$pkg" || true
    done
  fi
  say "  ${VERB}包：${NB_PKGS}"
else
  say "  未发现旧引擎的安装包，删除二进制"
  run rm -f /usr/bin/netbird /usr/local/bin/netbird
  say "  ${VERB}：/usr/bin/netbird、/usr/local/bin/netbird"
fi

say "== 5) 状态与残留 =="
run rm -f "$SOCKET"
if [ "$PURGE" -eq 1 ]; then
  run rm -rf "$STATE" "$CONF" "$LOGDIR"
  run rm -rf "$NB_STATE" "$NB_CONF"
  say "  ${VERB} state / 配置 / 日志（--purge）：Warp 与旧引擎两侧"
else
  say "  已保留 state：${STATE}、${NB_STATE}（连身份一起清：重跑加 --purge）"
fi

say "== 6) 核对 =="
say "  command -v netbird  ->  $(command -v netbird 2>/dev/null || echo '(无)')"
units="$(find /etc/systemd/system /lib/systemd/system /usr/lib/systemd/system -maxdepth 1 -type f \( -iname '*netbird*' -o -iname '*warp*' \) 2>/dev/null)"
say "  systemd unit 残留   ->  ${units:-（无）}"
if command -v systemctl >/dev/null 2>&1; then
  say "  systemctl 查询      ->  warp-ztna=$(systemctl is-active warp-ztna 2>&1 | head -1)，netbird=$(systemctl is-active netbird 2>&1 | head -1)"
fi
if [ -S "$SOCKET" ]; then
  say "  socket              ->  ${SOCKET}（存在）"
else
  say "  socket              ->  （无）"
fi
STATE_NOW=""
for d in "$STATE" "$NB_STATE"; do
  if [ -e "$d" ]; then STATE_NOW="${STATE_NOW}${STATE_NOW:+ }${d}"; fi
done
say "  state 目录          ->  ${STATE_NOW:-（无）}"
