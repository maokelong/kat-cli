# 事实评审知识示例

维护方：KAT 作者示例。这个 PACK 演示不依赖 Python 的经验知识入口，不是生产评审政策。

## 使用场景与示例

可以提出：“请按证据核查这份材料中的结论是否成立。”KAT 展示相关候选后，选择本 PACK 的 `evidence-review`。入口按[证据准则](knowledge/evidence-rules.md)检查给定材料，交付影响判断的问题及实际依据；没有发现时说明检查范围。

## 输入、结果与限制

需要用户明确的材料和待核查问题。无需 Python、Workflow 或 Run；输入不足时说明缺口，不推断整体正确，不修改被评审材料。知识方法本身不授予外部操作权限。

## 开发者复用

没有 Provider 或普通函数。作者可查看[知识入口](skills/evidence-review/SKILL.md)及其相对引用，按自己的领域维护方法，不把本示例当作团队政策。

<!-- kat:capabilities:start -->
## 当前能力（自动同步）

**事实评审知识示例**（`knowledge-example`）；维护方：KAT 作者示例。

示范无需 Python 的知识入口及按需引用；不是生产评审政策。

### 知识入口

- [evidence-review](skills/evidence-review/SKILL.md)：按事实与范围审视给定材料中的判断，给出有依据的问题或明确无发现。

### Workflow

尚无已发布 Workflow。

Provider 与普通函数的复用说明见作者维护的正文；它们不是直接分析入口。
<!-- kat:capabilities:end -->
