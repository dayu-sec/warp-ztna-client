# warp-ztna：routing-peer 安装包

Warp ZTNA 的**机器形态客户端**（routing peer）。载荷是**官方 NetBird 发行二进制**——未改源码、
不 patch、不 vendor；Warp 只做两件事：品牌化系统服务注册与一次性安装脚本。终端用户形态是另一个包
（`agent/warp-ztna-app` 的 `Warp ZTNA.app`），本包没有 UI。

## 命名空间

| 项 | 值 |
|---|---|
| 服务名 | `warp-ztna`（systemd unit / launchd label） |
| 二进制 | `/usr/local/lib/warp-ztna/netbird`（**不进 PATH**，避免与 `/usr/bin/netbird`、brew 抢名字） |
| state | `/var/lib/warp-ztna`（经 `--service-env NB_STATE_DIR`） |
| 日志 | `/var/log/warp-ztna/client.log` |
| 配置目录 | `/etc/warp-ztna` |
| socket | **保留官方默认** `unix:///var/run/netbird.sock`（不传 `--daemon-addr`） |

## 用法

```sh
curl -fsSL <pkg-host>/linux/install.sh | sudo sh -s -- \
  --setup-key-file /path/to/key --management-url https://netbird.example.com
```

`--setup-key` 会被脚本转成 `0600` 临时文件、用后删除；首选 `--setup-key-file` 或
`WARP_ZTNA_SETUP_KEY`，避免密钥进 argv 与 shell 历史。

### 参数

| 参数 | 说明 |
|---|---|
| `--setup-key <KEY>` | 机器入网凭据（一次性） |
| `--setup-key-file <PATH>` | 从文件读 key，优先于 `--setup-key` |
| `--management-url <URL>` | NetBird Management 地址；setup-key 入网必填（不给直接报错） |
| `--enroll <setup-key\|sso\|none>` | 入网方式，默认 `none`；只给 key 不给 `--enroll` 时按 `setup-key` |
| `--version <x.y.z>` | 安装的包版本（默认脚本内嵌版本） |
| `--base-url <URL>` | 产物基址（发布产物中已由 CI 注入实际基址；源码内嵌值为开发默认） |
| `--service <NAME>` | 服务名，默认 `warp-ztna` |
| `--force` | （兼容保留：遇官方 NetBird 现在一律先替换，无需开关） |
| `--uninstall [--purge]` | 卸载；state 默认保留，`--purge` 连 state/日志/配置一起删 |
| `--dry-run` | 只打印将执行的命令，不改动系统 |
| `--help` | 帮助 |

环境变量：`WARP_ZTNA_SERVICE_NAME` / `WARP_ZTNA_STATE_DIR` / `WARP_ZTNA_LOG_FILE` /
`WARP_ZTNA_CONF_DIR` / `WARP_ZTNA_MANAGEMENT_URL` / `WARP_ZTNA_SETUP_KEY` / `WARP_ZTNA_BASE_URL` /
`WARP_ZTNA_SHA256` / `WARP_ZTNA_FORCE` / `WARP_ZTNA_SUDO`。

`WARP_ZTNA_SHA256` 覆盖默认的「对发布目录 `checksums.txt` 校验」；校验失败即终止，没有跳过选项。

### 入网

- 机器：`--setup-key[-file]` → `netbird up --setup-key-file <tmp> --management-url <URL>`，
  随后有界轮询（≤60s）`netbird status -C ready`，成功后打印 FQDN 与内部 IP，供在管理台
  「资产入网」里认人。
- 有人值守的机器：`--enroll sso` → `netbird login --no-browser`，打印设备码 URI 后阻塞等待。
- `--enroll none`：装完即止，原文打印上面两条官方命令，不新增任何 Warp 命令面。

setup key 由管理台「路由入网」页签发（完整值只显示一次），本包**只消费**。

## 共存与阻断

- 机器已有官方 NetBird（**任意版本**）：**先替换再安装**——停用并移除其服务、CLI 与 UI 应用
  （state 保留），不读版本、无开关；不产生「并存」状态。
- 其它通道同口径：`install.ps1`（Windows）、`install-app.sh` / `install-app-linux.sh`（终端一条命令）、
  macOS `.pkg`、Linux `.deb`/`.rpm`、Windows `.msi`——都不把「先卸官方」留给用户。
- 已有 `warp-ztna` 服务：按**升级**处理——停用并注销 → 换二进制 → 全量 flag 重装服务 → 启动；state 保留。

## 升级、卸载与退场

升级即重跑安装脚本；**无自动更新**。卸载保留 state（`--purge` 才删），且**不触碰 Management
里的 peer 记录**。机器退场顺序：① 先在管理台摘掉 Router 引用 → ② 再删 Management 里的 peer →
③ 最后 `--uninstall`；顺序错会让 Network 丢路由。

整机卸载（连同桌面 app 与用户侧残留）另有按 OS 的三份脚本：`deploy/client-pkg/scripts/` 下的
`uninstall-warp-macos.sh` / `uninstall-warp-linux.sh` / `uninstall-warp-windows.ps1`（公开镜像仓可见；
帮助站各 OS 安装页也有卸载说明）；默认同样保留 state，`--purge` 才删。

排障备注：netbird 把服务参数缓存在 `<state>/service.json` 且跨卸载/重装存活，重装会静默恢复旧参数；
本脚本每次显式传全量 flag，手工改动用 `netbird service reconfigure`。

## 平台差异

Linux 是路由节点主平台（内核转发，netbird 自动开 IP forwarding）；macOS 走 userspace 路由，
服务注册时自动追加 `NB_ENABLE_LOCAL_FORWARDING=true`，且 masquerade 不能关。
无 Windows 节点包（Windows 只有终端用户 app 包）。

## 发布与校验

- 构建与分发由 GitHub CI 执行：本仓的客户端面镜像为公开仓 `dayu-sec/warp-ztna-client`，
  发布流水线在那里运行（`.github/workflows/release-client.yml`）：拉取上游六个归档、
  对 `NETBIRD_SHA256` 校验、重打包为 `warp-ztna_<v>_<os>_<arch>.tar.gz`，产物以 Release 资产发布
  （取件基址 `https://github.com/dayu-sec/warp-ztna-client/releases/download`，路径 `client-v<版本>/<资产名>`；
  `checksums.txt` 覆盖全部资产）。
- `VERSION` 是本包版本（产物名与门户显示用它）；`NETBIRD_VERSION` 是引擎钉，CI 守齐它与
  `libs/warp-ztna-nbrpc` 的 `SUPPORTED_NETBIRD_MINOR`。
- 包内附官方 NetBird 归档里的许可文件（`LICENSE` 与 `LICENSES/` 目录）；再分发须保留其版权与免责声明。

开发入口（仓库根）：`just check-client-scripts`、`just build-client-archives`、`just build-client-macpkg`、
`just build-client-app-dmg`（后两条只在 macOS 上跑；权威产物由 CI 出）。
