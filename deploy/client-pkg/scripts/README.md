# 卸载与旧引擎清理脚本

本目录提供 Warp ZTNA 的卸载脚本，以及替换/清理场景下的旧引擎清理脚本。脚本只从本仓 `main` 获取；执行前先打开脚本核对。

## 脚本

| 场景 | macOS | Linux | Windows |
|---|---|---|---|
| 卸载 Warp 桌面应用与引擎服务 | `uninstall-warp-macos.sh` | `uninstall-warp-linux.sh` | `uninstall-warp-windows.ps1` |
| 清理旧引擎（替换/清理场景） | `uninstall-old-engine-macos.sh` | `uninstall-old-engine-linux.sh` | `uninstall-old-engine-windows.ps1` |

## 使用

macOS / Linux：

```sh
sudo sh uninstall-warp-macos.sh --dry-run
sudo sh uninstall-warp-macos.sh
```

Windows（管理员 PowerShell）：

```powershell
powershell -ExecutionPolicy Bypass -File .\uninstall-warp-windows.ps1 -DryRun
powershell -ExecutionPolicy Bypass -File .\uninstall-warp-windows.ps1
```

- 默认保留 state、配置和日志，方便重装；`--purge` / `-Purge` 会删除设备身份、配置和日志。
- 旧引擎清理只在确认要替换或清理旧引擎时使用；它也会移除旧引擎的服务、应用或安装包。
- 先 `--dry-run` / `-DryRun` 看将执行的动作，再实际执行。
- macOS / Linux 需要 root，Windows 需要管理员 PowerShell。

## 说明

本仓不附许可文件，脚本按现状提供，仅供查看与构建；执行前请自行核对内容并确认影响范围。各平台安装页有对应的卸载说明。
