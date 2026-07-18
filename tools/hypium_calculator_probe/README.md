# Hypium 计算器穿刺

这个独立 OpenHarmony `ohosTest` 工程使用 Hypium 和 `@kit.TestKit`：

- 启动 `ohos.samples.distributedcalc/MainAbility`；
- 依次点击 `C 1 0 0 * 1 0 0 =`；
- 读取计算器最终显示节点 `expression`，并断言其值为 `10000`。

在本目录安装依赖并使用 DevEco Studio 自带的 Hvigor 构建：

```powershell
& 'D:\soft\DevEcoStudio\tools\ohpm\bin\ohpm.bat' install
& 'D:\soft\DevEcoStudio\tools\hvigor\bin\hvigorw.bat' --no-daemon --no-parallel --mode module -p product=default -p module=entry assembleHap
& 'D:\soft\DevEcoStudio\tools\hvigor\bin\hvigorw.bat' --no-daemon --no-parallel --mode module -p product=default -p module=entry@ohosTest assembleHap
```

主 HAP 和 `ohosTest` HAP 需使用同一调试证书和包含目标设备 UDID 的调试 Profile 签名。先安装主 HAP，再安装测试 HAP，最后运行：

```powershell
hdc -t <target> shell "aa test -b com.katrs.hypium.calculator -m entry_test -s unittest OpenHarmonyTestRunner -s class CalculatorHypiumTest -s timeout 30000"
```

在 OpenHarmony API 26 设备 `7001005458323933328a521c3c503800` 上的实测结果：`Tests run: 1, Failure: 0, Error: 0, Pass: 1`。
