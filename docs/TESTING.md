# 测试与覆盖边界

[返回首页](../README.md) · [贡献指南](../CONTRIBUTING.md)

以下是 **0.5.0 / build 9** 的分层记录，不是“所有功能全部验收通过”的声明。首次开源后，新增测试暂按维护者安排手动启动。

### 后续增量

- build 10：Rust stable / 1.88 的默认 137 项、格式与 Clippy 均通过；原生媒体门控扩展为 219 条策略断言。真实 OP 在 macOS 切换到其他应用后保持停帧，重新聚焦继续的有界 UI 回归通过。iPhone 模拟器触控停止后稳定、Home 往返与手动读档通过；其余媒体矩阵未完成。
- build 11 候选：用户在全能签重签名的 iPad 上报告目录选择器「打开」无响应，不能以此前模拟器导入通过排除该问题。按用户要求改为 Files 可见的文稿 `GameResources` 目录、无选择器直接加载和缺失提示。相对目录迁移、重复初始化不覆盖资源/存档/说明的原生测试及三 SDK 类型检查已通过；新的 Files UI 与真机流程尚待复验。
- build 12 候选：补回原作 `[c]` 宏在等待输入之后执行的 `motion_stop lastpos=true`，不改动合法无限循环、原始 `[p]` / `[l]` 等待或存档格式。增加翻页、原始等待、存读档和回溯的回归；真实合并资源第 217／345 页不再继承旧对白的抖动。Metal 在原始图像坐标中缓存合成立绘底图、表情、红晕和附加贴片，再进行子像素移动、缩放和整体透明度处理；使用预乘透明纹理的正确采样，保留线性过滤。4 项显式 GPU 回归通过，包含贴片接缝、半透明颜色、缓存复用和失败后重试；三目标 Release 构建通过。人物画面与 Files 流程的新版 UI、真机结果仍待复验。

## 可公开复现的检查

不需要原作资源：

```sh
cargo fmt --check
cargo test --locked --all-targets
cargo clippy --locked --all-targets -- -D warnings
# 需要完整 Xcode：Swift 6 严格检查、三 SDK shader、原生策略/导入测试
scripts/check-native.sh
```

首次构建需 CMake 3.x，推荐首页的隔离安装方式。CI 在 PR 上自动运行，也支持手动触发；不会下载游戏资源或把私有资源放进缓存/构建物。CI 绿灯只能证明其实际执行的检查。

## 已记录结果

| 层级 | 已完成 | 不能据此推断 |
| --- | --- | --- |
| Rust 回归 | macOS 默认 137 项、显式 opt-in 23 项 | 所有平台数量相同、所有资源变体兼容 |
| 原生策略 | 19 条集成断言、132 条媒体策略断言、路径与导入测试 | 真实声音、系统硬件中断已经实测 |
| 编译与构建 | Swift 6 严格检查、三 SDK Metal、三目标 Release、macOS/模拟器临时签名验证 | 真机安装、最低系统实测、Developer ID 公证 |
| 真实 XP3 | 19,513 个生效条目的严格完整性检查 | 归档内媒体内容本身绝无损坏 |
| 有限目标遍历 | 124 个主脚本、13 处选择、31 个分支、7 个结局、150 个回放、84 个图库 ID | 所有输入组合、所有条件和累积进度变体已穷举 |
| 原生 UI | macOS、iPhone/iPad 模拟器：真实资源导入、指定缺图页面、继续、存读档、快进/停止、回溯；macOS 合法选项、OP、片尾/结局和收藏流程 | 每条路线自然从头通关、完整设备/错误矩阵 |

选择/视频/计时器/结局的长程 UI 检查使用实际 Engine 合法遍历生成的 checkpoint，不是人工构造变量、全解锁或直接跳章，也不是从头自然 UI 通关。iPad 真实 Files 导入及中途取消保护旧资源/存档已检查；iPhone 相对路径、重启继续、手动与自动槽分离已检查。测试结束恢复了三端原用户存档和偏好。

## 仍未关闭的范围

- VM 没有音频输出设备：**实际声音、正增益取消静音后的当前音轨重建与听感未验证**。
- iPhone/iPad 真机、耳机拔插、硬件路由丢失和真实系统音频中断未验证。
- 关闭输出恢复弹窗后的暂停横幅、损坏 Opus/缺失 SE 的原生逐片 Skip/Retry、禁用选项分支未完成 UI 验证。
- 视频实际验证了渲染、设置弹窗暂停/恢复及 EOF；后台、失败/Retry/返回标题、过期回调的完整 UI 矩阵未完成。
- 已证明停止快进后页码稳定，**没有证明按下停止瞬间精确零额外页**。
- 全输入组合、反复新游戏后的进度分支、所有条件路径、最低支持系统和所有机型峰值内存均未穷举。

## 真实资源检查（必须自备）

资源只存放在仓库之外，运行前先备份用户存档；不要在公开 issue/CI 上传资源、存档或 QA checkpoint。

```sh
KOIKARI_ASSETS=/path/to/base KOIKARI_PATCH_ASSETS=/path/to/combined \
  cargo test --locked --release --test real_patch -- \
  --ignored --nocapture --test-threads=1

cargo run --locked --release --bin koikari -- \
  verify-targets /path/to/combined 5000000 --max-states 2000000
```

`verify-targets` 从首次到达各入口/选项的合法状态分叉，回放由自然终局进度调度。已有结果为 `targets_complete=true`、`complete=false`，零 direct probe、零未遍历目标或 blocker。`verify-all` 用于有预算的组合探索，预算耗尽必须报告失败，不能改写成完成。

## 已知资源缺陷

- `voice/vo5/vo5_1068.opus`：XP3 Adler-32 通过，Ogg 页内部 CRC 不符，严格拒绝。
- `scenario/main/1_4.ks`：引用不存在的 `se` 音效；保留明确提示后按原指令语义继续。
- 基础包 `bgimage/OPmovie.mp4` 损坏；补丁覆盖版本可通过严格导出并实际播放。

不能关闭校验、伪造静音、修改资源或自动无提示循环重试来消除这些结果。
