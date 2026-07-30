---
status: accepted
---

# Hitrace 事件保留时钟域与原始读数

HiProfiler `.htrace` 是多时钟容器，采集证据不保证持续校准的公共时间线；因此 Datasource 以不可分割的 `UnifiedClock { ClockDomain, ClockValue }` 保留具体时钟域与非负原始读数，并把域定义及 snapshot 作为普通 Source facts，header 与后续非空 snapshot groups 按来源顺序从零编号，使 `snapshot_id = 0` 成为当前 trace segment 的 baseline。Datasource 只从明确且一致的来源证据确定 domain，歧义、冲突或定义不完整时整体导入失败；不同 Dataset 的同名 domain 不等同，读数也只有明确对齐后才能比较、相减或解释为 UTC/Duration。跨域换算由 PACK 选择显式目标，首版只支持每秒 `1_000_000_000` ticks 的同频 checked 平移；domain/value 同空传播 null、半空失败，缺失 baseline、越界或其他频率也整体失败，KAT 不做异频缩放、舍入、多跳或漂移修正，结果仍是目标 domain 的 ClockValue。
