# 发布流程

[返回首页](../README.md) · [测试范围](TESTING.md)

源码公开发布不自动代表某个二进制已验收。0.5.0 是测试版；未关闭的声音、真机和媒体异常范围必须随发布说明保留。

## 版本与构建

1. 更新 `Cargo.toml`、`Cargo.lock` 与 `apple/Info.plist` 的版本；Apple build number 单调增加。
2. 完成获准的回归并记录实际平台/SDK、资源版本及未测项，不在 CI 下载原作素材。
3. 用 `CONFIGURATION=Release scripts/build-apple.sh <sdk>` 构建 `macosx`、`iphonesimulator`、`iphoneos`。
4. 检查应用包不含 XP3、原作脚本、媒体、存档、私有检查点、签名密钥或账号凭据。
5. 只从已提交源码生成源码包。发布标签指向该提交，测试版标记为 prerelease。

## 签名与安装

| 产物 | 状态及用途 |
| --- | --- |
| macOS arm64 `.app` | 临时签名，仅用于本地试用；未经 Developer ID 公证，不应称为正式分发包 |
| iOS Simulator arm64 `.app` | 仅用于 Apple Silicon 模拟器，不可当作真机应用 |
| iPhoneOS arm64 `.app` | 默认未签名；需开发者在 Xcode 配置自己的 Team/签名后安装 |

不上传证书、Apple ID、签名 IPA 或 provisioning profile。CI artifact 是源码构建结果，不是 App Store/TestFlight 发行物，也不证明真机验收通过。

## 发布说明至少包含

- 对应提交、版本与 build number；源码及各平台包的校验值。
- 有权使用的资源应由用户自行准备，正确 overlay 顺序和导入空间要求。
- 已完成的无界面/真实资源/UI 检查，与未测的声音/真机/组合范围分别说明。
- 已知坏 Opus、缺失 SE 和基础版损坏 OP 的处理，不宣称修复了原作文件。
- 依赖许可证及上述临时签名/未公证限制。
