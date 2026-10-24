#!/bin/sh
set -eu

WARP_VERSION="0.1.13"
NETBIRD_VERSION="0.80.0"

DEFAULT_BASE_URL="https://github.com/dayu-sec/warp-ztna-client/releases/download"
DEFAULT_SERVICE="warp-ztna"
DEFAULT_STATE_DIR="/var/lib/warp-ztna"
DEFAULT_LOG_FILE="/var/log/warp-ztna/client.log"
DEFAULT_CONF_DIR="/etc/warp-ztna"
BIN_DIR="/usr/local/lib/warp-ztna"
BIN_PATH="$BIN_DIR/netbird"
POLL_SECONDS=60
POLL_INTERVAL=3

SERVICE="${WARP_ZTNA_SERVICE_NAME:-$DEFAULT_SERVICE}"
STATE_DIR="${WARP_ZTNA_STATE_DIR:-$DEFAULT_STATE_DIR}"
LOG_FILE="${WARP_ZTNA_LOG_FILE:-$DEFAULT_LOG_FILE}"
CONF_DIR="${WARP_ZTNA_CONF_DIR:-$DEFAULT_CONF_DIR}"
BASE_URL="${WARP_ZTNA_BASE_URL:-$DEFAULT_BASE_URL}"
MANAGEMENT_URL="${WARP_ZTNA_MANAGEMENT_URL:-}"
PRODUCT_DEFAULT_MANAGEMENT_URL="https://ztna.corp.dy-sec.com:443"
SETUP_KEY="${WARP_ZTNA_SETUP_KEY:-}"
SETUP_KEY_FILE=""
VERSION="$WARP_VERSION"
ENROLL="none"
case "${WARP_ZTNA_FORCE:-0}" in
  1|true|yes) FORCE=1 ;;
  *) FORCE=0 ;;
esac
DRY_RUN=0
UNINSTALL=0
PURGE=0
OS=""
ARCH=""
MODE=""
CLI=""
ARTIFACT=""
WORK_DIR=""
KEY_TMP=""
KEY_TMP_OWNED=0
SUDO_CMD="${WARP_ZTNA_SUDO:-sudo}"

LOG_DIR="$(dirname "$LOG_FILE")"

usage() {
  cat <<'USAGE'
用法：install.sh [选项]

把 Warp ZTNA 引擎装为本机系统服务（默认服务名 warp-ztna），可选在安装后一步完成机器入网。

入网选项：
  --setup-key <KEY>          机器入网凭据（由管理台「资产入网」签发，一次性、用后即废）。
                             脚本会转成 0600 临时文件传递，不落明文本。
  --setup-key-file <PATH>    从文件读取入网凭据，优先于 --setup-key。
  --management-url <URL>     NetBird Management 地址（形如 https://<管理服务器域名>，由部署方提供）。
                             setup-key 入网必填：不给时引擎会打内置公有云地址，本处签发的 key 无效。
                             注意：入网安装会先清空本机既有的 NetBird 配置（/var/lib/warp-ztna，
                             可用 WARP_ZTNA_STATE_DIR 改）及旧版引擎残留的 /etc/netbird 等目录中
                             的旧配置，再按下发目标入网（对端旧 peer 记录不受影响）；入网完成后
                             会复核实际连接的 Management，不一致即报错退出（防“已连接”静默短路）。
  --enroll <setup-key|sso|none>
                             装完后的入网方式：
                               setup-key  用上面的 key 入网（只给 key 不给 --enroll 时自动选用）
                               sso        交互 SSO 登录（沿用引擎当前的 Management 配置；
                                          要更换 Management 请走 setup-key 路径或 --uninstall --purge 重装）
                               none       只装引擎、不入网（默认）

维护与调试：
  --uninstall                卸载服务与二进制；本地 state 保留
  --purge                    与 --uninstall 同用：连 state、日志与配置目录一并删除
                             （彻底重置/换环境用 --uninstall --purge 后重新安装）
  --version <x.y.z>          安装指定版本，默认脚本内嵌版本
  --base-url <URL>           产物基址，默认 GitHub Releases 直链（内网可换成镜像基址）
  --service <NAME>           服务名，默认 warp-ztna
  --dry-run                  只打印将执行的命令，不改动系统
  --force                    （兼容保留：遇同类组网客户端现在一律先替换，无需开关）
  --help                     显示本帮助

环境变量（与同名开关等价）：
  WARP_ZTNA_MANAGEMENT_URL / WARP_ZTNA_SETUP_KEY / WARP_ZTNA_BASE_URL / WARP_ZTNA_SHA256
  WARP_ZTNA_SERVICE_NAME / WARP_ZTNA_STATE_DIR / WARP_ZTNA_LOG_FILE / WARP_ZTNA_CONF_DIR
  WARP_ZTNA_FORCE / WARP_ZTNA_SUDO
USAGE
}

die() {
  printf '错误：%s\n' "$*" >&2
  exit 1
}

warn() {
  printf '警告：%s\n' "$*" >&2
}

note() {
  printf '%s\n' "$*"
}

need_cmd() {
  command -v "$1" >/dev/null 2>&1 || die "缺少命令：$1"
}

print_masked() {
  for masked_arg in "$@"; do
    if [ -n "$SETUP_KEY" ] && [ "$masked_arg" = "$SETUP_KEY" ]; then
      printf '%s ' "xxxxx***"
    else
      printf '%s ' "$masked_arg"
    fi
  done
  printf '\n'
}

announce() {
  printf '[dry-run] '
  print_masked "$@"
}

run() {
  if [ "$DRY_RUN" -eq 1 ]; then
    announce "$@"
    return 0
  fi
  "$@"
}

run_root() {
  if [ "$DRY_RUN" -eq 1 ]; then
    if [ "$(id -u)" -eq 0 ]; then
      announce "$@"
    else
      announce "$SUDO_CMD" "$@"
    fi
    return 0
  fi
  if [ "$(id -u)" -eq 0 ]; then
    "$@"
  else
    "$SUDO_CMD" "$@"
  fi
}

parse_args() {
  while [ $# -gt 0 ]; do
    case "$1" in
      --setup-key)
        [ $# -ge 2 ] || die "--setup-key 缺少取值"
        SETUP_KEY="$2"
        shift 2
        ;;
      --setup-key-file)
        [ $# -ge 2 ] || die "--setup-key-file 缺少取值"
        SETUP_KEY_FILE="$2"
        shift 2
        ;;
      --management-url)
        [ $# -ge 2 ] || die "--management-url 缺少取值"
        MANAGEMENT_URL="$2"
        shift 2
        ;;
      --enroll)
        [ $# -ge 2 ] || die "--enroll 缺少取值"
        ENROLL="$2"
        shift 2
        ;;
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
      --service)
        [ $# -ge 2 ] || die "--service 缺少取值"
        SERVICE="$2"
        shift 2
        ;;
      --force)
        FORCE=1
        shift
        ;;
      --uninstall)
        UNINSTALL=1
        shift
        ;;
      --purge)
        PURGE=1
        shift
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
  case "$ENROLL" in
    setup-key|sso|none) ;;
    *) die "--enroll 只能是 setup-key、sso 或 none" ;;
  esac
  if [ "$ENROLL" = "none" ] && { [ -n "$SETUP_KEY" ] || [ -n "$SETUP_KEY_FILE" ]; }; then
    ENROLL="setup-key"
  fi
  if [ "$ENROLL" = "setup-key" ] && [ -z "$SETUP_KEY" ] && [ -z "$SETUP_KEY_FILE" ]; then
    die "--enroll setup-key 需要 --setup-key 或 --setup-key-file"
  fi
  if [ "$ENROLL" = "setup-key" ] && [ -z "$MANAGEMENT_URL" ]; then
    die "setup key 入网必须给 --management-url（或环境变量 WARP_ZTNA_MANAGEMENT_URL）：不给时引擎会打它内置的公有云默认地址（不是本部署的管理服务器），本处签发的 key 在那边无效"
  fi
  if [ "$PURGE" -eq 1 ] && [ "$UNINSTALL" -eq 0 ]; then
    die "--purge 只能与 --uninstall 一起使用"
  fi
  case "$VERSION" in
    [0-9]*.[0-9]*.[0-9]*) ;;
    *) die "--version 需要形如 1.2.3 的版本号" ;;
  esac
  if [ "$FORCE" -eq 1 ]; then
    note "--force 兼容保留：遇同类组网客户端现在一律先替换，无需开关。"
  fi
}

detect_platform() {
  case "$(uname -s)" in
    Linux) OS="linux" ;;
    Darwin) OS="darwin" ;;
    *) die "不支持的系统：$(uname -s)" ;;
  esac
  case "$(uname -m)" in
    x86_64|amd64) ARCH="amd64" ;;
    arm64|aarch64) ARCH="arm64" ;;
    *) die "不支持的架构：$(uname -m)" ;;
  esac
}

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

service_installed() {
  if [ "$OS" = "linux" ]; then
    [ -f "/etc/systemd/system/$SERVICE.service" ] || [ -f "/lib/systemd/system/$SERVICE.service" ]
  else
    [ -f "/Library/LaunchDaemons/$SERVICE.plist" ]
  fi
}

detect_existing() {
  if service_installed; then
    MODE="upgrade"
    return 0
  fi
  if command -v netbird >/dev/null 2>&1; then
    MODE="replace"
    return 0
  fi
  MODE="fresh"
}

ensure_dirs() {
  run_root mkdir -p "$BIN_DIR"
  run_root mkdir -p "$STATE_DIR"
  run_root mkdir -p "$CONF_DIR"
  run_root mkdir -p "$LOG_DIR"
  run_root chmod 0755 "$BIN_DIR"
  run_root chmod 0700 "$STATE_DIR"
  run_root chmod 0750 "$CONF_DIR"
  run_root chmod 0750 "$LOG_DIR"
}

fetch_artifact() {
  ARTIFACT="warp-ztna_${VERSION}_${OS}_${ARCH}.tar.gz"
  ARTIFACT_URL="${BASE_URL%/}/client-v${VERSION}/${ARTIFACT}"
  CHECKSUMS_URL="${BASE_URL%/}/client-v${VERSION}/checksums.txt"
  WORK_DIR="$(mktemp -d)"
  note "下载 $ARTIFACT_URL"
  run curl -fsSL --retry 5 --retry-delay 2 --retry-all-errors -C - -o "$WORK_DIR/$ARTIFACT" "$ARTIFACT_URL"
  run curl -fsSL --retry 5 --retry-delay 2 --retry-all-errors -C - -o "$WORK_DIR/checksums.txt" "$CHECKSUMS_URL"
}

verify_checksum() {
  if [ "$DRY_RUN" -eq 1 ]; then
    note "[dry-run] 校验 $ARTIFACT 的 sha256（来源 ${CHECKSUMS_URL}，可用 WARP_ZTNA_SHA256 覆盖）"
    return 0
  fi
  if [ -n "${WARP_ZTNA_SHA256:-}" ]; then
    EXPECTED_SHA256="$WARP_ZTNA_SHA256"
  else
    EXPECTED_SHA256="$(awk -v name="$ARTIFACT" '$2 == name { print $1 }' "$WORK_DIR/checksums.txt")"
  fi
  [ -n "$EXPECTED_SHA256" ] || die "checksums.txt 里找不到 $ARTIFACT 的校验值"
  ACTUAL_SHA256="$(sha256_of "$WORK_DIR/$ARTIFACT")"
  if [ "$ACTUAL_SHA256" != "$EXPECTED_SHA256" ]; then
    die "校验失败：$ARTIFACT 期望 ${EXPECTED_SHA256}，实际 $ACTUAL_SHA256"
  fi
  note "校验通过：$ARTIFACT"
}

install_binary() {
  run tar -xzf "$WORK_DIR/$ARTIFACT" -C "$WORK_DIR" netbird
  run_root install -m 0755 "$WORK_DIR/netbird" "$BIN_PATH"
}

service_install() {
  SERVICE_ENV="NB_STATE_DIR=$STATE_DIR"
  if [ "$OS" = "darwin" ]; then
    SERVICE_ENV="$SERVICE_ENV,NB_ENABLE_LOCAL_FORWARDING=true"
  fi
  run_root "$BIN_PATH" service install --service "$SERVICE" --log-file "$LOG_FILE" --log-level info --service-env "$SERVICE_ENV"
  run_root "$BIN_PATH" service start --service "$SERVICE"
}

service_stop() {
  run_root "$CLI" service stop --service "$SERVICE"
}

service_uninstall() {
  run_root "$CLI" service uninstall --service "$SERVICE"
}

normalize_host() {
  printf '%s\n' "$1" | sed -e 's#^[A-Za-z][A-Za-z0-9+.-]*://##' -e 's#/.*$##' -e 's#:443$##'
}

management_file_host() {
  host="$(sed -n 's/.*"ManagementURL"[^}]*"Host": *"\([^"]*\)".*/\1/p' "$1" | head -n 1)"
  if [ -z "$host" ]; then
    host="$(sed -n 's/.*"Host": *"\([^"]*\)".*/\1/p' "$1" | head -n 1)"
  fi
  if [ -z "$host" ]; then
    host="$(sed -n 's/.*"managementURL": *"\([^"]*\)".*/\1/p' "$1" | head -n 1)"
  fi
  [ -n "$host" ] || return 1
  normalize_host "$host"
}

state_management_hosts() {
  for state_file in "$STATE_DIR"/*.json; do
    [ -f "$state_file" ] || continue
    management_file_host "$state_file" || true
  done
}

switch_management_state() {
  [ "$ENROLL" = "setup-key" ] || return 0
  previous="$(state_management_hosts | sed -n '1p')"
  if [ -n "$previous" ]; then
    note "检测到本机既有 NetBird 配置（指向 ${previous}）——入网安装将先清空："
  fi
  note "  清空本机既有配置（${STATE_DIR}）后按下发目标 $(normalize_host "$MANAGEMENT_URL") 入网；旧环境的连接不再保留（对端旧记录不受影响）。"
  run_root rm -rf "$STATE_DIR"
  run_root mkdir -p "$STATE_DIR"
  run_root chmod 0700 "$STATE_DIR"
}

sweep_legacy_management_state() {
  for root in /etc/netbird /var/lib/netbird; do
    [ -d "$root" ] || continue
    find "$root" -name '*.json' -type f 2>/dev/null | while IFS= read -r state_file; do
      case "$state_file" in
        */active_profile.json|*/state.json) continue ;;
      esac
      host="$(management_file_host "$state_file" || true)"
      [ -n "$host" ] || continue
      [ "$host" = "$(normalize_host "$PRODUCT_DEFAULT_MANAGEMENT_URL")" ] && continue
      note "移除残留的旧配置（${state_file} → ${host}）：旧版引擎的该文件会被新版引擎在原位置缺失时自动搬运，先清掉以免旧连接复活。"
      run_root rm -f "$state_file"
    done
  done
}

live_management_host() {
  "$CLI" status -d 2>/dev/null | sed -n 's/^Management: Connected to \(.*\)$/\1/p' | head -n 1
}

verify_management_target() {
  if [ "$DRY_RUN" -eq 1 ]; then
    return 0
  fi
  target="$(normalize_host "$MANAGEMENT_URL")"
  [ -n "$target" ] || return 0
  live="$(live_management_host || true)"
  [ -n "$live" ] || die "入网后无法确认管理服务器地址（$CLI status -d 未给出 Management: Connected to 行），请用 $CLI status -d 复查。"
  host="$(normalize_host "$live")"
  if [ "$host" != "$target" ]; then
    die "入网后校验失败：当前连接的 Management 为 ${host}，期望 ${target}（原始行：Management: Connected to ${live}）——setup key 未生效或未完成切换。"
  fi
}

replace_official() {
  NB_BIN="$(command -v netbird)"
  note "替换同类组网客户端（${NB_BIN}）：停用并移除其服务、CLI 与桌面应用；其配置与状态目录不随卸载删除，其中指向其他管理服务器的旧配置会在安装时被清理。"
  run_root "$NB_BIN" service stop --service netbird || true
  run_root "$NB_BIN" service uninstall --service netbird || true
  if [ "$OS" = "linux" ]; then
    replace_official_linux
  else
    replace_official_darwin
  fi
}

replace_official_linux() {
  for unit_dir in /etc/systemd/system /lib/systemd/system /usr/lib/systemd/system; do
    if [ -e "$unit_dir/netbird.service" ]; then
      run_root systemctl disable netbird || true
      run_root rm -f "$unit_dir/netbird.service"
    fi
  done
  run_root systemctl daemon-reload || true
  if command -v dpkg-query >/dev/null 2>&1; then
    for p in netbird-ui netbird; do
      if dpkg-query -W -f='${Status}' "$p" 2>/dev/null | grep -q 'install ok installed'; then
        run_root env DEBIAN_FRONTEND=noninteractive apt-get remove -y "$p" || true
      fi
    done
  elif command -v rpm >/dev/null 2>&1; then
    for p in netbird-ui netbird; do
      if rpm -q "$p" >/dev/null 2>&1; then
        run_root rpm -e "$p" || true
      fi
    done
  fi
  run_root rm -f "$NB_BIN" /usr/bin/netbird /usr/local/bin/netbird /usr/bin/netbird-ui
  run_root rm -f /var/run/netbird.sock
}

replace_official_darwin() {
  run_root launchctl bootout system/netbird 2>/dev/null || true
  run_root rm -f /Library/LaunchDaemons/netbird.plist
  run_root pkill -f "NetBird UI.app" || true
  run_root rm -rf "/Applications/NetBird.app" "/Applications/NetBird UI.app"
  run_root rm -f "$NB_BIN" /usr/local/bin/netbird /opt/homebrew/bin/netbird
  run_root rm -f /var/run/netbird.sock
}

prepare_key_file() {
  if [ -n "$SETUP_KEY_FILE" ]; then
    if [ "$DRY_RUN" -eq 0 ]; then
      [ -f "$SETUP_KEY_FILE" ] || die "找不到 --setup-key-file：$SETUP_KEY_FILE"
    fi
    KEY_TMP="$SETUP_KEY_FILE"
    return 0
  fi
  if [ -z "$WORK_DIR" ]; then
    WORK_DIR="$(mktemp -d)"
  fi
  KEY_TMP="$WORK_DIR/setup-key"
  KEY_TMP_OWNED=1
  if [ "$DRY_RUN" -eq 1 ]; then
    note "[dry-run] 把 --setup-key 写入 0600 临时文件（用后删除、不打印内容）"
    return 0
  fi
  (umask 077; printf '%s\n' "$SETUP_KEY" > "$KEY_TMP")
}

enroll() {
  case "$ENROLL" in
    none)
      note "未请求入网。装完后的两条引擎命令："
      note "  机器入网：$CLI up --setup-key-file <文件> --management-url <URL>"
      note "  交互登录：$CLI login --no-browser"
      ;;
    setup-key)
      prepare_key_file
      run "$CLI" up --setup-key-file "$KEY_TMP" --management-url "$MANAGEMENT_URL"
      ;;
    sso)
      note "即将打印设备码 URI 并阻塞等待浏览器授权；无 TTY 时请先准备好浏览器。"
      run "$CLI" login --no-browser
      ;;
  esac
}

wait_connected() {
  if [ "$DRY_RUN" -eq 1 ]; then
    note "[dry-run] 轮询 $CLI status -C ready，最多 ${POLL_SECONDS}s"
    return 0
  fi
  ELAPSED=0
  while [ "$ELAPSED" -lt "$POLL_SECONDS" ]; do
    if "$CLI" status -C ready >/dev/null 2>&1; then
      return 0
    fi
    sleep "$POLL_INTERVAL"
    ELAPSED=$((ELAPSED + POLL_INTERVAL))
  done
  return 1
}

report_peer() {
  note "$CLI status 摘要："
  "$CLI" status 2>/dev/null | grep -E 'Management|Signal|FQDN|NetBird IP' || true
  note "以上 FQDN / 内部 IP 供在管理台「资产入网」里认人。"
}

uninstall() {
  if ! service_installed && [ "$DRY_RUN" -eq 0 ]; then
    note "未发现 $SERVICE 服务，无需卸载。"
    return 0
  fi
  CLI="$BIN_PATH"
  if [ ! -x "$BIN_PATH" ] && command -v netbird >/dev/null 2>&1; then
    CLI="$(command -v netbird)"
  fi
  service_stop
  service_uninstall
  run_root rm -f "$BIN_PATH"
  if [ "$PURGE" -eq 1 ]; then
    run_root rm -rf "$STATE_DIR" "$CONF_DIR" "$LOG_DIR"
    note "已随 --purge 删除 state、日志与配置目录。"
  else
    note "state 已保留（${STATE_DIR}）；如需一并删除请加 --purge。"
  fi
  if [ "$OS" = "linux" ]; then
    run_root systemctl daemon-reload
  fi
  note "脚本不触碰 Management 里的 peer 记录；摘 Router 与删 peer 是运维动作。"
}

cleanup() {
  if [ "$KEY_TMP_OWNED" -eq 1 ] && [ -n "$KEY_TMP" ]; then
    rm -f "$KEY_TMP"
  fi
  if [ -n "$WORK_DIR" ]; then
    rm -rf "$WORK_DIR"
  fi
}

report_paths() {
  note "服务名：$SERVICE"
  note "二进制：$BIN_PATH"
  note "state：$STATE_DIR"
  note "日志：$LOG_FILE"
  note "引擎版本：${NETBIRD_VERSION}"
}

main() {
  parse_args "$@"
  trap 'cleanup' EXIT INT TERM
  need_cmd uname
  need_cmd tar
  need_cmd awk
  if [ "$(id -u)" -ne 0 ]; then
    command -v "$SUDO_CMD" >/dev/null 2>&1 || die "需要 root 权限，且找不到 $SUDO_CMD"
  fi
  detect_platform
  detect_existing

  if [ "$UNINSTALL" -eq 1 ]; then
    uninstall
    return 0
  fi

  case "$MODE" in
    upgrade)
      CLI="$BIN_PATH"
      note "检测到既有 $SERVICE 服务，按升级处理：停用并注销 → 换二进制 → 重装服务 → 启动。"
      service_stop || true
      service_uninstall || true
      if [ "$DRY_RUN" -eq 0 ] && service_installed; then
        die "旧服务未注销干净（$SERVICE 仍在）：请先手工执行 $BIN_PATH service uninstall --service $SERVICE 再重试"
      fi
      ;;
    replace)
      CLI="$BIN_PATH"
      replace_official
      ;;
    *)
      CLI="$BIN_PATH"
      ;;
  esac

  need_cmd curl
  ensure_dirs
  fetch_artifact
  verify_checksum
  install_binary
  switch_management_state
  sweep_legacy_management_state
  service_install

  enroll

  if [ "$ENROLL" = "none" ]; then
    report_paths
    note "完成（未请求入网）。"
    return 0
  fi

  if wait_connected; then
    verify_management_target
    report_peer
    report_paths
    note "完成。"
  else
    warn "服务已安装，但 ${POLL_SECONDS}s 内未确认连接（$CLI status -C ready）。请用 $CLI status 复查。"
    exit 1
  fi
}

main "$@"
