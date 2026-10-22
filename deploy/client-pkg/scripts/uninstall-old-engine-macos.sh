#!/bin/sh
set -eu

PURGE=0
DRY_RUN=0
usage() {
  printf '用法：sudo sh %s [--purge] [--dry-run]\n' "$0"
  printf '清理旧引擎与 Warp 的 warp-ztna 服务残留（替换场景：清干净后装 Warp）。默认保留 state（/var/lib/warp-ztna、/var/lib/netbird）；--purge 连 state、配置与日志一起删。\n'
  printf '先看将执行的动作：加 --dry-run。Warp 桌面应用（/Applications/Warp ZTNA.app）本脚本不动。\n'
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
  printf '本脚本只用于 macOS；Linux 用 uninstall-old-engine-linux.sh，Windows 用 uninstall-old-engine-windows.ps1\n' >&2
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

WARP_BIN=/usr/local/lib/warp-ztna/netbird
STATE=/var/lib/warp-ztna
CONF=/etc/warp-ztna
LOGDIR=/var/log/warp-ztna
NB_STATE=/var/lib/netbird
NB_CONF=/etc/netbird
NB_APP="/Applications/NetBird.app"
NB_UI_APP="/Applications/NetBird UI.app"
SOCKET=/var/run/netbird.sock

say "== 1) 盘点现状 =="
for c in /usr/local/bin/netbird "$WARP_BIN" /opt/homebrew/bin/netbird; do
  if [ -e "$c" ]; then say "  二进制：${c}"; fi
done
launchd_found="$(find /Library/LaunchDaemons -maxdepth 1 -type f \( -iname '*netbird*' -o -iname '*warp*' \) 2>/dev/null)"
if [ -n "$launchd_found" ]; then
  printf '%s\n' "$launchd_found"
else
  say "  /Library/LaunchDaemons 下无旧引擎 / warp-ztna plist"
fi
if [ -e "$SOCKET" ]; then say "  socket 在：${SOCKET}"; fi
if [ -e "$NB_APP" ] || [ -e "$NB_UI_APP" ]; then say "  应用：${NB_APP} / ${NB_UI_APP}"; fi
for d in "$STATE" "$NB_STATE" "$NB_CONF"; do
  if [ -e "$d" ]; then say "  目录：${d}"; fi
done

say "== 2) 停用并注销 Warp ZTNA 引擎服务（warp-ztna） =="
if [ -x "$WARP_BIN" ]; then
  run "$WARP_BIN" service stop --service warp-ztna || true
  run "$WARP_BIN" service uninstall --service warp-ztna || true
else
  say "  跳过：未发现 Warp ZTNA 引擎服务（${WARP_BIN}）"
fi
bootout system/warp-ztna
run rm -f /Library/LaunchDaemons/warp-ztna.plist

say "== 3) 停用并注销旧引擎 =="
NB=""
for c in /usr/local/bin/netbird /opt/homebrew/bin/netbird; do
  if [ -x "$c" ]; then NB="$c"; break; fi
done
if [ -z "$NB" ]; then
  NB="$(command -v netbird 2>/dev/null || true)"
fi
if [ -n "$NB" ]; then
  say "  使用：${NB}"
  run "$NB" service stop --service netbird || true
  run "$NB" service uninstall --service netbird || true
else
  say "  未发现旧引擎二进制，跳过服务注销"
fi
bootout system/netbird
run rm -f /Library/LaunchDaemons/netbird.plist

say "== 4) 删除二进制与应用 =="
run rm -rf /usr/local/lib/warp-ztna
run rm -f /usr/local/bin/netbird
run rm -rf "$NB_APP" "$NB_UI_APP"
say "  ${VERB}：/usr/local/lib/warp-ztna、/usr/local/bin/netbird、${NB_APP}、${NB_UI_APP}"
say "  （Warp 桌面应用 /Applications/Warp ZTNA.app 未动；不需要就用 Finder 拖走）"

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
launchd_found="$(find /Library/LaunchDaemons -maxdepth 1 -type f \( -iname '*netbird*' -o -iname '*warp*' \) 2>/dev/null)"
say "  LaunchDaemons 残留  ->  ${launchd_found:-（无）}"
say "  launchctl 查询      ->  $(launchctl print system/warp-ztna 2>&1 | head -1)"
if [ -S "$SOCKET" ]; then
  say "  socket              ->  ${SOCKET}（存在）"
else
  say "  socket              ->  （无）"
fi
