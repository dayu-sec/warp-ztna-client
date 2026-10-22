#Requires -Version 5.1
[CmdletBinding()]
param(
    [switch]$Purge,
    [switch]$DryRun,
    [switch]$Help
)

$ErrorActionPreference = "Stop"

$ServiceName = "warp-ztna"
$ProductName = "Warp ZTNA"
$ProgramRoot = if ($env:ProgramFiles) { $env:ProgramFiles } else { "C:\Program Files" }
$DataRoot = if ($env:ProgramData) { $env:ProgramData } else { "C:\ProgramData" }
$InstallDir = "$ProgramRoot\Warp ZTNA"
$EngineBinary = Join-Path $InstallDir "netbird.exe"
$StateDir = "$DataRoot\warp-ztna"
$RunKey = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run"
$StartupApprovedKey = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run"
$AppConfigDir = Join-Path $env:APPDATA "com.dy-sec.warpztna.app"
$AppLogDir = Join-Path $env:LOCALAPPDATA "com.dy-sec.warpztna.app\logs"

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
用法：powershell -ExecutionPolicy Bypass -File uninstall-warp-windows.ps1 [-Purge] [-DryRun]

卸载 Warp 桌面应用与 Warp ZTNA 引擎服务（warp-ztna）。默认保留 state（${StateDir}）与当前用户的 app 配置；-Purge 连它们一起删。
先看将执行的动作：加 -DryRun。本脚本不处理其他引擎的物料。
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

function Get-WarpMsiProducts {
    $roots = @(
        "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
        "HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall"
    )
    foreach ($root in $roots) {
        if (-not (Test-Path $root)) { continue }
        foreach ($key in Get-ChildItem -Path $root -ErrorAction SilentlyContinue) {
            $props = Get-ItemProperty -Path $key.PSPath -ErrorAction SilentlyContinue
            if ($props -and $props.DisplayName -like "$ProductName*") { $props.PSChildName }
        }
    }
}

function Get-OfficialNetbird {
    $command = Get-Command "netbird.exe" -ErrorAction SilentlyContinue
    if ($command) { return $command.Source }
    $candidates = @((Join-Path $env:ProgramFiles "NetBird\netbird.exe"))
    $programFilesX86 = ${env:ProgramFiles(x86)}
    if ($programFilesX86) { $candidates += (Join-Path $programFilesX86 "NetBird\netbird.exe") }
    foreach ($candidate in $candidates) {
        if (Test-Path $candidate) { return $candidate }
    }
    return $null
}

if ($Help) { Show-Usage; exit 0 }

if ($PSVersionTable.PSVersion.Major -ge 6) {
    if (-not $IsWindows) { Fail "本脚本只用于 Windows；macOS 用 uninstall-warp-macos.sh，Linux 用 uninstall-warp-linux.sh" }
} elseif ($env:OS -ne "Windows_NT") {
    Fail "本脚本只用于 Windows；macOS 用 uninstall-warp-macos.sh，Linux 用 uninstall-warp-linux.sh"
}

$Verb = if ($DryRun) { "将删除" } else { "已删除" }

Write-Note "== 1) 盘点现状 =="
$service = Get-Service -Name $ServiceName -ErrorAction SilentlyContinue
if ($service) { Write-Note "  服务：${ServiceName}（$($service.Status)）" }
if (Test-Path $EngineBinary) { Write-Note "  引擎二进制：${EngineBinary}" }
if (Test-Path $InstallDir) { Write-Note "  安装目录：${InstallDir}" }
$products = @(Get-WarpMsiProducts)
foreach ($product in $products) { Write-Note "  MSI 产品：${product}" }
$runValue = Get-ItemProperty -Path $RunKey -Name $ProductName -ErrorAction SilentlyContinue
if ($runValue) { Write-Note "  自启项：HKCU Run 值「${ProductName}」" }
if (Test-Path $AppConfigDir) { Write-Note "  app 配置目录：${AppConfigDir}" }
if (Test-Path $StateDir) { Write-Note "  state 目录：${StateDir}" }
$official = Get-OfficialNetbird
if ($official) {
    Write-Note "  发现旧引擎（${official}）——本脚本不触碰它"
} else {
    Write-Note "  未发现旧引擎"
}
if (-not (Test-Path $InstallDir) -and $products.Count -eq 0) {
    Write-Note "  未发现 Warp 应用与引擎——可能已卸载过"
}

if (-not $DryRun) {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = New-Object Security.Principal.WindowsPrincipal($identity)
    if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
        Fail "需要管理员权限：请以管理员身份打开 PowerShell 后重跑"
    }
}

Write-Note "== 2) 停用并注销 Warp ZTNA 引擎服务（warp-ztna） =="
if (Test-Path $EngineBinary) {
    Invoke-Step "${EngineBinary} service stop --service ${ServiceName}" { & $EngineBinary service stop --service $ServiceName }
    Invoke-Step "${EngineBinary} service uninstall --service ${ServiceName}" { & $EngineBinary service uninstall --service $ServiceName }
} else {
    Write-Note "  跳过：未发现 Warp ZTNA 引擎服务（${EngineBinary}）"
    if ($service) {
        Invoke-Step "sc.exe stop ${ServiceName}" { & sc.exe stop $ServiceName }
        Invoke-Step "sc.exe delete ${ServiceName}" { & sc.exe delete $ServiceName }
    }
}

Write-Note "== 3) 退出并删除应用与引擎 =="
$appProcess = Get-Process -Name $ProductName -ErrorAction SilentlyContinue
if ($appProcess) {
    Invoke-Step "结束 Warp ZTNA 进程（Stop-Process）" { $appProcess | Stop-Process -Force }
}
if ($products.Count -gt 0) {
    foreach ($product in $products) {
        Invoke-Step "msiexec /x ${product} /qn /norestart" {
            $proc = Start-Process -FilePath msiexec.exe -ArgumentList @("/x", $product, "/qn", "/norestart") -Wait -PassThru
            if ($proc.ExitCode -ne 0) { Write-Note "  msiexec 退出码 $($proc.ExitCode)——请到「应用和功能」核对" }
        }
    }
    Remove-PathForce $InstallDir "Remove-Item ${InstallDir} -Recurse -Force（MSI 卸载后的残留）"
} else {
    Remove-PathForce $InstallDir "Remove-Item ${InstallDir} -Recurse -Force"
}

Write-Note "== 4) 当前用户侧 =="
$runValue = Get-ItemProperty -Path $RunKey -Name $ProductName -ErrorAction SilentlyContinue
if ($runValue) {
    Invoke-Step "删除 HKCU Run 自启值「${ProductName}」" { Remove-ItemProperty -Path $RunKey -Name $ProductName -Force }
} else {
    Write-Note "  跳过：未发现自启值（或当前不是使用该 app 的 Windows 账户）"
}
$startupApproved = Get-ItemProperty -Path $StartupApprovedKey -Name $ProductName -ErrorAction SilentlyContinue
if ($startupApproved) {
    Invoke-Step "删除 StartupApproved 记录「${ProductName}」" { Remove-ItemProperty -Path $StartupApprovedKey -Name $ProductName -Force }
}
if ($Purge) {
    Remove-PathForce $AppConfigDir "Remove-Item ${AppConfigDir} -Recurse -Force（-Purge）"
    Write-Note "  ${Verb}用户配置（-Purge）：${AppConfigDir}"
} else {
    Write-Note "  已保留用户配置：${AppConfigDir}（连它一起清：重跑加 -Purge）"
}
Write-Note "  提示：app 日志在 ${AppLogDir}，脚本不删"

Write-Note "== 5) state =="
if ($Purge) {
    Remove-PathForce $StateDir "Remove-Item ${StateDir} -Recurse -Force（-Purge）"
    Write-Note "  ${Verb} state（-Purge）：${StateDir}"
} else {
    Write-Note "  已保留：${StateDir}（设备身份；连身份一起清：重跑加 -Purge）"
}
Write-Note "  提示：${StateDir}\service.json 跨卸载存活，重装可能静默恢复旧参数；改参数用引擎 CLI 的 service reconfigure"

Write-Note "== 6) 核对 =="
$serviceAfter = Get-Service -Name $ServiceName -ErrorAction SilentlyContinue
if ($serviceAfter) {
    Write-Note "  服务残留            ->  ${ServiceName}（$($serviceAfter.Status)）"
} else {
    Write-Note "  服务残留            ->  （无）"
}
if (Test-Path $EngineBinary) { Write-Note "  引擎残留            ->  ${EngineBinary}" } else { Write-Note "  引擎残留            ->  （无）" }
$productsAfter = @(Get-WarpMsiProducts)
if ($productsAfter.Count -gt 0) {
    foreach ($product in $productsAfter) { Write-Note "  MSI 记录残留        ->  ${product}" }
} else {
    Write-Note "  MSI 记录残留        ->  （无）"
}
$officialAfter = Get-OfficialNetbird
if ($officialAfter) { Write-Note "  旧引擎完好   ->  ${officialAfter}" } else { Write-Note "  旧引擎完好   ->  （未装）" }
if (Test-Path $StateDir) { Write-Note "  state 目录          ->  ${StateDir}" } else { Write-Note "  state 目录          ->  （无）" }
