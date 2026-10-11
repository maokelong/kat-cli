# 独立知识创作

仅当用户授权固化或修改经验时写入指定 PACK。先确定适用对象、触发场景、需要的输入、期望结果与限制；范围不限于报告，可以是评审准则、判断框架、诊断方法或模板。需要程序才能稳定完成的操作仍用 Workflow，不把已有程序步骤改成 AI 手工执行。

## 归属与声明

可以维护纯知识 PACK：最少只需要合法 pack.toml 和 `skills/<entry>/SKILL.md`，不要求 workflows、datasources 或 pytest。新 PACK 按作者流程用拒绝覆盖的骨架脚本创建，再添加必要知识；空程序目录不是交付能力。

SKILL.md 的 YAML 头只允许必填字符串 name、description，name 与入口目录一致，遵循 PACK 的小写 ASCII kebab-case 与 Windows 保留名规则。两者非空，正文也不能为空。不要添加模型、执行器、Workflow 绑定或依赖元数据。

正文描述如何应用知识及其适用边界，按需要连接入口旁的 references/ 或 PACK 共享 knowledge/。链接相对当前文件，CLI 返回 PACK 根相对路径；AI 按需采纳引用，不自动遍历或执行代码示例。知识不能授予新的外部访问、运行、安装或写入权限。

参考[最小知识示例入口](examples/knowledge-pack/skills/evidence-review/SKILL.md)及其[清单](examples/knowledge-pack/pack.toml)。示例用于评审事实材料，不提供可执行 Workflow；目录与正文可以按真实领域调整。

## 验证与交付

只读验证用 `inspect --pack-dir D`、`inspect knowledge --pack P --pack-dir D`、选定入口的 `--entry E` 与实际链接的 `--path F`。检查名称、描述、原文、引用可读性和最终路径边界。纯知识变更不以空 pytest 或虚构 Run 作为验证。

结构检查只证明可发现、可读取；行为验证使用与经验相符的真实 AI 任务，核对结果是否遵守方法、事实和权限。需要创建 Session、采纳知识或产出报告的验证须在获授权的任务或隔离环境内；再仅凭 Session ID 恢复方法与结果，区分未采纳引用和缺失输入。未执行行为验证必须明确标注。

交付入口与引用变更、实际验证证据、适用范围和限制。修改当前 PACK 不会替换既有 Session 快照；用户明确换方法后由分析流程追加快照并判断受影响正文。
