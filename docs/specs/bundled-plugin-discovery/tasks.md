# 发行包内插件自动发现 — 任务

**Spec**: `bundled-plugin-discovery`
**状态**: 本功能 T1–T3 已完成（2026-10-22）；全仓覆盖率等范围限制见实际证据
**授权**: 用户已批准需求，并于 2026-10-22 确认“你执行就行”，授权按设计补齐本清单后直接实施，不再逐阶段等待确认。该授权不包含提交、推送或发布。

## 执行约束

- 按顺序完成下列任务；每项验收通过后立即勾选并记录实际证据。
- 遵循 RED → GREEN → REFACTOR，先运行能揭示旧行为的失败测试，再实现自动发现。
- 新模块与真实调用方一并接入，不单独留下无人调用的模块。
- 文档和行为同一功能切片交付；不将 README 更新拖至所有验证结束。
- 若发现设计不成立，先更新 design.md，再继续实现，不静默改变需求。
- 不混入之前的安全审计修复，不改依赖、工具链、CLI 或已发布 Release。

## 任务清单

### T1 交付可直接使用的便携插件发现与管理兼容

- [x] T1 — 实现自动发现、回归测试及同步使用说明。

**对应需求**: R1–R6。

**文件**:

- `src/plugin.rs`
- `src/plugin/paths.rs`（新增）
- `src/plugin/install_command.rs`（新增）
- `tests/plugin_discovery.rs`（新增）
- `tests/common/plugin_fixture.rs`（新增）
- `README.md`、`README_ZH.md`、`AGENTS.md`、`CHANGELOG.md`
- `.github/workflows/release.yml`（仅后续发布正文及说明性注释）

**验收**:

1. 先取得有效 RED：旧 CLI 的真实运行期失败，或新组件测试引用缺失实现的编译期失败；保留实际证据。缺少依赖、下载超时和测试夹具故障不算产品 RED。
2. 纯选择器和命令构造单测通过；显式原值、包内目录、旧用户目录及 PATH 回退符合设计，安装参数仍指向同一有效根。
3. 隔离的原生子进程验证零配置、不同 cwd、经 PATH 启动、整体移动、显式覆盖、无目录/同名文件、HOME 不叠加、别名、JSON 与退出码。
4. dir/list/run 前后无核心附加写入；真实离线卸载只修改选中根；旧锁元数据和缺锁完整性语义保留。
5. 中英文说明、根目录事实、中文变更记录与后续发布模板同步；旧版示例不被描述成已经支持新功能。
6. 新增目标测试、`cargo fmt --check` 和 `cargo clippy --all-targets -- -D warnings` 通过。

**实际证据**:

- 测试先行：临时标准库 wrapper 引用真实 `paths.rs`，先取得 E0432/E0425 编译期 RED，再取得 16 项组件测试通过；`install_command.rs` 先取得 E0432 编译期 RED，再取得 4 项组件测试通过。初次完整构建在下载依赖时未执行旧 CLI 用例，不将其等待或后续结果记作旧版运行期 RED。
- 固定工具链实际为 `rustc 1.98.1 (48a229cea 2026-09-01)` / `cargo 1.98.1`；`cargo fmt --check`、`cargo clippy --all-targets -- -D warnings`、`cargo test` 全部通过。
- 完整 Cargo 验证：221 项二进制单元测试、31 项 CLI 测试、5 项启动冒烟、17 项插件发现 E2E 通过；另 1 项 ignored 专用插件 fixture 按设计只由子进程调用。合计 274 项通过、0 失败，真实父类型与模块接入已验证。
- 五份用户/仓库/发布说明同步；旧 tag 重建的发布模板也明确保留旧版手工配置要求。未新增依赖、参数、归档布局或版本号，未提交或发布。

### T2 验证真实只读目录及写操作失败边界

- [x] T2 — 在受控临时发行目录完成权限验收。

**对应需求**: R4、R5。

**文件**: 临时目录内的 `verify-bundled-plugin-readonly.ps1`（验收产物，不入仓库）、本任务记录；必要修正仅限 T1 所列文件。

**验收**:

- 仅对新建、隔离的临时插件树设置权限，不修改真实用户目录或系统 ACL，不提权。
- PowerShell 先写脚本，再以文件执行；先由写入/删除 canary 证明当前进程确实收到权限拒绝。
- 真实不可写时，dir/list/run 仍可使用已有插件，卸载明确失败，另一用户根副本保持不变。
- 不用目录 readonly 属性、找不到 cargo、模拟错误或未经确认的跳过冒充权限验收通过。
- 恢复临时目录权限并清理；无法可靠验证时如实记录未完成，不勾选为通过。

**实际证据**:

- 独立 PowerShell 5.1 `-File` 进程，仅对新临时 layout 的插件副本树设置当前 SID 的具体写/删拒绝位；进程 PATH 为隔离空目录，未提权或改系统/真实 HOME ACL。
- 写入 canary 与删除 canary 均真实得到拒绝：`createDenied=true`、`deleteDenied=true`。
- 核心目录查询和列举成功，原生插件实际执行并传播约定退出码 37；真实卸载返回 1，选中包内树与另一 HOME 副本内容均未改变。
- 最终结果：`permissionsRestored=true`、`layoutRemoved=true`。所有原始 DACL 已恢复，测试 layout 已清理。
- 初次临时脚本曾有 PS5 UTF-8 无 BOM 和数组包装问题，均作为验收夹具错误修正，未将这些失败当作产品 RED 或通过证据；生产代码未因此改动。

### T3 完成全部门禁、覆盖率测量和独立审查

- [x] T3 — 完成回归验证并收口审查意见。

**对应需求**: R1–R6。

**文件**: 本任务记录与规格状态；修复审查发现时仅修改本功能相关文件。

**验收**:

- `cargo fmt --check`、`cargo clippy --all-targets -- -D warnings`、`cargo test` 全部通过。
- 实际测量覆盖率；新增根选择逻辑按固定稳定工具链可测的行/region 指标至少 80%，同时如实报告整体数值及分支指标的 nightly 限制，不将新增文件数值冒充全项目覆盖率。
- code-reviewer、rust-reviewer、security-reviewer 完成独立审查，阻塞问题已修复并重跑相关检查。
- 手工核对 README 与发布模板不再把手工设置环境变量作为新发行包的必需步骤。
- 确认没有依赖、工具链、CLI schema 或归档结构漂移，没有提交、推送或发布。

**实际证据**:

- 固定 Rust 1.98.1 下，fmt、all-targets Clippy（`-D warnings`）及完整 cargo test 通过；全量覆盖率运行又执行同一批 274 项测试，仍为 0 失败、1 个专用 ignored fixture。
- 使用临时隔离的 cargo-llvm-cov 0.9.1 与固定编译器配套 LLVM 工具；下载内容分别通过 GitHub 官方资产摘要和已安装 Rust manifest 的 SHA-256 校验，未全局安装开发工具或改项目依赖。
- Windows、`src/` 文件统计口径的实际行覆盖率：`src/plugin/paths.rs` 为 97.02%（163/168），region 97.91%；`src/plugin/install_command.rs` 行/region 均为 100%。新增目录模块达到本功能 80% 目标。
- 当前全仓 `src/` 行覆盖率为 36.32%（4947/13619，44 个文件）；这不是新模块的数值，也没有声称整个仓库达到 80%。本次不扩展为无关的全库网络命令测试改造。稳定版未采集 nightly 分支指标。
- code-reviewer、rust-reviewer、security-reviewer 均完成实际变更的静态复核，无 CRITICAL/HIGH/MEDIUM 阻塞；权限脚本的进程 PATH 隔离缺口已补齐后才执行。
- 文档已明确新构建的零配置行为和旧版/旧 tag 重建的手工覆盖要求；依赖、工具链文件、CLI schema、归档布局均未变。未提交、推送或发布。
