<p align="center"><img src=".github/assets/banner.svg" alt="Koikari Reborn — Rust × Metal, macOS · iPhone · iPad" width="100%"></p>

# Koikari Reborn

[![License: MIT](https://img.shields.io/badge/license-MIT-477b68)](LICENSE)
![Status: beta](https://img.shields.io/badge/status-0.5.0_beta-c49d4c)
![Platforms](https://img.shields.io/badge/platforms-macOS_·_iPhone_·_iPad-456b86)

**用自己的游戏资源，在 Apple 设备上运行独立的 Rust + Metal 视觉小说运行时。**

An independent Rust + Metal runtime for macOS, iPhone and iPad. Source code is MIT licensed; original game assets are not included or licensed by this project.

[资源导入](#资源与导入) · [操作与存档](#游玩与存档) · [构建](#构建) · [架构](docs/ARCHITECTURE.md) · [验证范围](docs/TESTING.md) · [参与贡献](CONTRIBUTING.md)

Rust 游戏核心 + Metal 渲染，macOS / iPhone / iPad 共用代码。
独立读取用户自己的 XP3，不启动原 EXE、不加载原 DLL、不集成 Steam，不修改资源包。

已接入完整事件数据库、章节衔接、选择与路线、七个结局、变量与受限 TJS、动画转场、音视频、片尾字幕、完整状态存档、CG 与回想。不是删除首章边界的预览版。
**仍为测试版**：目标路线验证不等于所有输入组合穷举，原生界面、声音和真机的验收范围需分别说明。不是通用、任意插件兼容的 KiriKiri/TJS 引擎；未知必需操作明确报错，不偷偷跳过。

> **源码开源 ≠ 原作资源开源。** 仅本项目的新实现代码与文档按 [MIT](LICENSE) 授权；游戏脚本、图片、音视频、商标及第三方依赖保留各自权利。请自行合法取得资源。详见[资源与授权边界](docs/RESOURCE_POLICY.md)。本项目非原作官方移植。

| 层 | 职责 |
| --- | --- |
| Rust | XP3 校验与覆盖、剧情解释、变量与路线、媒体解码、存档和进度 |
| Metal | 分层合成、动画、镜头、转场和有界图集裁剪 |
| SwiftUI / AVFoundation | 原生界面、文件导入、键鼠/触控、音视频和生命周期 |

## 资源与导入

需自行准备有权使用的 `data.xp3`，可同目录放入 `adult.xp3`、`patch.xp3`、`patch1.xp3` 至 `patch50.xp3`。覆盖顺序：

```text
data → adult → patch → patch1 → … → patch50
```

数字按数值排序；后者覆盖同路径条目。源码和应用不包含 XP3、原版脚本、图片、音频、存档或 Windows 程序。不支持受保护条目，不提供解密或 DRM 绕过。每次提取检查边界、解压和 Adler-32。

- **macOS**：选择含资源的文件夹，直接只读访问，不复制资源。开发启动也可用 `open /path/KoikariReborn.app --args --assets /path/to/assets`。
- **iPhone/iPad（build 11 起）**：首次启动后，打开「文件 → 我的 iPhone/iPad → 租借男友 Reborn（或签名时指定的应用名）→ GameResources」，手动将 `data.xp3`、可选的 `adult.xp3` / `patch*.xp3` 放入其中。完成移动后回到游戏会自动检查，也可点击「重新检查资源」。没有资源时显示放置位置和缺失提示，不调用目录选择器。
- iOS 直接读取该文稿目录，不再额外复制一整份资源；游戏运行时不要移动、删除或改写 XP3。旧版已导入的 Application Support 资源和存档仍兼容，不会自动删除。
- iOS 保存相对资源标识，升级后沙盒路径变化不要求重新导入；兼容迁移早期绝对路径。
- 「更多 → 兼容状态」可检查实际归档和覆盖顺序。加入/移除归档会改变指纹；不匹配旧档仍保留，切回相同资源版本后再读取。

### KG012Z4A0X0X-VER100

将 ZIP 内 `adult.xp3` 放入主资源同目录，无需运行 `Setup.exe`。补丁中的空 `adult.tjs` 是合法标记；只接受结构、空 segment 表和校验均合法的空条目，不以此放宽受保护/损坏数据检查。

当前授权资源有两项明确缺陷：
- `voice/vo5/vo5_1068.opus`：XP3 校验通过，但一个 Ogg 页内部 CRC 不符；严格解码拒绝。
- `scenario/main/1_4.ks` 引用名为 `se` 的不存在音效。

界面明确提示失败片段，可由用户跳过此条或重试；不会自动循环重试、生成假静音、重写校验或伪造播放成功。原基础包的 `bgimage/OPmovie.mp4` 损坏，仍严格拒绝；补丁覆盖的版本可严格导出并交 AVFoundation 播放。

## 游玩与存档

- 点击画面、下一句、空格/右方向键推进；长对白可滚动。真实选项须显式选择，禁用项不能提交。
- 快进约每秒四页，可随时停止；再次点击快进、画面或下一句只停止，不额外推进。选项、视频、真正结局、错误、弹窗、后台会停止。仅可跳过脚本明确允许跳过的等待，后台无限动画不阻塞对白。
- macOS：`F` 切换快进，左方向键上一句，`Esc` 优先停止快进并保留全屏，再按沿用系统退出行为。
- 上一句与回看中的回溯恢复画面和完整执行状态，不只是查看文字。保留最近 100 页回溯及 300 页文字；回溯后丢弃未来分支，但不收回已经真正获得的全局解锁。
- 六个手动槽位和独立自动进度（槽 9），支持覆盖/读取确认。继续游戏寻找最新有效进度；损坏、不匹配或验证失败的档不替换当前状态。
- v2 存档保存完整解释器位置、调用/条件/宏帧、选项、变量、计时器、视觉时间线、音视频标识及进度；原子写入并绑定资源指纹。旧预览档仅在迁移可验证时转换，否则明确拒绝并保留文件。原版游戏存档不兼容。
- 自动保存失败会警告，不能保证强制退出后恢复最新页；不覆盖手动槽。新游戏、标题与回放保留同资源指纹下真正获得的进度。CG/回想只通过实际事件解锁，没有一键全开或章节跳转替代实现。
- 设置独立调整音乐/语音/音效音量、对白字号，可重播当前语音；精确原生音频/视频播放偏移不持久化。
- 全部静音或单声道音量为零时，不启动对应解码/播放器；取消静音从当前片段重建，不推进剧情。音频输出无法启动时显示一次恢复选择，可「静音并继续」或显式重试，不把设备故障记成每条资源损坏。静音不等于实际声音验收通过；重新请求播放时仍做严格资源校验。
- 音视频在后台、弹窗、错误和系统中断时受统一策略控制。OS 不允许自动恢复或耳机断开后，须显式同意恢复。视频失败不是完成，过期回调不能推进新片段；不可跳视频没有手动跳过入口。
- Metal 支持场景分层、样式、移动、镜头、震动、白场与转场；片尾和消息标记有独立原生显示。宽动画图集仅上传有明确几何的裁剪帧，不按图集宽度猜帧数。

## 构建

需要 Apple Silicon Mac、完整 Xcode（iOS SDK）、Rust 1.88+、Cargo/rustup 和 CMake；依赖固定在 Cargo.lock。仅构建 arm64，最低部署配置 macOS 13 / iOS 16。低系统版本不因构建成功而视为实测通过。

```sh
git clone https://github.com/Greendot-galgame/Koikari-Reborn.git
cd Koikari-Reborn
# Opus 原生构建使用 CMake 3.x，避免 CMake 4 与旧上游配置不兼容。
python3 -m venv .build-tools
.build-tools/bin/pip install cmake==3.31.10
export PATH="$PWD/.build-tools/bin:$HOME/.cargo/bin:$PATH"
```

```sh
CONFIGURATION=Release scripts/build-apple.sh macosx
CONFIGURATION=Release scripts/build-apple.sh iphonesimulator
CONFIGURATION=Release scripts/build-apple.sh iphoneos
```

不设置 CONFIGURATION 时为 Debug。产物分别位于 `target/xcode-<sdk>/Build/Products/Release[-iphonesimulator|-iphoneos]/KoikariReborn.app`。
也可打开 Xcode 工程，选择 KoikariReborn scheme。

macOS/模拟器构建会刷新临时签名并验签；设备构建未签名，不能直接安装。真机需自行在 Xcode 选择 Team 并配置签名。不附带凭据、证书或签名 IPA；未经 Developer ID 公证、App Store 审核或正式分发验收。

## 验证与边界

```sh
cargo fmt --check
cargo test --locked --all-targets
cargo clippy --locked --all-targets -- -D warnings
xcrun swift-format lint --strict apple/*.swift tests/*.swift

# 真实资源必须显式启用；不复制进源码，也不改写原包。
KOIKARI_ASSETS=/path/to/original KOIKARI_PATCH_ASSETS=/path/to/combined \
  cargo test --locked --release --test real_patch -- --ignored --nocapture --test-threads=1
cargo run --locked --release --bin koikari -- \
  verify-targets /path/to/combined 5000000 --max-states 2000000
```

build 9 在 macOS 上的 137 项默认 Rust 回归、23 项 opt-in 测试已通过；不同平台的条件编译测试数量可能不同，默认绿灯不能代替真实资源/GPU 检查。集成候选的真实 Engine/XP3 目标遍历已执行 124 个主脚本、13 处选择的 31 个分支、七个终局、150 个自然解锁回放，记录 84 个获得的图库 ID，无直接章节探针或强制标志。完整性审计通过 19,513 个生效条目。

`verify-targets` 在每个入口/选择首次观察到的合法状态分叉各选项，已覆盖选择使用规范输入；回放从自然终局进度调度。它报告 `targets_complete=true`、`complete=false`：不穷举所有选择组合、反复新游戏后的累积进度分支或每个条件路径。完整组合验证仍使用有预算的 `verify-all`，耗尽预算会明确失败，不冒充通过。

CLI 还提供 `list`、`text`、`verify`、`walk` 等资源与执行诊断。不将静态清单、合成脚本或直接媒体探测当作自然剧情通关。
全部图像及绝大多数音频已分别解码；上面的单条 Opus 损坏仍保留。纹理/视频内存测试不代表真实 iPhone/iPad 峰值内存安全证明。

Rust 的继承依赖 `block 0.1.6` 有 future-incompatibility 警告，不等于本项目 Clippy 失败。无界面测试、三平台构建和 shader 编译不能证明 UI、扬声器输出、真机中断与设备丢失行为通过；记录具体验收设备与未测范围。

macOS、iPhone/iPad 模拟器已完成 build 9 的有界真实资源 UI 检查。长程选择、视频、结局和计时器部分使用真实 Engine 合法遍历产生的检查点，不代表从头自然 UI 通关。**实际声音、真机、全部输入组合、完整媒体异常矩阵、停止快进瞬间精确零额外页仍未验证。** 详见[分层测试记录](docs/TESTING.md)。

[GitHub Checks](.github/workflows/checks.yml) 提供无原作资源的 Rust 检查与三 SDK 构建；PR 自动检查，也可手动运行。真实 XP3、存档和私有验收资料不会上传 CI。初次发布遵照先开源、后测试的安排，不自动触发新一轮测试。

## 原生验收注意

使用专用模拟器，保留原数据；操作前备份并核对手动存档。首次导入需从无资源页经真实 Files 选择器完成，不用启动参数冒充导入。资源源副本可用 APFS 克隆节省空间，应用内导入仍需真实操作。合成长文布局测试与真实资源路线验证必须区分。
未配备音频输出/采集设备时，按钮变化、成功解码或没有报错不等于听感验收。

## 结构

- `xp3.rs`：严格归档、安全路径、覆盖与指纹。
- `kag.rs / events.rs / tjs.rs / story.rs`：数据库控制流、受限表达式、执行状态与真正的路线终局。
- `animation.rs / renderer.rs / scene.metal`：可保存视觉时间线、裁剪与 Metal 合成。
- `media.rs / GameVideo.swift`：严格音频解码、视频校验/缓存和 AVFoundation 播放。
- `model.rs / save.rs`：完整状态、进度隔离、版本化持久化和回溯。
- `ffi.rs / apple/`：原生交互、输入/生命周期门控、字幕、CG 与回放。

## 参与与发布

- [贡献指南](CONTRIBUTING.md)：开发约定、回归要求与资源提交限制。
- [发布流程](docs/RELEASING.md)：版本、签名与不含资源的包。
- [问题反馈](https://github.com/Greendot-galgame/Koikari-Reborn/issues/new/choose)：附版本、平台和脱敏复现步骤，不上传原作资源或存档。
- [安全报告](SECURITY.md)：归档、路径、解码与存档漏洞请私下报告。
