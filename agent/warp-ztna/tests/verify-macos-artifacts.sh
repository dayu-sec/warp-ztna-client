#!/bin/sh
set -eu

DIR="${1:-}"
if [ -z "$DIR" ] || [ ! -d "$DIR" ]; then
  printf '用法：verify-macos-artifacts.sh <产物目录>\n' >&2
  exit 1
fi

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

check_pkg() {
  PKG="$1"
  NAME="${PKG##*/}"
  printf '%s\n' "--- $NAME ---"

  if pkgutil --check-signature "$PKG" 2>/dev/null | grep -q "no signature"; then
    ok "$NAME 未签名（预期）"
  else
    bad "$NAME 签名状态与预期不符（应无签名）"
  fi

  if pkgutil --payload-files "$PKG" 2>/dev/null | grep -q "usr/local/lib/warp-ztna/netbird"; then
    ok "$NAME 载荷含 netbird"
  else
    bad "$NAME 载荷缺 netbird"
  fi

  DOMINFO="$(installer -dominfo -pkg "$PKG" 2>/dev/null || true)"
  case "$DOMINFO" in
    *LocalSystem*) ok "$NAME 允许系统域" ;;
    *) bad "$NAME 域设置不符（缺 LocalSystem）" ;;
  esac
  case "$DOMINFO" in
    *CurrentUserHome*|*Anywhere*) bad "$NAME 域设置不符（出现了非系统域）" ;;
    *) ok "$NAME 只开放系统域" ;;
  esac

  EXPAND="$(mktemp -d)"
  if pkgutil --expand-full "$PKG" "$EXPAND/expanded" >/dev/null 2>&1; then
    BIN="$(find "$EXPAND" -path "*/netbird" -type f | head -n 1)"
    if [ -n "$BIN" ]; then
      SIG="$(codesign -dv --verbose=4 "$BIN" 2>&1 || true)"
      case "$SIG" in
        *linker-signed*) bad "$NAME 载荷二进制仍是 linker-signed 残桩" ;;
        *"flags=0x2(adhoc)"*) ok "$NAME 载荷二进制是完整 ad-hoc 签名" ;;
        *) bad "$NAME 载荷二进制签名形态意外" ;;
      esac
      if codesign --verify --strict "$BIN" >/dev/null 2>&1; then
        ok "$NAME 载荷二进制签名校验通过"
      else
        bad "$NAME 载荷二进制签名校验失败"
      fi
    else
      bad "$NAME 展开后找不到 netbird"
    fi
  else
    bad "$NAME pkgutil --expand-full 失败"
  fi
  rm -rf "$EXPAND"
}

check_dmg() {
  DMG="$1"
  NAME="${DMG##*/}"
  printf '%s\n' "--- $NAME ---"

  if hdiutil verify "$DMG" >/dev/null 2>&1; then
    ok "$NAME 镜像校验通过"
  else
    bad "$NAME 镜像校验失败"
  fi

  MP="$(mktemp -d)"
  if ! hdiutil attach -nobrowse -readonly -mountpoint "$MP" "$DMG" >/dev/null 2>&1; then
    bad "$NAME 挂载失败"
    rmdir "$MP" 2>/dev/null || true
    return 0
  fi

  if [ -d "$MP/Warp ZTNA.app" ]; then
    ok "$NAME 含 Warp ZTNA.app"
    if codesign --verify --deep --strict "$MP/Warp ZTNA.app" >/dev/null 2>&1; then
      ok "$NAME 的 app 签名校验通过"
    else
      bad "$NAME 的 app 签名校验失败"
    fi
  else
    bad "$NAME 缺 Warp ZTNA.app"
  fi

  if ls "$MP"/WarpZTNA-*.pkg >/dev/null 2>&1; then
    ok "$NAME 含引擎包"
  else
    bad "$NAME 缺引擎包"
  fi

  if [ -f "$MP/安装说明.txt" ]; then
    ok "$NAME 含安装说明"
  else
    bad "$NAME 缺安装说明"
  fi

  if [ -L "$MP/Applications" ]; then
    ok "$NAME 含 Applications 软链"
  else
    bad "$NAME 缺 Applications 软链"
  fi

  hdiutil detach "$MP" >/dev/null 2>&1 || true
  rmdir "$MP" 2>/dev/null || true
}

if [ -f "$DIR/checksums-macos.txt" ]; then
  if (cd "$DIR" && shasum -a 256 -c checksums-macos.txt >/dev/null 2>&1); then
    ok "checksums-macos.txt 在产物目录内自校验通过（裸文件名）"
  else
    bad "checksums-macos.txt 在产物目录内自校验失败（文件名带路径前缀？）"
  fi
fi

FOUND=0
for pkg in "$DIR"/WarpZTNA-*.pkg; do
  [ -f "$pkg" ] || continue
  FOUND=1
  check_pkg "$pkg"
done
for dmg in "$DIR"/WarpZTNA-*.dmg; do
  [ -f "$dmg" ] || continue
  FOUND=1
  check_dmg "$dmg"
done

if [ "$FOUND" -eq 0 ]; then
  bad "目录里没有任何 WarpZTNA-*.pkg / *.dmg"
fi

printf '\n%s 项，%s 失败\n' "$TOTAL" "$FAILED"
[ "$FAILED" -eq 0 ]
