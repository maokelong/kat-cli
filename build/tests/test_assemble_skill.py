from __future__ import annotations

import sys
import tempfile
import unittest
from contextlib import redirect_stderr
from io import StringIO
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

import assemble_skill as assembly


class AssembleSkillTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary_directory = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary_directory.name)
        self.skill = self.root / "skill"
        (self.skill / "agents").mkdir(parents=True)
        (self.skill / "SKILL.md").write_text("---\nname: kat\n---\n", encoding="utf-8")
        (self.skill / "agents" / "openai.yaml").write_text(
            "interface:\n  display_name: KAT\n",
            encoding="utf-8",
        )
        references = self.skill / "references"
        references.mkdir()
        for reference in (
            "analysis-flow.md",
            "pack-authoring-flow.md",
            "result-contract.md",
        ):
            (references / reference).write_text("# Reference\n", encoding="utf-8")
        self.packs = self.root / "packs"
        (self.packs / "kat-example").mkdir(parents=True)
        (self.packs / "kat-example" / "pack.toml").write_text(
            'name = "kat-example"\n', encoding="utf-8"
        )
        self.payload = self.root / "linux-payload"
        (self.payload / "python" / "bin").mkdir(parents=True)
        (self.payload / "kat").write_bytes(b"opaque CLI")
        (self.payload / "python" / "bin" / "python3").write_bytes(b"opaque host")
        (self.payload / "python" / "opaque-native-extension.so").write_bytes(
            b"opaque native"
        )
        self.windows_payload = self.root / "windows-payload"
        (self.windows_payload / "python").mkdir(parents=True)
        (self.windows_payload / "kat.exe").write_bytes(b"opaque Windows CLI")
        (self.windows_payload / "python" / "python.exe").write_bytes(
            b"opaque Windows host"
        )
        (self.windows_payload / "python" / "opaque-native-extension.pyd").write_bytes(
            b"opaque Windows native"
        )
        self.output = self.root / "dist" / "kat"

    def tearDown(self) -> None:
        self.temporary_directory.cleanup()

    def assemble(self) -> Path:
        return assembly.assemble_skill(
            skill_source=self.skill,
            packs=self.packs,
            linux_payload=self.payload,
            windows_payload=self.windows_payload,
            output=self.output,
        )

    def test_maps_each_source_once_without_interpreting_payload(self) -> None:
        self.assertEqual(self.assemble(), self.output.resolve())

        self.assertEqual(
            {
                path.relative_to(self.output).as_posix()
                for path in self.output.rglob("*")
                if path.is_file()
            },
            {
                "SKILL.md",
                "agents/openai.yaml",
                "references/analysis-flow.md",
                "references/pack-authoring-flow.md",
                "references/result-contract.md",
                "assets/packs/kat-example/pack.toml",
                "scripts/targets/linux-x86_64/kat",
                "scripts/targets/linux-x86_64/python/bin/python3",
                "scripts/targets/linux-x86_64/python/opaque-native-extension.so",
                "scripts/targets/windows-x86_64/kat.exe",
                "scripts/targets/windows-x86_64/python/python.exe",
                "scripts/targets/windows-x86_64/python/opaque-native-extension.pyd",
            },
        )
        self.assertEqual(len(list(self.output.rglob("kat-example"))), 1)
        self.assertEqual(len(list(self.output.rglob("python3"))), 1)
        self.assertEqual(len(list(self.output.rglob("python.exe"))), 1)
        self.assertEqual(len(list(self.output.rglob("packs"))), 1)

    def test_missing_input_does_not_leave_partial_output(self) -> None:
        cases = {
            "skill_source": self.root / "missing-skill",
            "packs": self.root / "missing-packs",
            "linux_payload": self.root / "missing-payload",
            "windows_payload": self.root / "missing-windows-payload",
        }

        for argument, missing in cases.items():
            with self.subTest(argument=argument):
                values = {
                    "skill_source": self.skill,
                    "packs": self.packs,
                    "linux_payload": self.payload,
                    "windows_payload": self.windows_payload,
                    "output": self.output,
                }
                values[argument] = missing
                with self.assertRaisesRegex(assembly.AssemblyError, "missing"):
                    assembly.assemble_skill(**values)
                self.assertFalse(self.output.exists())

    def test_missing_reference_does_not_leave_partial_output(self) -> None:
        (self.skill / "references" / "analysis-flow.md").unlink()

        with self.assertRaisesRegex(assembly.AssemblyError, "Skill reference"):
            self.assemble()

        self.assertFalse(self.output.exists())

    def test_copy_failure_removes_staging_and_output(self) -> None:
        real_copytree = assembly.shutil.copytree

        def fail_for_packs(source: Path, destination: Path, **kwargs: object) -> Path:
            if Path(source) == self.packs.resolve():
                raise OSError("injected copy failure")
            return real_copytree(source, destination, **kwargs)

        with mock.patch.object(assembly.shutil, "copytree", side_effect=fail_for_packs):
            with self.assertRaisesRegex(OSError, "injected copy failure"):
                self.assemble()

        self.assertFalse(self.output.exists())
        self.assertEqual(list((self.root / "dist").iterdir()), [])

    def test_rejects_output_or_inputs_that_overlap_sources(self) -> None:
        with self.assertRaisesRegex(assembly.AssemblyError, "overlaps output"):
            assembly.assemble_skill(
                skill_source=self.skill,
                packs=self.packs,
                linux_payload=self.payload,
                windows_payload=self.windows_payload,
                output=self.skill / "dist" / "kat",
            )

        nested_payload = self.packs / "kat-example" / "payload"
        nested_payload.mkdir()
        with self.assertRaisesRegex(assembly.AssemblyError, "inputs overlap"):
            assembly.assemble_skill(
                skill_source=self.skill,
                packs=self.packs,
                linux_payload=nested_payload,
                windows_payload=self.windows_payload,
                output=self.output,
            )

        nested_windows_payload = self.payload / "windows"
        nested_windows_payload.mkdir()
        with self.assertRaisesRegex(assembly.AssemblyError, "inputs overlap"):
            assembly.assemble_skill(
                skill_source=self.skill,
                packs=self.packs,
                linux_payload=self.payload,
                windows_payload=nested_windows_payload,
                output=self.output,
            )

    def test_refuses_to_merge_with_existing_output(self) -> None:
        self.output.mkdir(parents=True)
        (self.output / "duplicate-python").mkdir()

        with self.assertRaisesRegex(assembly.AssemblyError, "already exists"):
            self.assemble()

        self.assertTrue((self.output / "duplicate-python").is_dir())

    def test_cli_rejects_repeated_black_box_inputs(self) -> None:
        arguments = [
            "--skill-source",
            str(self.skill),
            "--packs",
            str(self.packs),
            "--packs",
            str(self.packs),
            "--linux-payload",
            str(self.payload),
            "--windows-payload",
            str(self.windows_payload),
            "--output",
            str(self.output),
        ]

        with redirect_stderr(StringIO()), self.assertRaises(SystemExit) as raised:
            assembly._parser().parse_args(arguments)

        self.assertEqual(raised.exception.code, 2)

    def test_cli_requires_both_payloads(self) -> None:
        arguments = [
            "--skill-source",
            str(self.skill),
            "--packs",
            str(self.packs),
            "--linux-payload",
            str(self.payload),
            "--output",
            str(self.output),
        ]

        with redirect_stderr(StringIO()), self.assertRaises(SystemExit) as raised:
            assembly._parser().parse_args(arguments)

        self.assertEqual(raised.exception.code, 2)


class SkillPlatformContractTests(unittest.TestCase):
    def test_skill_selects_windows_payload_and_rejects_unsupported_hosts(self) -> None:
        skill = (
            Path(__file__).resolve().parents[2] / "kat" / "skill" / "SKILL.md"
        ).read_text(encoding="utf-8")

        self.assertIn("每次操作前选择平台载荷", skill)
        self.assertEqual(
            skill.count("<skill-root>/scripts/targets/windows-x86_64/kat.exe"), 1
        )
        self.assertIn("Windows 10/11 x86_64 客户端", skill)
        self.assertIn("拒绝 Windows Server", skill)
        self.assertIn("ProductType", skill)
        self.assertIn("拒绝其他系统、架构、libc 或版本", skill)
        self.assertIn("KAT Response 是操作成功、失败和可用产物的唯一权威事实", skill)
        self.assertIn("`KAT_DATA_HOME`", skill)
        self.assertIn("`$XDG_DATA_HOME/kat/config.json`", skill)
        self.assertIn("`%APPDATA%\\KAT\\data\\config.json`", skill)
        self.assertIn("references/analysis-flow.md", skill)
        self.assertIn("references/pack-authoring-flow.md", skill)
        self.assertIn("references/result-contract.md", skill)
        self.assertNotIn("<kat> import", skill)
        self.assertNotIn("--pack-root", skill)
        self.assertNotIn("<kat> check", skill)
        self.assertNotIn("<skill-root>/scripts/kat", skill)


class SkillContractTests(unittest.TestCase):
    def setUp(self) -> None:
        self.skill = Path(__file__).resolve().parents[2] / "kat" / "skill"
        self.analysis = (self.skill / "references" / "analysis-flow.md").read_text(
            encoding="utf-8"
        )
        self.authoring = (
            self.skill / "references" / "pack-authoring-flow.md"
        ).read_text(encoding="utf-8")
        self.result = (self.skill / "references" / "result-contract.md").read_text(
            encoding="utf-8"
        )

    def test_contract_covers_the_six_accepted_scenarios(self) -> None:
        scenarios = {
            "source_to_analysis_result": (
                self.analysis,
                "正常分析只接受本地 `.htrace`",
                "`result.path`",
                "`path`、tables 与 schema",
                "Required tables 的唯一 Dataset facts",
                "`kat run`",
                "`kat query --run ... --sql ...`",
                "形成 Analysis Result",
            ),
            "materially_different_candidates_clarify": (
                self.analysis,
                "实质不同分析结论",
                "一个最小必要澄清问题",
                "说明每个选择的差异",
            ),
            "run_follow_up_does_not_rerun": (
                self.analysis,
                "直接进入第 4 步，再进入第 5 步",
                "不得重新导入或运行 Workflow",
                "kat query --run",
            ),
            "no_match_does_not_write_pack": (
                self.analysis,
                "以受阻状态交付已检查的 Dataset facts 和能力边界",
                "可以建议新建或扩展 PACK",
                "不得修改源码或切换作者流",
            ),
            "pack_understanding_is_read_only": (
                self.authoring,
                "对于“理解 PACK”",
                "每个 Workflow 所需的 Dataset facts 或参数",
                "已有测试或验证证据、明确限制与下一步",
                "不要复述目录、manifest 或源码",
            ),
            "explicit_fix_requires_inspect_and_test": (
                self.authoring,
                "只有用户明确要求创建、修改或修复时才能写入",
                "重新 inspection",
                "运行适用的 `kat test`",
                "交付变更摘要、受影响文件、实际验证证据和仍存限制",
            ),
        }

        for name, (document, *expectations) in scenarios.items():
            with self.subTest(scenario=name):
                for expectation in expectations:
                    self.assertIn(expectation, document)

        self.assertIn("对用户问题的直接结论", self.result)
        self.assertIn("少量可追溯证据", self.result)
        self.assertIn("当前任务阶段与已经确认的事实", self.result)
        self.assertIn("停止在哪个阶段", self.result)

    def test_source_flow_inspects_before_discovery_and_query(self) -> None:
        checkpoints = (
            "`kat import hitrace`",
            "`result.path`",
            "`kat inspect --dataset`",
            "`path`、tables 与 schema",
            "无目标调用 `kat inspect`",
            "`kat run`",
            "`kat query --run ... --sql ...`",
            "形成 Analysis Result",
        )
        positions = [self.analysis.index(checkpoint) for checkpoint in checkpoints]

        self.assertEqual(positions, sorted(positions))

    def test_authorized_change_inspects_and_tests_before_delivery(self) -> None:
        change_start = self.authoring.index("每次写入后：")
        checkpoints = (
            "重新 inspection",
            "运行适用的 `kat test`",
            "交付变更摘要、受影响文件、实际验证证据",
        )
        positions = [
            self.authoring.index(checkpoint, change_start) for checkpoint in checkpoints
        ]

        self.assertEqual(positions, sorted(positions))

    def test_existing_run_queries_before_forming_analysis_result(self) -> None:
        run_start = self.analysis.index("- Run：直接进入第 4 步，再进入第 5 步")
        query = self.analysis.index("`kat query --run ... --sql ...`", run_start)
        delivery = self.analysis.index("## 5. 形成交付", query)

        self.assertLess(run_start, query)
        self.assertLess(query, delivery)

    def test_result_contract_has_only_the_three_agreed_outcomes(self) -> None:
        for heading in ("## 已完成", "## 需要补充信息", "## 执行失败或受阻"):
            self.assertIn(heading, self.result)
        self.assertIn("一次只提出一个澄清问题", self.result)
        self.assertIn("不得把部分输出", self.result)

    def test_data_home_configuration_respects_pr_177_precedence_and_user_authorization(
        self,
    ) -> None:
        skill = (self.skill / "SKILL.md").read_text(encoding="utf-8")

        self.assertLess(skill.index("非空 `KAT_DATA_HOME`"), skill.index("config.json.kat_data_home"))
        self.assertIn("有效时直接选中且不读取 `config.json`", skill)
        self.assertIn("只有该变量缺失或为空时才读取配置", skill)
        self.assertIn("无效的已选值会使操作失败，不回退", skill)
        self.assertIn("提醒你当前平台的默认位置，并询问是否要更换 Data Home", skill)
        self.assertIn("这个问题每次对话只问一次", skill)
        self.assertIn("用户明确要求更换且提供路径后", skill)
        self.assertIn("原样保留未知字段", skill)
        self.assertIn("不覆盖损坏文件，也不调用 KAT", skill)
        self.assertIn("仅为用户当前请求的 KAT 进程设置同一 `KAT_DATA_HOME`", skill)
        self.assertIn("更新的配置文件路径与规范化后的目录", self.result)


if __name__ == "__main__":
    unittest.main()
