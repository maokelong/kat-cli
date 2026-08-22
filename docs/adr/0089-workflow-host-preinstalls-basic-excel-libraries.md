---
status: accepted
---

# Workflow Host 预装基础 Excel 读写库

`kat-workflow` Host 除 PostgreSQL common 依赖外，正式预装 `openpyxl==3.1.5`、`XlsxWriter==3.2.9` 和 `defusedxml==0.7.1`，并将其及传递依赖写入 Windows、Linux 的同一套锁定构建流程。PACK 直接导入这些成熟第三方库：openpyxl 用于读取、修改和写入 Office Open XML 工作簿，XlsxWriter 用于生成具有丰富格式能力的 `.xlsx`，defusedxml 供 openpyxl 的安全 XML 解析路径使用。

第一版不为这些库增加 `kat.common.excel` 包装，不预装 pandas，也不承诺旧 `.xls` 或 `.xlsb`。Host 构建验证必须在锁定的 Python 3.14 环境中创建一个 `.xlsx`、重新读取并断言内容，证明依赖不只是进入锁文件而是能够实际运行；后续格式或数据框集成只在出现真实 PACK 消费者后增加。
