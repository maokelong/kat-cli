# WeChat Cold Start DataFusion Replay Case

This case records a deterministic replay flow for HarmonyOS WeChat cold-start analysis.

It includes:

- Atomic capability specs under `docs/capabilities/`.
- Strategy docs under `docs/strategies/`.
- Analysis reports under `docs/analysis/`.
- A DataFusion-only replay runner under `tools/datafusion_signature_runner.py`.
- Golden replay outputs under `signature-output/`.

The trace file is intentionally not included in this case directory. Use an already loaded kat-rs Web UI dataset, or pass a local trace with `--trace`.

Example:

```powershell
python cases\wechat-cold-start\tools\datafusion_signature_runner.py `
  --server http://127.0.0.1:8787 `
  --trace tests\test.htrace `
  --out-dir cases\wechat-cold-start\signature-output\test-htrace-replay
```
