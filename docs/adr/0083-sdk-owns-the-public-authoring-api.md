---
status: accepted
---

# 独立 SDK 拥有公共作者 API，CLI 安装固定 SDK 版本

为使 Workflow 声明和数据工具可以脱离 CLI 安装、测试及复用，公共 `kat` API 迁移到独立的 [kat-sdk 仓库](https://github.com/qiqingzhixin/kat-sdk)，由 `kat-sdk` distribution 提供。CLI 仓库的 `kat-workflow` wheel 只包含私有 `_kat_runtime`，单向依赖经过验证的 SDK 精确版本；SDK 不携带 Runtime，不反向依赖 CLI。保留现有 `from kat import workflow` 等 import，Context 的执行能力仍由 Runtime 绑定，`kat.pack` 仍只由 Runtime 动态挂载。

SDK 在版本 tag 的双平台测试通过后向 GitHub Release 发布 wheel 和 SHA256SUMS。CLI 锁定版本、下载 URL 和 SHA-256，只下载已发布的标准 wheel，Payload Builder 校验 wheel 身份与 SHA-256 后用 pip 安装到 Bundled Python Host。SDK 独立版本化；CLI 更新 SDK 依赖时重新验证组合，不要求 SDK 版本与 CLI 相等。SDK 来源锁、Runtime 依赖和第三方依赖锁必须一致。首次交付使用 GitHub Release wheel，不依赖 PyPI 同名发布；CLI 不再拉取或构建 SDK 源码。

本决定替代 ADR-0045，以及 ADR-0004 中关于 API 与 Runtime 共用私有 wheel、SDK 不可独立安装的限制；Bundled Host、isolated mode 和运行期离线约束继续有效。原生 `kat-datasource` 保持独立，使用原生能力的 Provider 仍需它或外部工具；本次不扩大原生构建、Python 支持范围或引入兼容适配层。最小切片及验证见 [Issue #280](https://github.com/maokelong/kat-cli/issues/280)。
