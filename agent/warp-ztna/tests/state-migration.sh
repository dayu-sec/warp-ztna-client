#!/bin/sh
set -eu

unset CDPATH
HERE="$(cd -- "$(dirname -- "$0")" && pwd)"
POSTINSTALL="$(cd -- "$HERE/.." && pwd)/pkg/scripts/postinstall"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT INT TERM
TOTAL=0
FAILED=0

ok() {
  TOTAL=$((TOTAL + 1))
  printf 'ok  %s\n' "$1"
}

bad() {
  TOTAL=$((TOTAL + 1))
  FAILED=$((FAILED + 1))
  printf 'FAIL  %s\n' "$1" >&2
}

extract() {
  sed -n "/^$1()/,/^}/p" "$POSTINSTALL"
}

FUNCS="$TMP/functions.sh"
{
  extract normalize_host
  extract profile_config_host
  extract profile_file_in_root
  extract adopt_legacy_state
} > "$FUNCS"
. "$FUNCS"

DEFAULT_MANAGEMENT_URL="https://mgmt.example.test:443"
DEFAULT_MANAGEMENT_HOST="mgmt.example.test"
STATE_OWNER="$(id -u):$(id -g)"

case_dir() {
  CASE_DIR="$TMP/case$1"
  LEGACY="$CASE_DIR/legacy"
  STATE_DIR="$CASE_DIR/state"
  mkdir -p "$LEGACY" "$STATE_DIR"
  LEGACY_STATE_ROOTS="$LEGACY"
  ACTIVE_FILE="$STATE_DIR/default.json"
  ACTIVE_HOST=""
  ENROLL_READY=0
}

profile() {
  printf '{"ManagementURL":{"Host":"%s"},"FQDN":"%s"}\n' "$2" "$3" > "$1"
}

CASE_NO=0

CASE_NO=$((CASE_NO + 1))
case_dir "$CASE_NO"
printf '{"name":"dy-sec"}\n' > "$LEGACY/active_profile.json"
profile "$LEGACY/dy-sec.json" "mgmt.example.test" "old-peer"
adopt_legacy_state
if [ -f "$STATE_DIR/dy-sec.json" ] && [ "$ACTIVE_FILE" = "$STATE_DIR/dy-sec.json" ] && [ "$ACTIVE_HOST" = "$DEFAULT_MANAGEMENT_HOST" ]; then
  ok "空 state 接管同域 active profile"
else
  bad "空 state 没有接管同域 active profile（ACTIVE_FILE=${ACTIVE_FILE}）"
fi

CASE_NO=$((CASE_NO + 1))
case_dir "$CASE_NO"
profile "$LEGACY/default.json" "other.example.test" "foreign-peer"
adopt_legacy_state
if [ ! -e "$STATE_DIR/default.json" ]; then
  ok "外域旧 state 不接管"
else
  bad "外域旧 state 被误接管"
fi

CASE_NO=$((CASE_NO + 1))
case_dir "$CASE_NO"
printf '{"name":"dy-sec"}\n' > "$STATE_DIR/active_profile.json"
profile "$STATE_DIR/dy-sec.json" "mgmt.example.test" "existing-peer"
ACTIVE_FILE="$STATE_DIR/dy-sec.json"
ACTIVE_HOST="$(profile_config_host "$ACTIVE_FILE")"
profile "$LEGACY/default.json" "mgmt.example.test" "legacy-peer"
adopt_legacy_state
if [ ! -e "$STATE_DIR/default.json" ] && [ "$ACTIVE_FILE" = "$STATE_DIR/dy-sec.json" ]; then
  ok "已有 state 不被改写"
else
  bad "已有 state 被改写"
fi

CASE_NO=$((CASE_NO + 1))
case_dir "$CASE_NO"
ENROLL_READY=1
profile "$LEGACY/default.json" "mgmt.example.test" "legacy-peer"
adopt_legacy_state
if [ ! -e "$STATE_DIR/default.json" ]; then
  ok "显式入网不接管旧 state"
else
  bad "显式入网错误接管旧 state"
fi

CASE_NO=$((CASE_NO + 1))
case_dir "$CASE_NO"
profile "$LEGACY/config.json" "mgmt.example.test" "legacy-config"
adopt_legacy_state
if [ -f "$STATE_DIR/default.json" ]; then
  ok "legacy config.json 接管为 default profile"
else
  bad "legacy config.json 未接管"
fi

printf '\n%s 项，%s 失败\n' "$TOTAL" "$FAILED"
[ "$FAILED" -eq 0 ]
