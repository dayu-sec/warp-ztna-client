#!/bin/sh
set -eu

unset CDPATH
HERE="$(cd -- "$(dirname -- "$0")" && pwd)"
NODE_DIR="$(dirname -- "$HERE")"
VERSION_FILE="$(tr -d '[:space:]' < "$NODE_DIR/VERSION")"
INSTALL_SH="$NODE_DIR/install.sh"
TEST_PATH="/usr/bin:/bin"
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

expect_in() {
  case "$1" in
    *"$2"*) ok "$3" ;;
    *) bad "$3（输出中未找到：$2）" ;;
  esac
}

expect_not_in() {
  case "$1" in
    *"$2"*) bad "$3（输出中不应出现：$2）" ;;
    *) ok "$3" ;;
  esac
}

expect_absent() {
  if grep -q -- "$2" "$1"; then bad "$3（$1 不应出现：$2）"; else ok "$3"; fi
}

expect_grep() {
  if grep -q -- "$2" "$1"; then ok "$3"; else bad "$3（$1 未命中：$2）"; fi
}

run_install() {
  PATH="$TEST_PATH" sh "$INSTALL_SH" "$@"
}

if sh -n "$INSTALL_SH"; then
  ok "install.sh 通过 sh -n"
else
  bad "install.sh 语法错误"
fi

if command -v shellcheck >/dev/null 2>&1; then
  if shellcheck --shell=sh "$INSTALL_SH"; then
    ok "shellcheck 无告警"
  else
    bad "shellcheck 有告警"
  fi
else
  printf '跳过  shellcheck 未安装\n'
fi

if help_out="$(run_install --help 2>&1)"; then
  ok "--help 退出码为 0"
else
  bad "--help 非零退出"
  help_out=""
fi
for flag in --setup-key --setup-key-file --management-url --enroll --version --base-url --service --force --uninstall --purge --dry-run --help; do
  expect_in "$help_out" "$flag" "--help 列出 $flag"
done

if run_install --not-a-flag >/dev/null 2>&1; then
  bad "未知参数应当非零退出"
else
  ok "未知参数非零退出"
fi

if dry_out="$(run_install --dry-run 2>&1)"; then
  ok "install --dry-run 退出码为 0"
else
  bad "install --dry-run 非零退出"
  dry_out=""
fi
expect_in "$dry_out" "${VERSION_FILE}/warp-ztna_${VERSION_FILE}_" "dry-run 下载地址指向 <版本>/ 产物"
expect_in "$dry_out" "--service warp-ztna" "服务注册用 warp-ztna"
expect_in "$dry_out" "NB_STATE_DIR=/var/lib/warp-ztna" "state 目录带品牌前缀"
expect_in "$dry_out" "--log-file /var/log/warp-ztna/client.log" "日志路径落位"
expect_in "$dry_out" "service install" "注册系统服务"
expect_in "$dry_out" "service start" "启动系统服务"
expect_not_in "$dry_out" "--daemon-addr" "不改 socket 地址"

FAKE_BIN="$(mktemp -d)"
printf '#!/bin/sh\nprintf "0.80.0\\n"\n' > "$FAKE_BIN/netbird"
chmod 0755 "$FAKE_BIN/netbird"
if replaced_out="$(PATH="$FAKE_BIN:/usr/bin:/bin" sh "$INSTALL_SH" --dry-run --service warp-ztna-smoke-test 2>&1)"; then
  expect_in "$replaced_out" "service uninstall --service netbird" "有官方 NetBird 先替换：注销官方服务"
  expect_in "$replaced_out" "service install --service warp-ztna-smoke-test" "替换后继续走安装计划"
else
  bad "有官方 NetBird 应当走替换路径"
  replaced_out=""
fi
printf '#!/bin/sh\nprintf "0.76.3\\n"\n' > "$FAKE_BIN/netbird"
chmod 0755 "$FAKE_BIN/netbird"
if legacy_out="$(PATH="$FAKE_BIN:/usr/bin:/bin" WARP_ZTNA_FORCE=1 sh "$INSTALL_SH" --dry-run --service warp-ztna-smoke-test 2>&1)"; then
  expect_in "$legacy_out" "service uninstall --service netbird" "0.76.3 同样替换（不看版本；--force 兼容参数不影响）"
else
  bad "0.76.3 也应当走替换路径"
fi
expect_absent "$INSTALL_SH" "netbird version" "install.sh 不读官方版本"
if grep -A4 "按升级处理" "$INSTALL_SH" | grep -q "service_uninstall"; then
  ok "install.sh 升级路径停用并注销既有服务（否则 service install 报 Init already exists）"
else
  bad "install.sh 升级路径缺 service uninstall：既有 warp-ztna 服务时重装必失败"
fi
expect_grep "$INSTALL_SH" "旧服务未注销干净" "install.sh 注销残留时给出指路错误"
expect_grep "$INSTALL_SH" "清空本机既有配置" "install.sh 入网路径先清空既有配置（一律）"
expect_grep "$INSTALL_SH" "入网后校验失败" "install.sh 入网后复核实际 Management（防静默 no-op）"
expect_grep "$INSTALL_SH" "先清空本机既有的 NetBird 配置" "install.sh usage 反映入网先清空语义"
expect_grep "$INSTALL_SH" '"ManagementURL"' "install.sh 按真实键名解析既有配置（PascalCase）"
expect_grep "$INSTALL_SH" '"managementURL"' "install.sh 兼容 0.80 之前的键名形态"
expect_grep "$INSTALL_SH" "sweep_legacy_management_state" "install.sh 扫旧版残留目录（/etc/netbird 会被引擎自动搬运复活）"
EXISTING_STATE="$(mktemp -d)"
printf '{\n  "Name": "Default",\n  "ManagementURL": {\n    "Scheme": "https",\n    "Host": "old.example.com:443",\n    "Path": ""\n  }\n}\n' > "$EXISTING_STATE/default.json"
if switch_out="$(PATH="$TEST_PATH" WARP_ZTNA_STATE_DIR="$EXISTING_STATE" sh "$INSTALL_SH" --dry-run --setup-key S3CR3T-CANARY --management-url https://nb.example.com 2>&1)"; then
  expect_in "$switch_out" "既有 NetBird 配置（指向 old.example.com）" "install.sh 打印检测到的既有配置（行为断言）"
  expect_in "$switch_out" "清空本机既有配置" "install.sh 入网先清空说明（行为断言）"
  expect_in "$switch_out" "rm -rf $EXISTING_STATE" "install.sh 清空既有 state（dry-run 可见）"
else
  bad "带既有 state 的 dry-run 非零退出"
fi
rm -rf "$EXISTING_STATE"
COMPACT_STATE="$(mktemp -d)"
printf '{"Name":"Default","ManagementURL":{"Scheme":"https","Host":"old2.example.com:443","Path":""}}\n' > "$COMPACT_STATE/default.json"
if compact_out="$(PATH="$TEST_PATH" WARP_ZTNA_STATE_DIR="$COMPACT_STATE" sh "$INSTALL_SH" --dry-run --setup-key S3CR3T-CANARY --management-url https://nb.example.com 2>&1)"; then
  expect_in "$compact_out" "既有 NetBird 配置（指向 old2.example.com）" "install.sh 兼容单行紧凑形态（行为断言）"
else
  bad "紧凑形态 state 的 dry-run 非零退出"
fi
rm -rf "$COMPACT_STATE"
LEGACY_STATE="$(mktemp -d)"
printf '{\n  "managementURL": "https://old3.example.com:443",\n  "adminURL": "https://app.netbird.io:443"\n}\n' > "$LEGACY_STATE/default.json"
if legacy_fmt_out="$(PATH="$TEST_PATH" WARP_ZTNA_STATE_DIR="$LEGACY_STATE" sh "$INSTALL_SH" --dry-run --setup-key S3CR3T-CANARY --management-url https://nb.example.com 2>&1)"; then
  expect_in "$legacy_fmt_out" "既有 NetBird 配置（指向 old3.example.com）" "install.sh 兼容 0.80 之前的 camelCase 字符串形态（行为断言）"
else
  bad "旧版 camelCase 形态 state 的 dry-run 非零退出"
fi
rm -rf "$LEGACY_STATE"
rm -rf "$FAKE_BIN"

if canary_out="$(run_install --dry-run --setup-key S3CR3T-CANARY --management-url https://nb.example.com 2>&1)"; then
  ok "带密钥 dry-run 退出码为 0"
else
  bad "带密钥 dry-run 非零退出"
  canary_out=""
fi
expect_not_in "$canary_out" "S3CR3T-CANARY" "密钥不出现在输出里"
expect_in "$canary_out" "up --setup-key-file" "密钥经临时文件传递"

if no_url_out="$(run_install --dry-run --setup-key S3CR3T-CANARY 2>&1)"; then
  bad "带 setup key 但不带 management-url 应当非零退出"
  no_url_out=""
else
  ok "带 setup key 但不带 management-url 非零退出"
fi
expect_in "$no_url_out" "必须给 --management-url" "缺管理地址时指路报错（引擎默认会打官方 SaaS）"
expect_not_in "$no_url_out" "S3CR3T-CANARY" "拒绝时也不回显密钥"

if scope_out="$(run_install --dry-run --management-url https://nb.example.com --setup-key-file /tmp/k 2>&1)"; then
  ok "带 management-url dry-run 退出码为 0"
else
  bad "带 management-url dry-run 非零退出"
  scope_out=""
fi
expect_in "$scope_out" "--management-url https://nb.example.com" "management-url 透传"

if vb_out="$(run_install --dry-run --version 9.9.9 --base-url https://mirror.example 2>&1)"; then
  ok "带 version/base-url dry-run 退出码为 0"
else
  bad "带 version/base-url dry-run 非零退出"
  vb_out=""
fi
expect_in "$vb_out" "https://mirror.example/client-v9.9.9/warp-ztna_9.9.9_" "version 与 base-url 影响下载地址"
expect_grep "$INSTALL_SH" "github.com/dayu-sec/warp-ztna-client/releases/download" "install.sh 默认取件基址是 Release 直链"
expect_absent "$INSTALL_SH" "pkg.warp-ztna.internal" "install.sh 不再含旧内网域名"

if un_out="$(run_install --uninstall --dry-run 2>&1)"; then
  ok "uninstall --dry-run 退出码为 0"
else
  bad "uninstall --dry-run 非零退出"
  un_out=""
fi
expect_in "$un_out" "service stop" "卸载先停服务"
expect_in "$un_out" "service uninstall" "注销服务"
expect_not_in "$un_out" "rm -rf" "默认卸载不动 state"

if purge_out="$(run_install --uninstall --purge --dry-run 2>&1)"; then
  ok "uninstall --purge --dry-run 退出码为 0"
else
  bad "uninstall --purge --dry-run 非零退出"
  purge_out=""
fi
expect_in "$purge_out" "rm -rf /var/lib/warp-ztna" "--purge 删除 state"

VERSION_SCRIPT="$(sed -n 's/^WARP_VERSION="\([^"]*\)"$/\1/p' "$INSTALL_SH")"
if [ "$VERSION_FILE" = "$VERSION_SCRIPT" ]; then
  ok "install.sh 内嵌 WARP_VERSION 与 VERSION 一致（${VERSION_FILE}）"
else
  bad "WARP_VERSION 不一致：文件 ${VERSION_FILE}，脚本 $VERSION_SCRIPT"
fi

NETBIRD_FILE="$(tr -d '[:space:]' < "$NODE_DIR/NETBIRD_VERSION")"
NETBIRD_SCRIPT="$(sed -n 's/^NETBIRD_VERSION="\([^"]*\)"$/\1/p' "$INSTALL_SH")"
if [ "$NETBIRD_FILE" = "$NETBIRD_SCRIPT" ]; then
  ok "install.sh 内嵌 NETBIRD_VERSION 与 NETBIRD_VERSION 一致（${NETBIRD_FILE}）"
else
  bad "NETBIRD_VERSION 不一致：文件 ${NETBIRD_FILE}，脚本 $NETBIRD_SCRIPT"
fi

SHA_LINES="$(grep -c '=' "$NODE_DIR/NETBIRD_SHA256")"
if [ "$SHA_LINES" -eq 6 ]; then
  ok "NETBIRD_SHA256 覆盖六个平台组合"
else
  bad "NETBIRD_SHA256 行数应为 6，实际 $SHA_LINES"
fi
for pair in linux_amd64 linux_arm64 darwin_amd64 darwin_arm64 windows_amd64 windows_arm64; do
  sha_value="$(sed -n "s/^${pair}=//p" "$NODE_DIR/NETBIRD_SHA256")"
  case "$sha_value" in
    [0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f])
      ok "NETBIRD_SHA256 的 $pair 是 64 位十六进制"
      ;;
    *)
      bad "NETBIRD_SHA256 的 $pair 不是 64 位十六进制：$sha_value"
      ;;
  esac
done

if grep -q "warp up" "$INSTALL_SH"; then
  bad "install.sh 出现了 warp 自有命令面"
else
  ok "脚本里没有 warp 自有命令面"
fi

ARCHIVES_SH="$NODE_DIR/build-archives.sh"
MACPKG_SH="$NODE_DIR/build-macpkg.sh"
VERIFY_SH="$NODE_DIR/tests/verify-macos-artifacts.sh"
PREINSTALL="$NODE_DIR/pkg/scripts/preinstall"
POSTINSTALL="$NODE_DIR/pkg/scripts/postinstall"
DISTRIBUTION="$NODE_DIR/pkg/Distribution.xml"
README_HTML="$NODE_DIR/pkg/Readme.html"
APP_DIR="$NODE_DIR/../warp-ztna-app"
APP_MAIN="$APP_DIR/src-tauri/src/main.rs"
DMG_SH="$APP_DIR/build-dmg.sh"
MACOS_CONF="$APP_DIR/src-tauri/tauri.macos.conf.json"
APP_INSTALL_SH="$APP_DIR/install-app.sh"
APP_LINUX_INSTALL_SH="$APP_DIR/install-app-linux.sh"

if [ -d "$NODE_DIR/../warp-ztna-node" ]; then
  bad "旧的 agent/warp-ztna-node 目录仍在"
else
  ok "旧目录名已清"
fi

MULTIBYTE_AFTER_VAR=""
for script in "$INSTALL_SH" "$ARCHIVES_SH" "$MACPKG_SH" "$DMG_SH" "$VERIFY_SH" "$PREINSTALL" "$POSTINSTALL" "$APP_INSTALL_SH" "$APP_LINUX_INSTALL_SH" "$0"; do
  if LC_ALL=C grep -qE '\$[A-Za-z_][A-Za-z0-9_]*[^ -~]' "$script"; then
    MULTIBYTE_AFTER_VAR="$MULTIBYTE_AFTER_VAR $script"
  fi
done
if [ -n "$MULTIBYTE_AFTER_VAR" ]; then
  bad "以下脚本里变量名紧跟多字节字符，会被吞进变量名（用 \${VAR} 括起来）：$MULTIBYTE_AFTER_VAR"
else
  ok "没有变量紧跟多字节字符的写法"
fi

for script in "$ARCHIVES_SH" "$MACPKG_SH" "$DMG_SH" "$VERIFY_SH" "$PREINSTALL" "$POSTINSTALL"; do
  if sh -n "$script"; then ok "$script 通过 sh -n"; else bad "$script 语法错误"; fi
done

if command -v shellcheck >/dev/null 2>&1; then
  if shellcheck --shell=sh "$ARCHIVES_SH" "$MACPKG_SH" "$DMG_SH" "$VERIFY_SH" "$PREINSTALL" "$POSTINSTALL"; then
    ok "shellcheck 无告警（打包与 pkg 脚本）"
  else
    bad "shellcheck 有告警（打包与 pkg 脚本）"
  fi
else
  printf '跳过  shellcheck 未安装（打包与 pkg 脚本）\n'
fi

for script in "$ARCHIVES_SH" "$MACPKG_SH" "$DMG_SH"; do
  if "$script" --help >/dev/null 2>&1; then ok "$script --help 退出码 0"; else bad "$script --help 非零退出"; fi
done
if sh "$ARCHIVES_SH" --dry-run >/dev/null 2>&1; then
  bad "build-archives.sh 缺 --upstream 应当非零退出"
else
  ok "build-archives.sh 缺 --upstream 非零退出"
fi
if sh "$MACPKG_SH" --dry-run --arch arm64 >/dev/null 2>&1; then
  bad "build-macpkg.sh 缺 --archive 应当非零退出"
else
  ok "build-macpkg.sh 缺 --archive 非零退出"
fi
if sh "$DMG_SH" --dry-run --arch arm64 --app /nonexistent.app >/dev/null 2>&1; then
  bad "build-dmg.sh 缺 --nodepkg 应当非零退出"
else
  ok "build-dmg.sh 缺 --nodepkg 非零退出"
fi
if sh "$VERIFY_SH" >/dev/null 2>&1; then
  bad "verify-macos-artifacts.sh 缺目录应当非零退出"
else
  ok "verify-macos-artifacts.sh 缺目录非零退出"
fi

if arch_out="$(sh "$ARCHIVES_SH" --dry-run --upstream x 2>&1)"; then
  ok "build-archives.sh --dry-run 退出码 0"
else
  bad "build-archives.sh --dry-run 非零退出"
  arch_out=""
fi
expect_in "$arch_out" "dist/client-archives/warp-ztna_${VERSION_FILE}_linux_amd64.tar.gz" "archives dry-run 指向输出目录与六个产物"
expect_in "$arch_out" "dist/client-archives/warp-ztna_${VERSION_FILE}_windows_amd64.tar.gz" "archives dry-run 覆盖 Windows 组合"
expect_in "$arch_out" "wintun.dll" "archives dry-run 提示 Windows 归档带 wintun.dll"
expect_grep "$ARCHIVES_SH" "WINTUN_SHA256" "build-archives.sh 校验 wintun pin"

if pkg_out="$(sh "$MACPKG_SH" --dry-run --arch arm64 --archive x.tar.gz 2>&1)"; then
  ok "build-macpkg.sh --dry-run 退出码 0"
else
  bad "build-macpkg.sh --dry-run 非零退出"
  pkg_out=""
fi
expect_in "$pkg_out" "codesign --force --sign -" "载荷补 ad-hoc 签名"
expect_in "$pkg_out" "pkgbuild" "打组件包"
expect_in "$pkg_out" "productbuild" "打产品包"
expect_in "$pkg_out" "WarpZTNA-Service-${VERSION_FILE}-arm64.pkg" "pkg 产物名带 Service 与版本架构"

if dmg_out="$(sh "$DMG_SH" --dry-run --arch arm64 --app "/tmp/Warp ZTNA.app" --nodepkg "WarpZTNA-Service-${VERSION_FILE}-arm64.pkg" 2>&1)"; then
  ok "build-dmg.sh --dry-run 退出码 0"
else
  bad "build-dmg.sh --dry-run 非零退出"
  dmg_out=""
fi
expect_in "$dmg_out" "ditto" "用 ditto 拷 app"
expect_in "$dmg_out" "hdiutil create" "用 hdiutil 建镜像"
expect_in "$dmg_out" "-format ULFO" "压缩只读格式"
expect_in "$dmg_out" "WarpZTNA-App-${VERSION_FILE}-arm64.dmg" "dmg 产物名带 App 与版本架构"
expect_in "$dmg_out" "Applications" "带 Applications 软链"

if sh -n "$APP_INSTALL_SH"; then
  ok "install-app.sh 通过 sh -n"
else
  bad "install-app.sh 语法错误"
fi
if command -v shellcheck >/dev/null 2>&1; then
  if shellcheck --shell=sh "$APP_INSTALL_SH"; then
    ok "install-app.sh shellcheck 无告警"
  else
    bad "install-app.sh shellcheck 有告警"
  fi
fi
if app_out="$(sh "$APP_INSTALL_SH" --dry-run --version 9.9.9 2>&1)"; then
  ok "install-app.sh --dry-run 退出码 0"
else
  bad "install-app.sh --dry-run 非零退出"
  app_out=""
fi
expect_in "$app_out" "WarpZTNA-Service-9.9.9-" "install-app dry-run 引用 Service 包名"
expect_in "$app_out" "WarpZTNA-App-9.9.9-" "install-app dry-run 引用 App dmg 名"
expect_in "$app_out" "installer -pkg" "install-app 用 installer 装引擎"
expect_in "$app_out" "ditto" "install-app 用 ditto 拷 app"
expect_in "$app_out" "hdiutil" "install-app 用 hdiutil 挂载卸载"
APP_VERSION_SCRIPT="$(sed -n 's/^WARP_VERSION="\([^"]*\)"$/\1/p' "$APP_INSTALL_SH")"
if [ "$VERSION_FILE" = "$APP_VERSION_SCRIPT" ]; then
  ok "install-app.sh 内嵌 WARP_VERSION 与 VERSION 一致（${VERSION_FILE}）"
else
  bad "install-app.sh WARP_VERSION 不一致：文件 ${VERSION_FILE}，脚本 $APP_VERSION_SCRIPT"
fi
if app_default="$(sh "$APP_INSTALL_SH" --dry-run 2>&1)"; then
  ok "install-app.sh 无 --version 时用内嵌版本"
else
  bad "install-app.sh 无 --version 非零退出"
  app_default=""
fi
expect_in "$app_default" "WarpZTNA-App-${VERSION_FILE}-" "install-app 内嵌版本进入产物名"
expect_absent "$APP_INSTALL_SH" "netbird version" "install-app.sh 不读官方版本"
expect_grep "$APP_INSTALL_SH" "github.com/dayu-sec/warp-ztna-client/releases/download" "install-app.sh 默认取件基址是 Release 直链"
expect_absent "$APP_INSTALL_SH" "pkg.warp-ztna.internal" "install-app.sh 不再含旧内网域名"
expect_grep "$APP_INSTALL_SH" "service uninstall --service netbird" "install-app.sh 替换：注销官方服务"

if sh -n "$APP_LINUX_INSTALL_SH"; then
  ok "install-app-linux.sh 通过 sh -n"
else
  bad "install-app-linux.sh 语法错误"
fi
if command -v shellcheck >/dev/null 2>&1; then
  if shellcheck --shell=sh "$APP_LINUX_INSTALL_SH"; then
    ok "install-app-linux.sh shellcheck 无告警"
  else
    bad "install-app-linux.sh shellcheck 有告警"
  fi
fi
if linux_app_help="$(sh "$APP_LINUX_INSTALL_SH" --help 2>&1)"; then
  ok "install-app-linux.sh --help 退出码 0"
else
  bad "install-app-linux.sh --help 非零退出"
  linux_app_help=""
fi
for flag in "--version" "--base-url" "--dry-run" "--help"; do
  expect_in "$linux_app_help" "$flag" "install-app-linux.sh 帮助列出 ${flag}"
done
if sh "$APP_LINUX_INSTALL_SH" --not-a-flag >/dev/null 2>&1; then
  bad "install-app-linux.sh 未知参数应当非零退出"
else
  ok "install-app-linux.sh 未知参数非零退出"
fi
if linux_app_out="$(sh "$APP_LINUX_INSTALL_SH" --dry-run --version 9.9.9 2>&1)"; then
  ok "install-app-linux.sh --dry-run 退出码 0"
else
  bad "install-app-linux.sh --dry-run 非零退出"
  linux_app_out=""
fi
expect_in "$linux_app_out" "WarpZTNA-App-9.9.9-" "linux install-app dry-run 引用 App 包名"
expect_in "$linux_app_out" "client-v9.9.9/WarpZTNA-App-9.9.9-" "linux install-app dry-run 指向 Release 资产路径"
LINUX_APP_VERSION_SCRIPT="$(sed -n 's/^WARP_VERSION="\([^"]*\)"$/\1/p' "$APP_LINUX_INSTALL_SH")"
if [ "$VERSION_FILE" = "$LINUX_APP_VERSION_SCRIPT" ]; then
  ok "install-app-linux.sh 内嵌 WARP_VERSION 与 VERSION 一致（${VERSION_FILE}）"
else
  bad "install-app-linux.sh WARP_VERSION 不一致：文件 ${VERSION_FILE}，脚本 $LINUX_APP_VERSION_SCRIPT"
fi
expect_grep "$APP_LINUX_INSTALL_SH" "apt-get install -y" "linux install-app 走 apt-get"
expect_grep "$APP_LINUX_INSTALL_SH" "dpkg -i" "linux install-app 退路 dpkg -i"
expect_grep "$APP_LINUX_INSTALL_SH" "dnf install -y" "linux install-app 走 dnf"
expect_grep "$APP_LINUX_INSTALL_SH" "rpm -Uvh" "linux install-app 退路 rpm -Uvh"
expect_grep "$APP_LINUX_INSTALL_SH" "service uninstall --service netbird" "linux install-app 替换：注销官方服务"
expect_absent "$APP_LINUX_INSTALL_SH" "请先卸载官方" "linux install-app 不再把卸载交给用户"
expect_grep "$APP_LINUX_INSTALL_SH" "github.com/dayu-sec/warp-ztna-client/releases/download" "install-app-linux.sh 默认取件基址是 Release 直链"
expect_absent "$APP_LINUX_INSTALL_SH" "pkg.warp-ztna.internal" "install-app-linux.sh 不再含旧内网域名"
FAKE_BIN="$(mktemp -d)"
cat > "$FAKE_BIN/uname" <<'FAKE_UNAME'
#!/bin/sh
if [ "$1" = "-m" ]; then printf '%s\n' "$FAKE_UNAME_MACHINE"; else printf 'Linux\n'; fi
FAKE_UNAME
cat > "$FAKE_BIN/dpkg" <<'FAKE_DPKG'
#!/bin/sh
exit 0
FAKE_DPKG
chmod +x "$FAKE_BIN/uname" "$FAKE_BIN/dpkg"
for pair in "x86_64 amd64" "aarch64 arm64"; do
  machine="${pair%% *}"
  arch="${pair#* }"
  if arch_plan="$(FAKE_UNAME_MACHINE="$machine" PATH="$FAKE_BIN:$PATH" sh "$APP_LINUX_INSTALL_SH" --dry-run --version 9.9.9 2>&1)"; then
    expect_in "$arch_plan" "WarpZTNA-App-9.9.9-${arch}.deb" "linux install-app ${machine} 映射到 ${arch}"
  else
    bad "linux install-app ${machine} dry-run 非零退出"
  fi
done
if FAKE_UNAME_MACHINE=mips PATH="$FAKE_BIN:$PATH" sh "$APP_LINUX_INSTALL_SH" --dry-run >/dev/null 2>&1; then
  bad "linux install-app 未知架构应当非零退出"
else
  ok "linux install-app 未知架构非零退出"
fi
rm -rf "$FAKE_BIN"

PS1_INSTALL="$NODE_DIR/install.ps1"
PS_VERSION_SCRIPT="$(sed -n 's/^\$WarpVersion = "\([^"]*\)"$/\1/p' "$PS1_INSTALL")"
if [ "$VERSION_FILE" = "$PS_VERSION_SCRIPT" ]; then
  ok "install.ps1 内嵌 WarpVersion 与 VERSION 一致（${VERSION_FILE}）"
else
  bad "install.ps1 WarpVersion 不一致：文件 ${VERSION_FILE}，脚本 $PS_VERSION_SCRIPT"
fi
expect_grep "$PS1_INSTALL" "Replace-Official" "install.ps1 有官方即走替换"
expect_grep "$PS1_INSTALL" "service uninstall" "install.ps1 替换：注销官方服务"
expect_absent "$PS1_INSTALL" "Get-NetbirdVersion" "install.ps1 不读官方版本"
expect_grep "$PS1_INSTALL" "github.com/dayu-sec/warp-ztna-client/releases/download" "install.ps1 默认取件基址是 Release 直链"
expect_absent "$PS1_INSTALL" "pkg.warp-ztna.internal" "install.ps1 不再含旧内网域名"
expect_grep "$PS1_INSTALL" "Switch-ManagementState" "install.ps1 入网前清空既有配置"
expect_grep "$PS1_INSTALL" "Remove-ForeignLegacyConfig" "install.ps1 扫官方残留目录（ProgramData Netbird）"
expect_grep "$PS1_INSTALL" "Assert-ManagementTarget" "install.ps1 入网后复核 Management"
expect_grep "$PS1_INSTALL" "清空本机既有配置" "install.ps1 入网先清空说明"
expect_grep "$PS1_INSTALL" "入网后校验失败" "install.ps1 复核失败给出原始行"
expect_grep "$PS1_INSTALL" "先清空本机既有的 NetBird 配置" "install.ps1 usage 反映入网先清空语义"
expect_grep "$PS1_INSTALL" "wintun.dll" "install.ps1 安装 wintun.dll"
expect_grep "$PS1_INSTALL" "wintun-LICENSE.txt" "install.ps1 安装 wintun 许可"
if command -v pwsh >/dev/null 2>&1; then
  if pwsh -NoProfile -Command "\$null = [scriptblock]::Create((Get-Content -Raw '${PS1_INSTALL}'))" >/dev/null 2>&1; then
    ok "install.ps1 解析通过"
  else
    bad "install.ps1 解析失败"
  fi
  if ps_out="$(pwsh -NoProfile -File "$PS1_INSTALL" -DryRun -Version 9.9.9 2>&1)"; then
    ok "install.ps1 -DryRun 退出码 0"
  else
    bad "install.ps1 -DryRun 非零退出"
    ps_out=""
  fi
  expect_in "$ps_out" "warp-ztna_9.9.9_windows_amd64.tar.gz" "install.ps1 dry-run 引用 Windows 归档名"
  expect_in "$ps_out" "--service warp-ztna" "install.ps1 dry-run 带服务名"
  expect_in "$ps_out" "checksums.txt" "install.ps1 dry-run 核对校验和"
  expect_in "$ps_out" "无入网凭据" "install.ps1 dry-run 未带凭据时说明 state 保留"
  if ps_enroll="$(pwsh -NoProfile -File "$PS1_INSTALL" -DryRun -SetupKey S3CR3T-CANARY -ManagementUrl https://nb.example.com 2>&1)"; then
    expect_in "$ps_enroll" "入网前先清空本机既有配置" "install.ps1 dry-run 带凭据时给出清空计划（行为断言）"
    expect_not_in "$ps_enroll" "S3CR3T-CANARY" "install.ps1 dry-run 不回显密钥"
  else
    bad "install.ps1 带凭据的 dry-run 非零退出"
  fi
  if ps_no_url="$(WARP_ZTNA_MANAGEMENT_URL= pwsh -NoProfile -File "$PS1_INSTALL" -DryRun -SetupKey S3CR3T-CANARY 2>&1)"; then
    bad "install.ps1 带 setup key 但不带 ManagementUrl 应当非零退出"
    ps_no_url=""
  else
    ok "install.ps1 带 setup key 但不带 ManagementUrl 非零退出"
  fi
  expect_in "$ps_no_url" "必须给 -ManagementUrl" "install.ps1 缺管理地址时指路报错"
  expect_not_in "$ps_no_url" "S3CR3T-CANARY" "拒绝时也不回显密钥"
else
  printf '跳过  pwsh 未安装\n'
fi

LINUXPKG_SH="$APP_DIR/build-linuxpkg.sh"
if sh -n "$LINUXPKG_SH"; then
  ok "build-linuxpkg.sh 通过 sh -n"
else
  bad "build-linuxpkg.sh 语法错误"
fi
if command -v shellcheck >/dev/null 2>&1; then
  if shellcheck --shell=sh "$LINUXPKG_SH"; then
    ok "build-linuxpkg.sh shellcheck 无告警"
  else
    bad "build-linuxpkg.sh shellcheck 有告警"
  fi
fi
if lp_out="$(sh "$LINUXPKG_SH" --dry-run --arch amd64 --deb x.deb --archive x.tar.gz 2>&1)"; then
  ok "build-linuxpkg.sh --dry-run 退出码 0"
else
  bad "build-linuxpkg.sh --dry-run 非零退出"
  lp_out=""
fi
expect_in "$lp_out" "WarpZTNA-App-${VERSION_FILE}-amd64.deb" "linuxpkg dry-run 指向 deb 产物名"
expect_in "$lp_out" "WarpZTNA-App-${VERSION_FILE}-amd64.rpm" "linuxpkg dry-run 指向 rpm 产物名"
expect_in "$lp_out" "fpm" "linuxpkg 用 fpm 出 rpm"
expect_in "$lp_out" "--package" "linuxpkg dry-run 打印 fpm 的 --package"
expect_in "$lp_out" "Conflicts: netbird, netbird-ui" "linuxpkg 计划含 control 的 Conflicts"
expect_in "$lp_out" "preinst" "linuxpkg 计划含 preinst"
expect_in "$lp_out" "--before-install" "linuxpkg 的 fpm 带 --before-install"
expect_in "$lp_out" "--conflicts netbird" "linuxpkg 的 fpm 带 --conflicts"
case "$lp_out" in
  *" -C <stage> usr"*) ok "linuxpkg 的 fpm 位置参数收尾（flag 全在前）" ;;
  *) bad "linuxpkg 的 fpm 命令位置参数不在末尾——fpm 会把它后面的 flag 当路径" ;;
esac
if sh "$LINUXPKG_SH" --dry-run --arch amd64 --deb x.deb >/dev/null 2>&1; then
  bad "build-linuxpkg.sh 缺 --archive 应当非零退出"
else
  ok "build-linuxpkg.sh 缺 --archive 非零退出"
fi
expect_grep "$LINUXPKG_SH" "--service warp-ztna" "linuxpkg 的 postinst 注册 warp-ztna 服务"
expect_grep "$LINUXPKG_SH" "service uninstall --service netbird" "linuxpkg 的 preinst 注销官方服务"
expect_grep "$LINUXPKG_SH" "Conflicts: netbird, netbird-ui" "linuxpkg 生成 control 的 Conflicts 行"
expect_grep "$LINUXPKG_SH" "按升级处理" "linuxpkg 的 postinst 对既有服务走升级路径"
expect_grep "$LINUXPKG_SH" "NB_STATE_DIR=" "linuxpkg 的 postinst 透传 state 目录"
expect_grep "$LINUXPKG_SH" "没有 MANAGEMENT_URL" "linuxpkg 的 postinst 缺 MANAGEMENT_URL 时跳过入网并说明"
expect_grep "$LINUXPKG_SH" "清空本机既有配置" "linuxpkg 的 postinst 入网路径先清空既有配置"
expect_grep "$LINUXPKG_SH" "移除残留的旧配置" "linuxpkg 的 postinst 外科清理旁路旧配置（含旧版目录/旧键名）"
expect_grep "$LINUXPKG_SH" "当前激活配置指向其他管理服务器" "linuxpkg 的 postinst 激活份为他管时整清（文案）"
expect_grep "$LINUXPKG_SH" '/etc/netbird' "linuxpkg 的 postinst 兼顾旧版官方配置目录"

expect_grep "$POSTINSTALL" "--service warp-ztna" "postinstall 注册 warp-ztna 服务"
expect_grep "$POSTINSTALL" "按升级处理" "postinstall 对既有服务走升级路径（先停用并注销）"
expect_grep "$POSTINSTALL" 'STATE_DIR="/var/lib/warp-ztna"' "postinstall 带品牌 state 目录"
expect_grep "$POSTINSTALL" "NB_STATE_DIR=" "postinstall 透传 state 目录"
expect_grep "$POSTINSTALL" "NB_ENABLE_LOCAL_FORWARDING=true" "postinstall 开本机转发"
expect_grep "$POSTINSTALL" "--setup-key-file" "postinstall 经临时文件传密钥"
expect_grep "$POSTINSTALL" "没有 MANAGEMENT_URL" "postinstall 缺 MANAGEMENT_URL 时跳过入网并说明"
expect_grep "$POSTINSTALL" '"ManagementURL"' "postinstall 按真实键名解析既有配置（PascalCase）"
expect_grep "$POSTINSTALL" '"managementURL"' "postinstall 兼容 0.80 之前的键名形态"
expect_grep "$POSTINSTALL" "清空本机既有配置" "postinstall 入网路径先清空既有配置"
expect_grep "$POSTINSTALL" "指向其他管理服务器" "postinstall 无入网路径按他管清空（文案）"
expect_grep "$POSTINSTALL" "移除残留的旧配置" "postinstall 外科清理旁路旧配置（含旧版目录/旧键名）"
expect_grep "$POSTINSTALL" "当前激活配置指向其他管理服务器" "postinstall 激活份为他管时整清（文案）"
expect_grep "$POSTINSTALL" '/etc/netbird' "postinstall 兼顾旧版官方配置目录"
APP_LIB="$(dirname -- "$NODE_DIR")/warp-ztna-app/src-tauri/src/lib.rs"
APP_DEFAULT_URL="$(sed -n 's/.*DEFAULT_MANAGEMENT_URL: &str = "\([^"]*\)".*/\1/p' "$APP_LIB")"
if [ -z "$APP_DEFAULT_URL" ]; then
  bad "app lib.rs 缺 DEFAULT_MANAGEMENT_URL"
else
  for default_file in "$POSTINSTALL" "$LINUXPKG_SH" "$PS1_INSTALL" "$INSTALL_SH"; do
    file_url="$(grep -o 'https://ztna\.corp\.dy-sec\.com:443' "$default_file" | head -n 1)"
    if [ "$file_url" = "$APP_DEFAULT_URL" ]; then
      ok "${default_file##*/} 的默认管理服务器与 app DEFAULT_MANAGEMENT_URL 一字不差"
    else
      bad "${default_file##*/} 的默认管理服务器与 app 不一致（${file_url:-缺字面量} vs ${APP_DEFAULT_URL}）"
    fi
  done
fi
expect_absent "$POSTINSTALL" "--daemon-addr" "postinstall 不改 socket 地址"
for brandless in "$PREINSTALL" "$POSTINSTALL" "$INSTALL_SH" "$PS1_INSTALL" "$APP_INSTALL_SH" "$APP_LINUX_INSTALL_SH" "$LINUXPKG_SH"; do
  expect_absent "$brandless" "官方 NetBird" "${brandless##*/} 的输出不含 NetBird 品牌散文"
done

expect_grep "$PREINSTALL" "command -v netbird" "preinstall 以在否检测官方 NetBird"
expect_absent "$PREINSTALL" "netbird version" "preinstall 不看版本"
expect_grep "$PREINSTALL" "service uninstall --service netbird" "preinstall 替换：注销官方服务"
expect_grep "$PREINSTALL" 'NetBird UI.app' "preinstall 替换：清退官方 UI 应用"
if grep -q '^export PATH=.*/usr/local/bin' "$PREINSTALL"; then
  ok "preinstall 的 PATH 含官方安装位置（/usr/local/bin）"
else
  bad "preinstall 的 PATH 覆盖掉了 /usr/local/bin——替换动作会失效"
fi

expect_grep "$DISTRIBUTION" "<title>Warp ZTNA 引擎服务</title>" "安装器标题用统一名「Warp ZTNA 引擎服务」"
expect_grep "$DISTRIBUTION" 'enable_localSystem="true"' "Distribution 锁系统域"
expect_grep "$DISTRIBUTION" 'enable_currentUserHome="false"' "Distribution 关用户域"
expect_grep "$DISTRIBUTION" 'hostArchitectures="@ARCH@"' "Distribution 按架构拦错装机"
expect_grep "$DISTRIBUTION" 'Readme.html' "Distribution 引介绍页 readme"
expect_absent "$README_HTML" "NetBird" "介绍页不出现 NetBird 品牌（对外表面已去品牌）"
expect_grep "$README_HTML" "Warp ZTNA 引擎服务" "介绍页用统一名「Warp ZTNA 引擎服务」"
expect_grep "$README_HTML" "本机已安装引擎服务" "readme 覆盖已有 warp-ztna 的升级路径"
expect_grep "$README_HTML" "入网配置" "介绍页写明入网配置例外（先清空再入网）"
expect_grep "$README_HTML" "应用将以产品默认管理服务器开始" "介绍页写明无入网配置时按他管清空"

expect_grep "$MACOS_CONF" '"targets": \["app"\]' "macOS 只出 app（dmg 自建）"
expect_absent "$DMG_SH" "tauri build" "dmg 由脚本自建、不调 tauri"

RELEASE_WF="$NODE_DIR/../../.github/workflows/release-client.yml"
if [ ! -f "$RELEASE_WF" ]; then
  RELEASE_WF="$NODE_DIR/../../deploy/client-ci/workflows/release-client.yml"
fi

if grep -q -- "--no-sign" "$INSTALL_SH" "$ARCHIVES_SH" "$MACPKG_SH" "$DMG_SH" "$MACOS_CONF" "$RELEASE_WF" 2>/dev/null; then
  bad "出现了 --no-sign（会关掉 ad-hoc 签名）"
else
  ok "没有 --no-sign"
fi

expect_grep "$RELEASE_WF" "releases/download" "release 流水线的取件基址是 Release 资产直链"
expect_grep "$RELEASE_WF" "github.com/dayu-sec/warp-ztna-client" "发布到公开客户端仓"
expect_absent "$RELEASE_WF" "warp-ztna-pkg" "不再发布到共享产物仓"
expect_grep "$RELEASE_WF" "build-assets.sh" "流水线组装 Release 资产"
expect_grep "$RELEASE_WF" "dry_run" "流水线支持 dry_run 试跑"
expect_grep "$RELEASE_WF" "WINTUN_VERSION" "流水线按 pin 下载 wintun"
expect_grep "$RELEASE_WF" "TAURI_WINTUN_DLL" "流水线注入 wintun.dll 路径"
expect_grep "$RELEASE_WF" "TAURI_WINTUN_LICENSE" "流水线注入 wintun 许可路径"
expect_grep "$RELEASE_WF" "expected 2 (GUI)" "流水线断言 PE 子系统为 GUI"

SITE_SH="$NODE_DIR/../../deploy/client-pkg/build-assets.sh"
if sh -n "$SITE_SH"; then
  ok "build-assets.sh 通过 sh -n"
else
  bad "build-assets.sh 语法错误"
fi
if command -v shellcheck >/dev/null 2>&1; then
  if shellcheck --shell=sh "$SITE_SH"; then
    ok "build-assets.sh shellcheck 无告警"
  else
    bad "build-assets.sh shellcheck 有告警"
  fi
fi
if site_help="$(sh "$SITE_SH" --help 2>&1)"; then
  ok "build-assets.sh --help 退出码 0"
else
  bad "build-assets.sh --help 非零退出"
  site_help=""
fi
for flag in "--dist" "--out" "--dry-run" "--help"; do
  expect_in "$site_help" "$flag" "build-assets.sh 帮助列出 ${flag}"
done
if sh "$SITE_SH" --not-a-flag >/dev/null 2>&1; then
  bad "build-assets.sh 未知参数应当非零退出"
else
  ok "build-assets.sh 未知参数非零退出"
fi
if site_plan="$(sh "$SITE_SH" --dry-run --dist /nonexistent --out /nonexistent 2>&1)"; then
  ok "build-assets.sh --dry-run 退出码 0"
else
  bad "build-assets.sh --dry-run 非零退出"
  site_plan=""
fi
expect_in "$site_plan" "WarpZTNA-App-" "资产计划含桌面安装包"
expect_in "$site_plan" "checksums.txt" "资产计划含合并校验和"
SITE_WORK="$(mktemp -d)"
mkdir -p "$SITE_WORK/dist/client-archives" "$SITE_WORK/dist/client-macos" "$SITE_WORK/dist/client-windows" "$SITE_WORK/dist/client-linux" \
  "$SITE_WORK/dist/client-pkg/macos" "$SITE_WORK/dist/client-pkg/linux" "$SITE_WORK/dist/client-pkg/windows"
for f in "client-macos/WarpZTNA-App-${VERSION_FILE}-arm64.dmg" "client-macos/WarpZTNA-App-${VERSION_FILE}-amd64.dmg" \
  "client-macos/WarpZTNA-Service-${VERSION_FILE}-arm64.pkg" "client-macos/WarpZTNA-Service-${VERSION_FILE}-amd64.pkg" \
  "client-windows/WarpZTNA-App-${VERSION_FILE}-amd64.msi" "client-linux/WarpZTNA-App-${VERSION_FILE}-amd64.deb" \
  "client-linux/WarpZTNA-App-${VERSION_FILE}-amd64.rpm"; do
  printf 'x\n' > "$SITE_WORK/dist/$f"
done
for combo in linux_amd64 linux_arm64 darwin_amd64 darwin_arm64 windows_amd64 windows_arm64; do
  printf 'x\n' > "$SITE_WORK/dist/client-archives/warp-ztna_${VERSION_FILE}_${combo}.tar.gz"
done
printf 'x\n' > "$SITE_WORK/dist/client-archives/checksums.txt"
for p in macos/install.sh macos/install-app.sh linux/install.sh linux/install-app.sh windows/install.ps1; do
  printf 'x\n' > "$SITE_WORK/dist/client-pkg/$p"
done
if sh "$SITE_SH" --dist "$SITE_WORK/dist" --out "$SITE_WORK/assets" >/dev/null 2>&1; then
  ok "build-assets.sh 组装退出码 0"
else
  bad "build-assets.sh 组装非零退出"
fi
for rel in "WarpZTNA-App-${VERSION_FILE}-arm64.dmg" "WarpZTNA-Service-${VERSION_FILE}-amd64.pkg" \
  "WarpZTNA-App-${VERSION_FILE}-amd64.msi" "WarpZTNA-App-${VERSION_FILE}-amd64.deb" "WarpZTNA-App-${VERSION_FILE}-amd64.rpm" \
  "warp-ztna_${VERSION_FILE}_linux_amd64.tar.gz" "install.sh" "install-app.sh" "install-app-linux.sh" \
  "install.ps1" "checksums.txt"; do
  if [ -f "$SITE_WORK/assets/$rel" ]; then
    ok "资产目录含 $rel"
  else
    bad "资产目录缺 $rel"
  fi
done
if (cd "$SITE_WORK/assets" && { sha256sum -c checksums.txt >/dev/null 2>&1 || shasum -a 256 -c checksums.txt >/dev/null 2>&1; }); then
  ok "合并校验和可校验（sha256sum -c）"
else
  bad "合并校验和校验失败"
fi
rm -rf "$SITE_WORK"

WXS="$NODE_DIR/../warp-ztna-app/src-tauri/wix/engine.wxs"
expect_grep "$WXS" "ServiceInstall" "wxs 声明 ServiceInstall"
expect_grep "$WXS" "service run --log-level info --daemon-addr npipe://netbird" "wxs 的 binPath 参数带 service run"
expect_grep "$WXS" 'Type="multiString"' "wxs 用 multiString 写服务环境"
expect_grep "$WXS" 'NB_STATE_DIR=C:\\ProgramData\\warp-ztna' "wxs 环境含品牌 state 目录"
expect_grep "$WXS" "WarpZtnaStateDir" "wxs 声明 state 目录组件"
expect_grep "$WXS" '<DirectoryRef Id="TARGETDIR">' "wxs 显式声明 CommonAppDataFolder 的父目录（WiX 3 不自动解析标准目录）"
expect_grep "$WXS" '<Directory Id="CommonAppDataFolder" />' "wxs 声明 CommonAppDataFolder 本身"
expect_grep "$WXS" 'WarpZtnaStateDir" Guid="177f507c-4e8d-4688-aa6b-d5780beebc82"' "state 目录组件用显式 GUID（目录作 KeyPath 不吃自动 GUID）"
expect_grep "$WXS" "TAURI_ENGINE_EXE" "wxs 引擎文件来自 CI 传入的绝对路径"
expect_grep "$WXS" "TAURI_WINTUN_DLL" "wxs 带 wintun.dll"
expect_grep "$WXS" "TAURI_WINTUN_LICENSE" "wxs 带 wintun 许可"
expect_grep "$APP_MAIN" 'windows_subsystem = "windows"' "Desktop release 用 GUI 子系统（不弹 cmd）"

MAIN_WXS="$NODE_DIR/../warp-ztna-app/src-tauri/wix/main.wxs"
expect_grep "$MAIN_WXS" "WarpCleanupForeignState" "MSI 含外域状态清理动作"
expect_grep "$MAIN_WXS" 'Return="ignore"' "MSI 清理动作失败不阻断安装"
expect_grep "$MAIN_WXS" 'Before="WarpReplaceOfficialNetbird">NOT Installed' "MSI 清理在安装前执行（替换官方之后）"
expect_grep "$MAIN_WXS" "ManagementURL" "MSI 清理按真实键名解析既有配置"
expect_grep "$MAIN_WXS" '"managementURL"' "MSI 清理兼容旧版键名形态"
expect_grep "$MAIN_WXS" "active_profile.json" "MSI 清理识别激活 profile"
expect_grep "$MAIN_WXS" "https://ztna.corp.dy-sec.com:443" "MSI 清理带产品默认域（全 URL 形态）"
expect_grep "$MAIN_WXS" "warp-ztna-install-cleanup.log" "MSI 清理输出日志供现场诊断"
MAIN_WXS_URL="$(grep -o 'https://ztna\.corp\.dy-sec\.com:443' "$MAIN_WXS" | head -n 1)"
if [ "$MAIN_WXS_URL" = "$APP_DEFAULT_URL" ]; then
  ok "main.wxs 的默认管理服务器与 app DEFAULT_MANAGEMENT_URL 一字不差"
else
  bad "main.wxs 的默认管理服务器与 app 不一致（${MAIN_WXS_URL:-缺字面量} vs ${APP_DEFAULT_URL}）"
fi

COMPOSE_SH="$(cd -- "$NODE_DIR/../.." && pwd)/deploy/client-ci/compose.sh"
if [ -f "$COMPOSE_SH" ]; then
  GATE_IDENTITY="$(sed -n "s/^IDENTITY_PATTERNS='\(.*\)'$/\1/p" "$COMPOSE_SH")"
  GATE_ALLOW="$(sed -n "s/^IDENTITY_ALLOW_PATTERNS='\(.*\)'$/\1/p" "$COMPOSE_SH")"
  if [ -z "$GATE_ALLOW" ]; then
    bad "compose.sh 缺 IDENTITY_ALLOW_PATTERNS（快照扫描门）"
  else
    GATE_TMP="$(mktemp)"
    for scan_file in "$POSTINSTALL" "$PREINSTALL" "$INSTALL_SH" "$PS1_INSTALL" "$APP_INSTALL_SH" "$APP_LINUX_INSTALL_SH" "$LINUXPKG_SH" "$MACPKG_SH" "$ARCHIVES_SH" "$MAIN_WXS" "$NODE_DIR/tests/smoke.sh"; do
      grep -HnE "$GATE_IDENTITY" "$scan_file" 2>/dev/null || true
    done | grep -vE "$GATE_ALLOW" > "$GATE_TMP" || true
    if [ -s "$GATE_TMP" ]; then
      bad "客户端面脚本含未放行的内网标识（镜像快照扫描门会红，参考 client 0.1.4 教训）"
      sed -n '1,5p' "$GATE_TMP" >&2
    else
      ok "客户端面脚本的内网标识全部落在快照放行串内"
    fi
    rm -f "$GATE_TMP"
  fi
fi

for f in "$INSTALL_SH" "$APP_INSTALL_SH" "$APP_LINUX_INSTALL_SH"; do
  expect_grep "$f" "--retry-all-errors" "$(basename -- "$f") 的下载带重试与断点续传"
done
expect_grep "$PS1_INSTALL" "Get-RemoteFile" "install.ps1 的下载走重试封装"

printf '\n%s 项，%s 失败\n' "$TOTAL" "$FAILED"
[ "$FAILED" -eq 0 ]
