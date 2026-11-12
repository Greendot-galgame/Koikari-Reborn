# 架构

[返回首页](../README.md)

这是针对《租借男友》实际脚本集合实现的独立运行时，不执行原 Windows 程序，不是完整 TJS2 虚拟机或任意 KiriKiri 插件兼容层。

```mermaid
flowchart LR
    XP3[用户自己的 XP3] -->|只读 / 校验 / 覆盖| Store[资源 Store]
    Store --> Engine[Rust Engine]
    Engine --> State[完整执行状态与进度]
    State <--> Save[指纹隔离的存档与回溯]
    Engine --> Scene[视觉时间线]
    Scene --> Metal[Metal 合成]
    Engine --> FFI[C FFI / JSON 快照]
    FFI <--> UI[SwiftUI 原生界面]
    Store --> Media[严格媒体解码与导出]
    Media --> AV[AVFoundation]
    UI --> Gate[生命周期 / 设备 / 弹窗门控]
    Gate --> AV
```

## 模块地图

| 路径 | 负责的边界 |
| --- | --- |
| `src/xp3.rs` | 索引、segment、解压、校验、安全路径、数字覆盖与资源指纹 |
| `src/kag.rs`, `events.rs`, `tjs.rs` | KAG 指令、事件数据库及受限表达式/变量求值 |
| `src/story.rs` | 解释器、章节/调用栈、条件、选项、计时器、路线、结局和回放 |
| `src/model.rs`, `save.rs` | 执行状态、进度、版本化存档、原子写入与恢复校验 |
| `src/animation.rs`, `renderer.rs`, `scene.metal` | 可保存的视觉时间线、裁剪、图层及 Metal 绘制 |
| `src/media.rs` | Opus/Vorbis 解码、视频校验、受控导出与缓存 |
| `src/coverage.rs` | 使用实际 Engine API 的有限预算路线遍历 |
| `src/ffi.rs`, `apple/Koikari.h` | Rust/native 所有权和调用边界 |
| `apple/KoikariApp.swift` | UI、输入、快进、原生播放器与统一恢复策略 |
| `apple/GameVideo.swift`, `CreditsView.swift` | 原生视频生命周期及片尾显示 |
| `apple/AssetImport.swift`, `AssetLocation.swift` | 可取消导入、相对资源标识及旧路径迁移 |

## 必须保持的不变量

1. **资源只读且严格校验。** 后包覆盖同路径条目；不读取原始程序，不解密受保护资源，不关闭 Adler-32/Ogg 校验。
2. **剧情状态是唯一事实来源。** 画面、选项、计时器与播放器由快照驱动；不能用 UI 跳章或假解锁代替执行。
3. **恢复是事务。** 先验证存档、资源指纹和回溯序列，成功后才替换活动 Engine；失败不破坏现有状态或源存档。
4. **页号不等于提交状态。** 当前页面仍在构建时，可与上一已提交 checkpoint 共用页号；不能因此允许未来、重复、无序或未提交 checkpoint。
5. **媒体失败有明确语义。** 资源损坏按片段 Skip/Retry；输出设备失败按全局恢复处理。静音/零音量不启动播放器，不伪造解码成功。
6. **回调绑定身份。** 过期音视频回调不得影响新的片段，播放失败不能当作 EOF。
7. **进度来自真实执行。** 回溯丢弃未来分支，但不会撤销已自然获得的 CG/回想；新游戏和标题不清空同指纹全局进度。

## 平台与限制

Rust 核心可以在无界面环境中验证合成脚本；Metal 和原生壳使用 Apple SDK。交付构建为 arm64 macOS、iOS Simulator 与 iPhoneOS。系统最低部署版本不等于该版本已实测。

宽图集只在存在明确帧几何时裁剪，避免创建超过设备限制的整张 GPU texture。原生音视频的精确播放偏移不写入存档。完整的设备内存、声音与中断验收参见[测试范围](TESTING.md)。
