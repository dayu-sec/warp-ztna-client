#!/bin/sh
set -eu

PURGE=0
DRY_RUN=0
usage() {
  printf '用法：sudo sh %s [--purge] [--dry-run]\n' "$0"
  printf '卸载 Warp 桌面应用与 Warp ZTNA 引擎服务（warp-ztna）。默认保留 state（/var/lib/warp-ztna）、配置（/etc/warp-ztna）与日志；--purge 连它们和当前用户的 app 配置一起删。\n'
  printf '先看将执行的动作：加 --dry-run。本脚本不处理其他引擎的物料。\n'
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
  printf '本脚本只用于 Linux；macOS 用 uninstall-warp-macos.sh，Windows 用 uninstall-warp-windows.ps1\n' >&2
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
UNIT_DIRS="/etc/systemd/system /lib/systemd/system /usr/lib/systemd/system"

UNIT_PATH=""
for d in $UNIT_DIRS; do
  if [ -e "$d/warp-ztna.service" ]; then UNIT_PATH="$d/warp-ztna.service"; break; fi
done

PKG=""
PKG_MGR=""
if command -v dpkg >/dev/null 2>&1 && dpkg -S "$WARP_BIN" >/dev/null 2>&1; then
  PKG="$(dpkg -S "$WARP_BIN" | cut -d: -f1)"
  PKG_MGR=apt-get
elif command -v rpm >/dev/null 2>&1 && rpm -q -f "$WARP_BIN" >/dev/null 2>&1; then
  PKG="$(rpm -q --qf '%{NAME}' -f "$WARP_BIN")"
  PKG_MGR=rpm
fi

SUDO_HOME=""
if [ -n "${SUDO_USER:-}" ] && [ "$SUDO_USER" != "root" ]; then
  SUDO_HOME="$(sudo -u "$SUDO_USER" sh -c 'printf %s "$HOME"' 2>/dev/null || true)"
fi

say "== 1) 盘点现状 =="
if [ -x "$WARP_BIN" ]; then say "  引擎二进制：${WARP_BIN}"; fi
if [ -n "$UNIT_PATH" ]; then say "  systemd unit：${UNIT_PATH}"; fi
if command -v systemctl >/dev/null 2>&1; then
  say "  warp-ztna 服务状态：$(systemctl is-active warp-ztna 2>&1 | head -1)"
fi
if [ -n "$PKG" ]; then
  say "  安装包：${PKG}"
elif [ -x "$WARP_BIN" ]; then
  say "  引擎非包安装（由 install.sh 接入）"
fi
OFFICIAL="$(command -v netbird 2>/dev/null || true)"
if command -v systemctl >/dev/null 2>&1 && systemctl is-active netbird >/dev/null 2>&1; then
  say "  旧引擎服务在跑——本脚本不触碰它"
elif [ -n "$OFFICIAL" ]; then
  say "  发现旧引擎二进制（${OFFICIAL}）——本脚本不触碰它"
else
  say "  未发现旧引擎"
fi
for d in "$STATE" "$CONF" "$LOGDIR"; do
  if [ -e "$d" ]; then say "  目录：${d}"; fi
done
if [ -n "$SUDO_HOME" ] && [ -e "$SUDO_HOME/.config/autostart/Warp ZTNA.desktop" ]; then
  say "  自启文件：${SUDO_HOME}/.config/autostart/Warp ZTNA.desktop"
fi
if [ ! -x "$WARP_BIN" ] && [ -z "$PKG" ]; then
  say "  未发现 Warp ZTNA 引擎服务与安装包——可能已卸载过"
fi

say "== 2) 停用并注销 Warp ZTNA 引擎服务（warp-ztna） =="
if [ -x "$WARP_BIN" ]; then
  run "$WARP_BIN" service stop --service warp-ztna || true
  run "$WARP_BIN" service uninstall --service warp-ztna || true
else
  say "  跳过：未发现 Warp ZTNA 引擎服务（${WARP_BIN}）"
  if [ -n "$UNIT_PATH" ] && command -v systemctl >/dev/null 2>&1; then
    run systemctl stop warp-ztna || true
    run systemctl disable warp-ztna || true
  fi
fi
if [ -n "$UNIT_PATH" ]; then
  run rm -f "$UNIT_PATH"
fi
if [ -n "$UNIT_PATH" ] && command -v systemctl >/dev/null 2>&1; then
  run systemctl daemon-reload || true
fi

say "== 3) 卸包与删除引擎 =="
if [ -n "$PKG" ]; then
  if [ "$PKG_MGR" = "apt-get" ]; then
    run env DEBIAN_FRONTEND=noninteractive apt-get remove -y "$PKG" || true
  else
    run rpm -e "$PKG" || true
  fi
else
  say "  无安装包记录，直接删引擎目录"
fi
run rm -rf /usr/local/lib/warp-ztna
say "  ${VERB}：${WARP_BIN}、/usr/local/lib/warp-ztna"

say "== 4) 当前用户侧 =="
if [ -z "$SUDO_HOME" ]; then
  say "  未能解析当前用户家目录（SUDO_USER 缺失或不可解析），跳过自启文件与用户配置处理"
else
  run rm -f "$SUDO_HOME/.config/autostart/Warp ZTNA.desktop"
  APP_DATA="$SUDO_HOME/.config/com.dy-sec.warpztna.app"
  if [ "$PURGE" -eq 1 ]; then
    run rm -rf "$APP_DATA"
    say "  ${VERB}用户配置（--purge）：${APP_DATA}"
  else
    say "  已保留用户配置：${APP_DATA}（连它一起清：重跑加 --purge）"
  fi
  say "  提示：app 日志在 ${SUDO_HOME}/.local/share/com.dy-sec.warpztna.app/logs/，脚本不删"
fi

say "== 5) state / 配置 / 日志 =="
if [ "$PURGE" -eq 1 ]; then
  run rm -rf "$STATE" "$CONF" "$LOGDIR"
  say "  ${VERB} state / 配置 / 日志（--purge）"
else
  say "  已保留：${STATE}（设备身份）、${CONF}、${LOGDIR}"
  say "  （连身份一起清：重跑加 --purge；${CONF}/enroll.conf 含 setup key 明文，属敏感面）"
fi
say "  提示：${STATE}/service.json 跨卸载存活，重装可能静默恢复旧参数；改参数用引擎 CLI 的 service reconfigure"

say "== 6) 核对 =="
if [ -e "$WARP_BIN" ]; then
  say "  引擎残留            ->  ${WARP_BIN}"
else
  say "  引擎残留            ->  （无）"
fi
units="$(find /etc/systemd/system /lib/systemd/system /usr/lib/systemd/system -maxdepth 1 -type f -iname '*warp*' 2>/dev/null)"
say "  systemd unit 残留   ->  ${units:-（无）}"
if command -v systemctl >/dev/null 2>&1; then
  say "  systemctl 查询      ->  $(systemctl is-active warp-ztna 2>&1 | head -1)"
fi
PKG_NOW=""
if command -v dpkg >/dev/null 2>&1 && dpkg -S "$WARP_BIN" >/dev/null 2>&1; then
  PKG_NOW="$(dpkg -S "$WARP_BIN" | cut -d: -f1)"
elif command -v rpm >/dev/null 2>&1 && rpm -q -f "$WARP_BIN" >/dev/null 2>&1; then
  PKG_NOW="$(rpm -q --qf '%{NAME}' -f "$WARP_BIN")"
fi
say "  安装包残留          ->  ${PKG_NOW:-（无）}"
say "  旧引擎完好   ->  $(command -v netbird 2>/dev/null || echo '(未装)')"
if [ -e "$STATE" ]; then
  say "  state 目录          ->  ${STATE}"
else
  say "  state 目录          ->  （无）"
fi
