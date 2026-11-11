# Koikari Reborn 0.3 — 开场章节预览

Rust 游戏核心 + Rust Metal 渲染，macOS / iPhone / iPad 共用代码。
这是独立重实现，不启动原 EXE、不加载原 DLL、不集成 Steam，也不修改原游戏。

**当前不是完整游戏移植。** 支持首个主剧情章节；全 TJS、路线选项、后续剧情、视频、动画、转场和原版完整 UI 尚未实现。遇到未知指令会停下并显示脚本行号，不会偷偷跳过。

## 资源

需要自行准备有权使用的 `data.xp3`，可同时放入 `patch.xp3`。补丁包优先覆盖主包。
资源不会随项目分发，源码内不包含原游戏的图片、音频、完整脚本或 Windows 程序。
不支持加密/受保护 XP3，不提供解密或 DRM 绕过。

- macOS：点击「导入 XP3 所在文件夹」，选择同时包含上述文件的目录；直接只读访问，不额外复制 2.9 GB。
- iPhone/iPad：在「文件」中准备资源目录，再用应用的同名按钮选择目录。应用复制 XP3 到自己的 Application Support，不导入 DLL、EXE 或原版存档。导入先检查文件头和空间，预留资源大小加 64 MB；显示字节进度并可取消。新副本验证成功前保留原资源；失败或取消清理临时副本，启动时清理上次中断的导入暂存目录。导入期间保持前台。
- iOS 的资源偏好只保存相对目录标识，升级安装改变沙盒根目录后仍可恢复；早期预览保存的旧容器绝对路径会迁移到当前 Application Support，不删除或重新复制原有资源与存档。
- macOS 开发启动也支持 `open PATH/KoikariReborn.app --args --assets /path/to/assets`。

## 构建

要求 Apple Silicon Mac、完整 Xcode（含 iOS SDK）、Rust 1.88+ / Cargo / rustup 和 CMake。
首次构建需要联网下载 Cargo 包和 Rust target。依赖版本固定在 Cargo.lock。

如果系统没有 CMake，可安装到项目私有虚拟环境，无需修改系统：

```sh
python3 -m venv .build-tools
.build-tools/bin/pip install cmake==3.31.10
```

```sh
scripts/build-apple.sh macosx
scripts/build-apple.sh iphonesimulator
scripts/build-apple.sh iphoneos
```

默认是 Debug，设置 `CONFIGURATION=Release` 构建 Release。应用位于：

```text
target/xcode-macosx/Build/Products/Debug/KoikariReborn.app
target/xcode-iphonesimulator/Build/Products/Debug-iphonesimulator/KoikariReborn.app
target/xcode-iphoneos/Build/Products/Debug-iphoneos/KoikariReborn.app
```

脚本只生成 **arm64**，不生成 Intel Mac 或 x86_64 模拟器版本。最低部署配置为 macOS 13 / iOS 16，低版本系统实际运行尚未验证。

也可打开 `apple/KoikariReborn.xcodeproj`，选择共享 scheme `KoikariReborn` 和目标设备。在 Xcode 的 Signing & Capabilities 中选择自己的 Team 后，才可安装到 iPhone/iPad 真机；脚本默认关闭签名，因此 iphoneos 构建结果**不能直接安装**。不附带开发者凭据、证书、描述文件或签名 IPA。

macOS 本地构建可使用临时签名运行：

```sh
codesign --force --deep --sign - target/xcode-macosx/Build/Products/Debug/KoikariReborn.app
open target/xcode-macosx/Build/Products/Debug/KoikariReborn.app
```

分发包未经 Developer ID 签名、公证或 App Store 审核；不要把本地构建成功当作分发或真机验收完成。

在已启动的 Apple Silicon iPhone / iPad 模拟器中安装模拟器包：

```sh
xcrun simctl install booted /path/to/KoikariReborn.app
xcrun simctl launch booted dev.greenhats.koikari-reborn
```

请自行确认素材使用与分发授权；本工程不赋予原游戏资源的再分发权。

## 操作与存档

- 点击画面、点击「下一句」或按空格推进；长对白可在面板内滚动。
- 「快进」约每秒推进 4 页，按钮再次点击即可停止；快进中点击画面或「下一句」只停止，不再额外换页。人物介绍图、预览边界、错误、弹窗或切到后台均会停止快进。快进期间不逐句播放语音/音效，也不每页写自动进度；停止后恢复当前页音频并尝试自动保存。
- 「上一句」返回上一完整画面；「回看」可选择最近 100 页中的任意页，确认后恢复背景、立绘、对白与音频标识。回溯后会丢弃该页之后的回溯分支，继续推进重新执行剧情；最近 300 页的文字仍可单独查看。读档后重建回溯窗口，不覆盖手动槽位。
- macOS 快捷键：空格 / 右方向键推进、左方向键上一句、`F` 切换快进、`Esc` 停止快进。快进中 `Esc` 优先停止并保留全屏，停止后再按沿用系统行为。弹窗中不继续推进或快进。
- 人物介绍使用 XP3 中的真实 `eyecatch` 图片，隐藏对白，点击继续后恢复指定背景和音乐；本轮修复了介绍图未显示、结束后仍停在上一场背景的问题。介绍以静态可停留画面呈现，不冒充原版动画和定时转场。
- 「存档 / 读档」打开 6 个独立手动槽位和一个只读的自动进度（磁盘槽位 9）；显示时间、页码和对白摘要。手动覆盖、读档及开始新游戏均有确认，旧版槽位 1 仍兼容。
- 新游戏、成功换页、读档及进入后台时尝试更新自动进度，不覆盖手动槽位。「继续游戏」恢复时间最新的有效手动或自动进度；损坏、资源不匹配、无效剧情/音频引用的存档显示不可用，不作为继续入口。读档验证失败不替换当前进度。
- 返回标题前尝试自动保存，失败则提示并保留当前画面；只有明确选择放弃时才离开。开始新游戏仅覆盖自动进度，手动槽位保留。自动进度写盘失败时显示警告，不能保证强制退出后仍能恢复最新页。
- 「设置」独立调整音乐 / 语音 / 音效音量和 80%–160% 对白字号，设置自动持久化，可恢复默认值。当前语音可重播；回看最近 300 页。长对白切到下一页时滚动位置重置。
- 存档为自定义 JSON，保存剧情位置、图层、角色状态、回看与音频标识；原版存档不兼容。读档从首章入口确定性重建到保存位置，同时修复旧预览存档的介绍图/后续背景；新增人物介绍停页会使旧页码略有变化。无法重建时保留当前状态，「继续游戏」尝试较旧的有效存档。读档重播当前语音，不恢复精确音频时间。
- 存档绑定 XP3 长度与索引的指纹，补丁变化后拒绝不匹配存档。这不是对整个 2.9 GB 文件的密码学完整性证明；每次读取资源另做 Adler-32 校验。
- 应用进入后台时尝试保存自动进度并暂停音频，回到前台后继续；不覆盖手动槽位，不支持原始音频采样位置的持久化。
- 「兼容状态」显示已知视觉近似和未支持指令；动画、震动、过场和等待目前用静态切换近似。

## 验证

```sh
cargo test --locked
cargo clippy --all-targets -- -D warnings
cargo fmt --check
xcrun swift-format lint --strict apple/*.swift tests/*.swift
xcrun swiftc apple/AssetLocation.swift tests/asset_location.swift -o target/asset-location-tests
target/asset-location-tests
xcrun swiftc apple/AssetImport.swift tests/asset_import.swift -o target/asset-import-tests
target/asset-import-tests

# 不包含原版资源的合成 XP3 测试始终运行；真实资源测试须显式启用。
KOIKARI_ASSETS=/path/to/assets cargo test --test real_assets -- --ignored --nocapture
cargo run -- walk /path/to/assets
```

真实资源测试涵盖首章到预览边界、存档确定性恢复、Opus/Vorbis 解码、Metal shader 编译和当前资源包的异常视频拒绝。`damaged_video_is_rejected_not_silently_used` 是针对本次审计文件的回归断言，若另一个合法资源版本已修复该视频，应只运行其他测试，而不是把正确视频当作损坏。

CLI 还提供 `list`、`text`、`verify`。完整性失败时保留错误，不对原始资源做补丁或忽略校验。

当前 `bgimage/OPmovie.mp4` 的一个 zlib segment 校验失败：尚未确认是源包、下载还是特殊封装问题，视频路径未接入。第三方 `metal 0.29` 依赖的 `block 0.1.6` 还会产生 Rust 的 future-incompatibility 警告；本项目的 Clippy 检查与该依赖的未来兼容性是两回事。

无界面测试、GPU shader 编译和三平台构建不能证明应用 UI、实际扬声器输出或真机生命周期正常；需要另外做 macOS、iPhone 模拟器、iPad 模拟器和真机实测。

### 本次验证结果

- v0.3 合成资源/单元回归：30 项通过，覆盖人物介绍、旧视觉存档重建、回溯边界及继续推进一致性。真实 XP3 / GPU 回归：8 项；另有 16 项 Swift 路径断言及 Swift 导入的复制、进度、取消、坏补丁、目标保护、暂存清理和空间检查回归。
- 首章脚本运行到显式预览边界：690 次停页，未遇到未支持命令；包含 3 张人物介绍，最后一次停页为章节边界，不等同于 690 句对白。
- 464 个不同图片成功解码；最大画面活动纹理像素约 150.4 MiB（不是应用总内存）。
- 实际 Opus 语音、Vorbis 音乐样本成功解码，Metal shader 成功编译，存档恢复后下一页状态一致。
- macOS arm64、iOS Simulator arm64、iOS Device arm64 的 Release 应用构建均通过；设备版本未签名，未安装到真机。
- Rust Clippy `-D warnings`、Rust fmt、Swift format 严格检查、Shell 语法和 plist 检查通过。第三方 `block` 的 future-incompatibility 提醒仍存在。
- v0.3/build 5 实际界面复验：macOS 原生全屏工具栏可见、可点击；全屏 `F` → `Esc` 停止且无页码漂移，再按 `Esc` 正常退出全屏，窗口模式也能停止。修复了 Metal 视图全屏时遮挡工具栏，以及系统先处理 `Esc` 导致快进未停的问题。
- macOS、iPhone 和 iPad 模拟器实际显示第一张 `EYE_C_1` 及指定第 115 页梦乃对白，背景为教室而非旧卧室；上一句、回溯第 112 页人物介绍、继续重现第 113 页画面通过。iPhone 录像还验证第 199 页 `EYE_C_3`、第 247 页 `EYE_C_4` 及后续 `CI_1` 画面；不是三平台逐一遍历所有介绍图。
- v0.3 快进会在三张人物介绍和第 690 页预览边界停止。「下一句」停止快进前后页码相同；画面点击已观察到停止，但未单独确认其精确无额外推进时序。macOS 设置、回看和存档弹窗中 `F` / 空格 / 右方向键不能穿透推进。
- v0.3 验证旧槽位恢复及重建回溯、手动槽位 4 / 5 的覆盖与读取取消/确认、停止后的自动进度及重启继续、新游戏取消及仅覆盖自动进度、受控损坏槽位回退、设置持久化及恢复默认。保留原手动槽位，受控损坏使用测试副本并还原。
- v0.3 iPhone 首次导入从无资源页经真实 Files 选择器完成，并观察到字节进度；iPad 更换导入的进度、取消、无效目录后重试及成功替换通过，原槽位和相对资源标识保留。使用隔离的合成长文资源验证 iPhone 160% / iPad 100% 字号滚动到末尾不换页、下一页从顶部开始；这不是原版脚本覆盖。
- 当前 UI 验收不是全功能组合矩阵：错误/blocked 自动停止、各平台全部存档/自动进度转移、重复 `F` 和按钮停止的全平台组合、macOS 设置持久化与尺寸矩阵、语音重播 UI 尚未完整验证。原 macOS 手动槽 1/2/3、iPhone/iPad 原槽 1 与测试前备份哈希一致；新增测试槽与损坏样本已移出实际存档目录。
- 早期版本的 macOS、iPhone 17 / iOS 26.5 模拟器及 iPad 模拟器已完成真实界面操作验收：首次资源导入、错误恢复、首章前 31 页、CG / 单头像、存读档、重启继续、标题警告及文字回看。修正了切场残留立绘、头像标志优先级、iOS 警告可见性及升级后绝对路径失效。这些旧录屏不能替代 v0.3 快进、状态回溯和人物介绍的验收。
- iPhone 升级后无需重导入即可恢复原存档；iPad 的取消 / 无效资源更换不破坏原素材，成功更换后新素材及存档仍可继续。macOS 的宽矮 / 窄高窗口、iPhone 安全区及 iPad 留黑已观察验证；不是所有设备尺寸或低版本系统矩阵覆盖。
- 使用独立、自有的 3,512 字符合成 XP3，在 macOS、iPhone / iPad 模拟器中验证超长对白滚至末尾且不误推进，下一句才进入第二页；未修改原 XP3，也未覆盖真实手动存档。此项是布局专项，不冒充原版剧情兼容验证。
- 测试 VM 无音频输出设备：BGM / 语音 / SE 的实际声音、静音效果、音频竞争及音频生命周期均未验证；iPhone / iPad 真机未测试。未人为破坏真实资源验证未知指令停止界面（仅有合成脚本单测覆盖）。不宣称完整游戏可玩。

### 界面复验准备

- 使用专用 iPhone / iPad 模拟器保留测试数据，不清空其他应用或用户设备；首次导入验收必须从无资源页面经真实 Files 选择器完成，不能以 `--assets` 跳过。
- 在模拟器 Files 可访问目录准备用户有权使用的 XP3；同一 APFS 卷可用克隆减少测试源副本的磁盘开销，但仍须实际测试应用内复制和导入。资源始终位于源码之外，不加入交付包。
- 合成布局专项使用独立模拟器；macOS 仅设置临时 `HOME` 不保证隔离 `UserDefaults`，应记录并恢复原资源偏好，前后核对手动存档未变。
- 开始操作前启动录屏并标注断言；实际音频只能在具备输出及采集能力的环境单独验收，按钮标签切换、解码成功或无错误弹窗不算声音通过。

## 结构

- `src/xp3.rs`：受限且校验完整性的 XP3 读取，补丁覆盖，安全路径与歧义检测。
- `src/kag.rs` / `story.rs`：Unicode KAG tokenizer、首章指令适配和显式兼容边界；仅提取静态配置，不执行原始 TJS。
- `src/renderer.rs` / `scene.metal`：原生尺寸立绘部件合成、纹理缓存和 16:9 留黑绘制。
- `src/media.rs`：Opus/Vorbis 转 PCM WAV，交给 AVAudioPlayer 播放；不是 MP4 播放器。
- `src/model.rs` / `save.rs`：状态、输入坐标映射、版本化原子写入与存档恢复。
- `src/ffi.rs` / `apple/`：主线程 C ABI、SwiftUI + MTKView 外壳和 Apple 生命周期。

后续仍须补齐实际音频及真机覆盖，并扩充全剧情控制流、选项与变量解释器、动画/视频和旧系统覆盖，不能直接删掉章节边界宣称全剧情可用。
