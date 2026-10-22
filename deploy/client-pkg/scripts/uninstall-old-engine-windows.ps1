#Requires -Version 5.1
[CmdletBinding()]
param(
    [switch]$Purge,
    [switch]$DryRun,
    [switch]$Help
)

$ErrorActionPreference = "Stop"

$ServiceName = "warp-ztna"
$NetBirdDir = "$env:ProgramFiles\NetBird"
$NetBirdExe = Join-Path $NetBirdDir "netbird.exe"
$NetBirdUninstaller = Join-Path $NetBirdDir "netbird_uninstall.exe"
$NetBirdStateDir = "$env:ProgramData\Netbird"
$WarpInstallDir = "$env:ProgramFiles\Warp ZTNA"
$WarpEngineBinary = Join-Path $WarpInstallDir "netbird.exe"
$WarpStateDir = "$env:ProgramData\warp-ztna"

function Write-Note {
    param([string]$Message)
    Write-Host $Message
}

function Fail {
    param([string]$Message)
    Write-Error "错误：$Message"
    exit 1
}

function Show-Usage {
    @"
用法：powershell -ExecutionPolicy Bypass -File uninstall-old-engine-windows.ps1 [-Purge] [-DryRun]

清理旧引擎与 Warp 的 warp-ztna 服务残留（替换场景：清干净后装 Warp）。默认保留 state（${NetBirdStateDir}、${WarpStateDir}）；-Purge 连它们一起删。
先看将执行的动作：加 -DryRun。Warp 的文件归 MSI / install.ps1 管，本脚本不动。
"@ | Write-Host
}

function Invoke-Step {
    param([string]$Plan, [scriptblock]$Action)
    if ($DryRun) {
        Write-Note "  [dry-run] ${Plan}"
        return
    }
    & $Action
}

function Remove-PathForce {
    param([string]$Path, [string]$Plan)
    if ($DryRun) {
        Write-Note "  [dry-run] ${Plan}"
        return
    }
    if (Test-Path $Path) { Remove-Item -Path $Path -Recurse -Force }
}

function Get-NetBirdMsiProducts {
    $roots = @(
        "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
        "HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall"
    )
    foreach ($root in $roots) {
        if (-not (Test-Path $root)) { continue }
        foreach ($key in Get-ChildItem -Path $root -ErrorAction SilentlyContinue) {
            $props = Get-ItemProperty -Path $key.PSPath -ErrorAction SilentlyContinue
            if ($props -and $props.DisplayName -like "*NetBird*") { $props.PSChildName }
        }
    }
}

if ($Help) { Show-Usage; exit 0 }

if ($PSVersionTable.PSVersion.Major -ge 6) {
    if (-not $IsWindows) { Fail "本脚本只用于 Windows；macOS 用 uninstall-old-engine-macos.sh，Linux 用 uninstall-old-engine-linux.sh" }
} elseif ($env:OS -ne "Windows_NT") {
    Fail "本脚本只用于 Windows；macOS 用 uninstall-old-engine-macos.sh，Linux 用 uninstall-old-engine-linux.sh"
}

$Verb = if ($DryRun) { "将删除" } else { "已删除" }

Write-Note "== 1) 盘点现状 =="
$services = @(Get-Service -Name "*NetBird*" -ErrorAction SilentlyContinue)
foreach ($svc in $services) { Write-Note "  旧引擎服务：$($svc.Name)（$($svc.Status)）" }
if (Test-Path $NetBirdExe) { Write-Note "  旧引擎可执行文件：${NetBirdExe}" }
if (Test-Path $NetBirdUninstaller) { Write-Note "  旧引擎卸载器：${NetBirdUninstaller}" }
$products = @(Get-NetBirdMsiProducts)
foreach ($product in $products) { Write-Note "  MSI 产品：${product}" }
if (Test-Path $NetBirdStateDir) { Write-Note "  state 目录：${NetBirdStateDir}" }
$warpService = Get-Service -Name $ServiceName -ErrorAction SilentlyContinue
if ($warpService) { Write-Note "  warp-ztna 服务：$($warpService.Status)" }
if (Test-Path $WarpInstallDir) { Write-Note "  Warp 安装目录：${WarpInstallDir}" }
if (Test-Path $WarpStateDir) { Write-Note "  Warp state：${WarpStateDir}" }
if ($services.Count -eq 0 -and -not (Test-Path $NetBirdDir)) { Write-Note "  未发现旧引擎——可能已卸载过" }

if (-not $DryRun) {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = New-Object Security.Principal.WindowsPrincipal($identity)
    if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
        Fail "需要管理员权限：请以管理员身份打开 PowerShell 后重跑"
    }
}

Write-Note "== 2) 停用并注销 Warp ZTNA 引擎服务（warp-ztna） =="
if (Test-Path $WarpEngineBinary) {
    Invoke-Step "${WarpEngineBinary} service stop --service ${ServiceName}" { & $WarpEngineBinary service stop --service $ServiceName }
    Invoke-Step "${WarpEngineBinary} service uninstall --service ${ServiceName}" { & $WarpEngineBinary service uninstall --service $ServiceName }
} elseif ($warpService) {
    Invoke-Step "sc.exe stop ${ServiceName}" { & sc.exe stop $ServiceName }
    Invoke-Step "sc.exe delete ${ServiceName}" { & sc.exe delete $ServiceName }
} else {
    Write-Note "  跳过：未发现 warp-ztna 服务与引擎"
}

Write-Note "== 3) 停用并注销旧引擎 =="
foreach ($svc in $services) {
    Invoke-Step "Stop-Service $($svc.Name)" { Stop-Service -Name $svc.Name -Force -ErrorAction SilentlyContinue }
}
if (Test-Path $NetBirdExe) {
    Invoke-Step "${NetBirdExe} service uninstall" { & $NetBirdExe service uninstall }
}
Invoke-Step "结束旧引擎相关进程" { Get-Process -ErrorAction SilentlyContinue | Where-Object { $_.Name -like "*netbird*" } | Stop-Process -Force -ErrorAction SilentlyContinue }
if (Test-Path $NetBirdUninstaller) {
    Invoke-Step "运行旧引擎卸载器 /S" {
        $proc = Start-Process -FilePath $NetBirdUninstaller -ArgumentList @("/S") -Wait -PassThru
        if ($proc.ExitCode -ne 0) { Write-Note "  卸载器退出码 $($proc.ExitCode)" }
    }
} elseif ($products.Count -gt 0) {
    foreach ($product in $products) {
        Invoke-Step "msiexec /x ${product} /qn /norestart" {
            $proc = Start-Process -FilePath msiexec.exe -ArgumentList @("/x", $product, "/qn", "/norestart") -Wait -PassThru
            if ($proc.ExitCode -ne 0) { Write-Note "  msiexec 退出码 $($proc.ExitCode)——请到「应用和功能」核对" }
        }
    }
} else {
    Write-Note "  未发现旧引擎卸载器与 MSI 记录"
}

Write-Note "== 4) 目录与残骸 =="
if (Test-Path $NetBirdDir) {
    Remove-PathForce $NetBirdDir "Remove-Item ${NetBirdDir} -Recurse -Force（卸载后的残骸）"
} else {
    Write-Note "  旧引擎目录已不在：${NetBirdDir}"
}
Write-Note "  （Warp 桌面应用与引擎文件归 MSI / install.ps1 管，本脚本不动；要清 Warp 用 uninstall-warp-windows.ps1）"

Write-Note "== 5) 状态 =="
if ($Purge) {
    Remove-PathForce $NetBirdStateDir "Remove-Item ${NetBirdStateDir} -Recurse -Force（-Purge）"
    Remove-PathForce $WarpStateDir "Remove-Item ${WarpStateDir} -Recurse -Force（-Purge）"
    Write-Note "  ${Verb} state（-Purge）：${NetBirdStateDir}、${WarpStateDir}"
} else {
    Write-Note "  已保留 state：${NetBirdStateDir}、${WarpStateDir}（连身份一起清：重跑加 -Purge）"
}

Write-Note "== 6) 核对 =="
$servicesAfter = @(Get-Service -Name "*NetBird*" -ErrorAction SilentlyContinue)
if ($servicesAfter.Count -gt 0) {
    foreach ($svc in $servicesAfter) { Write-Note "  旧引擎服务残留    ->  $($svc.Name)（$($svc.Status)）" }
} else {
    Write-Note "  旧引擎服务残留    ->  （无）"
}
if (Test-Path $NetBirdDir) { Write-Note "  旧引擎目录        ->  ${NetBirdDir}" } else { Write-Note "  旧引擎目录        ->  （无）" }
$productsAfter = @(Get-NetBirdMsiProducts)
if ($productsAfter.Count -gt 0) {
    foreach ($product in $productsAfter) { Write-Note "  MSI 记录残留        ->  ${product}" }
} else {
    Write-Note "  MSI 记录残留        ->  （无）"
}
$warpAfter = Get-Service -Name $ServiceName -ErrorAction SilentlyContinue
if ($warpAfter) { Write-Note "  warp-ztna 服务      ->  $($warpAfter.Status)" } else { Write-Note "  warp-ztna 服务      ->  （无）" }
if (Test-Path $NetBirdStateDir) { Write-Note "  state 目录          ->  ${NetBirdStateDir}" } else { Write-Note "  state 目录          ->  （无）" }
