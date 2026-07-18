import { abilityDelegatorRegistry, TestRunner } from '@kit.TestKit';
import { UIAbility, Want } from '@kit.AbilityKit';
import { BusinessError } from '@kit.BasicServicesKit';

export default class OpenHarmonyTestRunner implements TestRunner {
  onPrepare(): void {
  }

  onRun(): void {
    const delegator = abilityDelegatorRegistry.getAbilityDelegator();
    const args = abilityDelegatorRegistry.getArguments();
    const moduleName = args.parameters['-m'];
    const monitor: abilityDelegatorRegistry.AbilityMonitor = {
      abilityName: 'TestAbility',
      moduleName: moduleName
    };
    delegator.addAbilityMonitor(monitor, (_error: BusinessError) => {
    });
    const want: Want = {
      bundleName: args.bundleName,
      abilityName: 'TestAbility',
      moduleName: moduleName
    };
    delegator.startAbility(want, (_error: BusinessError, _data: void) => {
    });
  }
}
