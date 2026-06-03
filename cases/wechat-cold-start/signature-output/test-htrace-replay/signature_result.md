# Signature Result: harmony_wechat_cold_start_js_load

Status: `match`

Trace: `C:\Users\77294\AppData\Local\Temp\kat-rs-web-ui\uploads\1780480119322465500-test.htrace`

## Summary

- Target process: `.tencent.wechat` pid=15040 upid=329
- Anchor confidence: `fallback`
- Max phase: `C_launch_ability_to_transaction` 483.568 ms
- Max phase ratio: `0.699032`
- Main running ratio in max phase: `0.972535`
- Small ratio: `0.001531`
- Top hotspot: `H:virtual int OHOS::AppExecFwk::AppSchedulerHost::OnRemoteRequest(uint32_t, OHOS::MessageParcel &, OHOS::MessageParcel &, OHOS::MessageOption &)|I31` 483.568 ms

## Predicates

- `max_phase_is_launch_ability`: `True`
- `max_phase_ratio_high`: `True`
- `main_thread_running_dominant`: `True`
- `js_load_hotspot`: `True`
- `not_small_core_issue`: `True`

## CPU Cluster Total

- `big`: 595.208 ms
- `middle`: 24.55 ms
- `small`: 0.95 ms
