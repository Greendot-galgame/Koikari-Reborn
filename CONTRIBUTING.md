# 参与贡献

感谢帮助完善 Koikari Reborn。请先阅读[架构](docs/ARCHITECTURE.md)、[资源边界](docs/RESOURCE_POLICY.md)和[测试范围](docs/TESTING.md)。

## 开发流程

1. 从最新 `main` 建立聚焦单一问题的分支。
2. 沿用现有 Rust / Swift / Metal 接口，尽量用最小修改解决根因。
3. 在代码中构造不含原作内容的最小回归；不要提交提取脚本、素材、存档或私有 QA checkpoint。
4. 执行 `cargo fmt --check`、`cargo test --locked --all-targets`、`cargo clippy --locked --all-targets -- -D warnings`；Apple 改动另执行 `scripts/check-native.sh` 和对应 Release 构建。
5. PR 描述写明行为、验证命令及没测的范围。界面/媒体改动的编译成功不能代替实际 UI、听感或设备验证。

原作资源必须自行合法准备，放在仓库之外。不要把它们放进 Actions、缓存、artifact 或公开 issue。账户凭据也不属于测试夹具。

## 兼容性约定

- 不执行原 EXE/DLL，不加载 Steam，不实现 DRM 绕过。
- 保留归档、解码和存档安全校验；未知必需指令明确失败，不静默忽略。
- 存档恢复失败不替换活动状态，不改写用户原档；更改格式须有版本及迁移/拒绝测试。
- 音频设备失败与片段损坏分开处理，静音/零音量不得启动播放器；视频失败不能当作完成。
- CG/回想来源于合法剧情进度，不用人工全解锁或直接跳章证明覆盖。
- 不把目标遍历说成所有输入组合穷举；新未测项必须明确记录。

## 授权

贡献者须有权提交所提供的代码和文档。提交的本项目代码按 [MIT](LICENSE) 许可证提供，第三方来源请保留适用声明；原作资源不因此得到再分发授权。
