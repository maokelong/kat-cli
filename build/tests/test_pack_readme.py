from __future__ import annotations

import importlib.util
from pathlib import Path
import tempfile
import unittest
from unittest import mock


SCRIPT = Path(__file__).resolve().parents[2] / "kat/skills/kat-author/scripts/pack_readme.py"
SPEC = importlib.util.spec_from_file_location("pack_readme", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
readme = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(readme)

PACK = {"name": "sample", "title": "示例", "description": "可验证的示例能力", "owner": "测试团队"}
WORKFLOW = {"name": "summarize", "description": "汇总给定观察", "parameters": [
    {"name": "enabled", "option": "--enabled", "negative_option": "--no-enabled",
     "type": "bool", "required": False, "default": True, "description": "是否汇总 | 所有项"},
    {"name": "mode", "option": "--mode", "type": "str", "required": True,
     "choices": ["a", "b"], "description": "汇总口径"}]}


class PackReadmeTests(unittest.TestCase):
    def test_empty_pack_has_no_invented_entry_or_program(self):
        content = readme.initial_readme(PACK)
        self.assertIn("尚无已发布知识入口", content)
        self.assertIn("尚无已发布 Workflow", content)
        self.assertNotIn("kat run", content)
        self.assertEqual(readme.replace_facts(content, readme.render_facts(PACK, [], [])), content)

    def test_prose_is_preserved_and_update_is_idempotent(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            path = root / "README.md"
            prose = "# 使用者手写标题\r\n\r\n说明 [来源](source.txt)。\r\n"
            (root / "source.txt").write_text("真实输入", encoding="utf-8")
            path.write_bytes(prose.encode("utf-8"))
            facts = readme.render_facts(PACK, [], [WORKFLOW])
            with mock.patch.object(readme, "collect_facts", return_value=facts):
                readme.update_readme(Path("kat"), root)
                first = path.read_bytes()
                readme.update_readme(Path("kat"), root)
                readme.update_readme(Path("kat"), root, check=True)
            self.assertTrue(first.decode("utf-8").startswith(prose.rstrip()))
            self.assertEqual(path.read_bytes(), first)

    def test_added_removed_and_changed_capabilities_require_regeneration(self):
        baseline = readme.render_facts(PACK, [], [WORKFLOW])
        changed = {**WORKFLOW, "parameters": [{**WORKFLOW["parameters"][0], "default": False}]}
        alternatives = [readme.render_facts(PACK, [], []),
                        readme.render_facts(PACK, [], [WORKFLOW, {**WORKFLOW, "name": "second"}]),
                        readme.render_facts(PACK, [], [changed])]
        for facts in alternatives:
            with self.subTest(facts=facts), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary).resolve()
                path = root / "README.md"
                path.write_text(baseline + "\n", encoding="utf-8")
                before = path.read_bytes()
                with mock.patch.object(readme, "collect_facts", return_value=facts):
                    with self.assertRaisesRegex(ValueError, "stale"):
                        readme.update_readme(Path("kat"), root, check=True)
                    self.assertEqual(path.read_bytes(), before)
                    readme.update_readme(Path("kat"), root)
                    readme.update_readme(Path("kat"), root, check=True)

    def test_failure_during_inspection_does_not_modify_readme(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            (root / "pack.toml").write_text('name="sample"\n', encoding="utf-8")
            (root / "workflows").mkdir()
            (root / "workflows/example.py").write_text("# Source exists", encoding="utf-8")
            path = root / "README.md"
            path.write_text("# 原说明\n", encoding="utf-8")
            before = path.read_bytes()
            responses = [{"packs": [PACK]}, {"entries": []},
                         {"workflows": [{"name": "summarize"}]}, ValueError("broken import")]
            with mock.patch.object(readme, "inspect", side_effect=responses):
                with self.assertRaisesRegex(ValueError, "broken import"):
                    readme.update_readme(Path("kat"), root)
            self.assertEqual(path.read_bytes(), before)

    def test_pure_knowledge_documentation_does_not_start_workflow_host(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            (root / "pack.toml").write_text('name="sample"\n', encoding="utf-8")
            entry = root / "skills/review/SKILL.md"
            entry.parent.mkdir(parents=True)
            entry.write_text("# Review", encoding="utf-8")
            entries = [{"name": "review", "description": "核对事实", "disable-model-invocation": True}]
            with mock.patch.object(readme, "inspect", side_effect=[{"packs": [PACK]}, {"entries": entries}]):
                readme.update_readme(Path("kat"), root)
            content = (root / "README.md").read_text(encoding="utf-8")
            self.assertIn("skills/review/SKILL.md", content)
            self.assertIn("只能由用户主动选用", content)
            self.assertIn("尚无已发布 Workflow", content)

    def test_partial_repeated_or_reversed_markers_are_rejected(self):
        facts = readme.render_facts(PACK, [], [])
        for content in (readme.START, readme.END, readme.END + readme.START,
                        facts + facts):
            with self.subTest(content=content), self.assertRaisesRegex(ValueError, "markers"):
                readme.replace_facts(content, facts)

    def test_git_checkout_line_endings_do_not_make_facts_stale(self):
        facts = readme.render_facts(PACK, [], [WORKFLOW])
        for newline in ("\n", "\r\n"):
            with self.subTest(newline=newline):
                current = ("# Owner prose\n\n" + facts + "\n\nAfterword.\n").replace("\n", newline)
                self.assertEqual(readme.replace_facts(current, facts), current)

    def test_markdown_links_use_parser_and_resolve_within_pack(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            (root / "有效 文件.md").write_text("# Valid", encoding="utf-8")
            valid = "[link][ref]\n\n[ref]: <%E6%9C%89%E6%95%88%20%E6%96%87%E4%BB%B6.md#section>\n\n```md\n[example](missing.md)\n```\n[web](https://example.com/guide)\n"
            readme.validate_links(root, valid)
            for invalid in ("[bad](missing.md)", "![bad](../outside.png)",
                            "[bad][ref]\n\n[ref]: absent.md"):
                with self.subTest(invalid=invalid), self.assertRaisesRegex(ValueError, "link"):
                    readme.validate_links(root, invalid)

    def test_parameters_keep_real_options_defaults_and_choices(self):
        facts = readme.render_facts(PACK, [], [WORKFLOW])
        for expected in ("--enabled / --no-enabled", "true", "&quot;a&quot;", "&#124;"):
            self.assertIn(expected, facts)

    def test_a_disabled_entry_is_documented_without_promising_user_invocation(self):
        entries = [{"name": "private", "description": "受限方法", "user-invocable": False,
                    "disable-model-invocation": True}]
        facts = readme.render_facts(PACK, entries, [])
        self.assertIn("不提供用户调用选项", facts)
        self.assertIn("不由模型自动选用", facts)
        self.assertNotIn("只能由用户主动选用", facts)

    def test_check_does_not_create_a_missing_readme(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            facts = readme.render_facts(PACK, [], [])
            with mock.patch.object(readme, "collect_facts", return_value=facts):
                with self.assertRaisesRegex(ValueError, "missing or stale"):
                    readme.update_readme(Path("kat"), root, check=True)
            self.assertFalse((root / "README.md").exists())

    def test_invalid_workflow_tree_is_not_reported_as_no_capabilities(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            (root / "pack.toml").write_text('name="sample"\n', encoding="utf-8")
            (root / "workflows").write_text("not a directory", encoding="utf-8")
            with mock.patch.object(readme, "inspect", side_effect=[{"packs": [PACK]}, {"entries": []}]):
                with self.assertRaisesRegex(ValueError, "ordinary directory"):
                    readme.update_readme(Path("kat"), root)
            self.assertFalse((root / "README.md").exists())


if __name__ == "__main__":
    unittest.main()
