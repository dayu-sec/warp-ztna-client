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

if [ "$(uname -s)" != "Darwin" ]; then
  printf '本脚本只用于 macOS；Linux 用 uninstall-warp-linux.sh，Windows 用 uninstall-warp-windows.ps1\n' >&2
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

bootout() {
  if [ "$DRY_RUN" -eq 1 ]; then
    say "  [dry-run] launchctl bootout $*"
  else
    launchctl bootout "$@" 2>/dev/null || true
  fi
}

VERB="已删除"
if [ "$DRY_RUN" -eq 1 ]; then VERB="将删除"; fi

APP="/Applications/Warp ZTNA.app"
WARP_BIN=/usr/local/lib/warp-ztna/netbird
PLIST=/Library/LaunchDaemons/warp-ztna.plist
STATE=/var/lib/warp-ztna
CONF=/etc/warp-ztna
LOGDIR=/var/log/warp-ztna
RECEIPT=com.dy-sec.warpztna

SUDO_HOME=""
if [ -n "${SUDO_USER:-}" ] && [ "$SUDO_USER" != "root" ]; then
  SUDO_HOME="$(sudo -u "$SUDO_USER" sh -c 'printf %s "$HOME"' 2>/dev/null || true)"
fi
LOGIN_ITEM="$SUDO_HOME/Library/LaunchAgents/Warp ZTNA.plist"

say "== 1) 盘点现状 =="
if [ -e "$APP" ]; then say "  应用：${APP}"; fi
if [ -e "$WARP_BIN" ]; then say "  引擎二进制：${WARP_BIN}"; fi
if [ -e "$PLIST" ]; then say "  launchd plist：${PLIST}"; fi
for d in "$STATE" "$CONF" "$LOGDIR"; do
  if [ -e "$d" ]; then say "  目录：${d}"; fi
done
if [ -n "$SUDO_HOME" ] && [ -e "$LOGIN_ITEM" ]; then
  say "  登录项：${LOGIN_ITEM}"
fi
for v in /Volumes/Warp\ ZTNA*; do
  if [ -d "$v" ]; then say "  挂载卷：${v}（脚本不弹卷；不用了就 hdiutil detach \"${v}\" 弹出）"; fi
done
OFFICIAL=""
for c in /usr/local/bin/netbird /opt/homebrew/bin/netbird; do
  if [ -x "$c" ]; then OFFICIAL="$c"; break; fi
done
if launchctl print system/netbird >/dev/null 2>&1; then
  say "  旧引擎服务在跑——本脚本不触碰它"
elif [ -n "$OFFICIAL" ]; then
  say "  发现旧引擎二进制（${OFFICIAL}）——本脚本不触碰它"
else
  say "  未发现旧引擎"
fi
if [ ! -e "$WARP_BIN" ] && [ -e "$APP" ]; then
  say "  场景：只装 app（daemon 由旧引擎驱动）——只删 app 与用户侧残留"
fi

say "== 2) 停用并注销 Warp ZTNA 引擎服务（warp-ztna） =="
if [ -x "$WARP_BIN" ]; then
  run "$WARP_BIN" service stop --service warp-ztna || true
  run "$WARP_BIN" service uninstall --service warp-ztna || true
else
  say "  跳过：未发现 Warp ZTNA 引擎服务（${WARP_BIN}）"
fi
bootout system/warp-ztna
run rm -f "$PLIST"

say "== 3) 退出并删除应用与引擎 =="
if [ -n "$SUDO_HOME" ]; then
  if [ "$DRY_RUN" -eq 1 ]; then
    say "  [dry-run] 退出应用（osascript，若在运行）"
  else
    sudo -u "$SUDO_USER" osascript -e 'tell application "Warp ZTNA" to quit' >/dev/null 2>&1 || true
  fi
fi
run rm -rf "$APP"
run rm -rf /usr/local/lib/warp-ztna
if [ "$DRY_RUN" -eq 1 ]; then
  say "  [dry-run] pkgutil --forget ${RECEIPT}（若存在该安装收据）"
elif pkgutil --pkg-info "$RECEIPT" >/dev/null 2>&1; then
  pkgutil --forget "$RECEIPT" >/dev/null
  say "  已注销安装收据：${RECEIPT}"
else
  say "  无安装收据可注销：${RECEIPT}"
fi
say "  ${VERB}：${APP}、/usr/local/lib/warp-ztna"

say "== 4) 当前用户侧 =="
if [ -z "$SUDO_HOME" ]; then
  say "  未能解析当前用户家目录（SUDO_USER 缺失或不可解析），跳过登录项与用户配置处理"
else
  SUDO_UID_NUM="$(id -u "$SUDO_USER" 2>/dev/null || true)"
  if [ -n "$SUDO_UID_NUM" ]; then
    bootout "gui/${SUDO_UID_NUM}/Warp ZTNA"
  fi
  if [ -e "$LOGIN_ITEM" ]; then
    run rm -f "$LOGIN_ITEM"
    say "  ${VERB}登录项：${LOGIN_ITEM}"
  fi
  APP_DATA="$SUDO_HOME/Library/Application Support/com.dy-sec.warpztna.app"
  if [ "$PURGE" -eq 1 ]; then
    run rm -rf "$APP_DATA"
    say "  ${VERB}用户配置（--purge）：${APP_DATA}"
  else
    say "  已保留用户配置：${APP_DATA}（连它一起清：重跑加 --purge）"
  fi
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
if [ -e "$APP" ]; then
  say "  应用残留            ->  ${APP}"
else
  say "  应用残留            ->  （无）"
fi
launchd_found="$(find /Library/LaunchDaemons -maxdepth 1 -type f -iname '*warp*' 2>/dev/null)"
say "  LaunchDaemons 残留  ->  ${launchd_found:-（无）}"
say "  launchctl 查询      ->  $(launchctl print system/warp-ztna 2>&1 | head -1)"
say "  旧引擎完好   ->  $(command -v netbird 2>/dev/null || echo '(未装)') / $(launchctl print system/netbird 2>&1 | head -1)"
if [ -e "$STATE" ]; then
  say "  state 目录          ->  ${STATE}"
else
  say "  state 目录          ->  （无）"
fi
