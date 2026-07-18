import { describe, expect, it } from '@ohos/hypium';
import { abilityDelegatorRegistry, Component, Driver, ON } from '@kit.TestKit';

const CALCULATOR_BUNDLE = 'ohos.samples.distributedcalc';

async function clickById(driver: Driver, id: string): Promise<void> {
  const component: Component = await driver.waitForComponent(ON.id(id), 3000);
  await component.click();
}

export default function calculatorHypiumTest(): void {
  describe('CalculatorHypiumTest', () => {
    it('calculates_100_times_100', 0, async (done: () => void): Promise<void> => {
      const delegator = abilityDelegatorRegistry.getAbilityDelegator();
      await delegator.executeShellCommand(`aa force-stop ${CALCULATOR_BUNDLE}`);
      await delegator.executeShellCommand(`aa start -b ${CALCULATOR_BUNDLE} -a MainAbility`);

      const driver: Driver = Driver.create();
      await driver.delayMs(1000);
      for (const id of ['C', '1', '0', '0', '*', '1', '0', '0', '=']) {
        await clickById(driver, id);
      }

      // 按下等号后，计算器会把最终值写入 expression，并清空预览用的 result。
      const result: Component = await driver.waitForComponent(ON.id('expression'), 3000);
      const text: string = await result.getText();
      expect(text).assertEqual('10000');
      done();
    });
  });
}
