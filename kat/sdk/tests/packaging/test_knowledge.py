from __future__ import annotations

import importlib.util
from importlib.metadata import distribution
from pathlib import Path
import re
import subprocess
import sys
import unittest

from kat import Context, knowledge


class KnowledgeTests(unittest.TestCase):
    def test_all_topics_are_installed_and_unknown_names_are_rejected(self):
        for topic in ("index", "authoring", "ftrace", "trace_streamer"):
            self.assertTrue(knowledge.read(topic).startswith("# "))
        for topic in ("../_workflow", "missing", "/index", "index.md"):
            with self.assertRaises(ValueError):
                knowledge.read(topic)

    def test_installed_markdown_is_centralized_and_inspection_reads_it(self):
        markdown = {str(path).replace("\\", "/") for path in distribution("kat-sdk").files
                    if str(path).endswith(".md") and str(path).startswith("kat")}
        self.assertEqual(markdown, {
            "kat/knowledge/index.md",
            "kat/knowledge/authoring/pack-authoring-flow.md",
            "kat/knowledge/providers/ftrace/guide.md",
            "kat/knowledge/providers/trace_streamer/guide.md",
        })
        from kat._runtime.provider_inspection import inspect_provider
        for name, topic in (("ftrace-text", "ftrace"), ("trace-streamer-sqlite", "trace_streamer")):
            self.assertEqual(inspect_provider(None, None, name).provider["guide"], knowledge.read(topic))

    def test_framework_import_does_not_load_providers_or_runtime(self):
        subprocess.run([sys.executable, "-I", "-c",
            "import sys; import kat.dataprovider; "
            "assert not any(n.startswith(('kat.providers', 'kat._runtime')) for n in sys.modules)"
        ], check=True)

    def test_documented_sdk_examples_execute(self):
        for topic in ("authoring",):
            examples = re.findall(r"~~~python\n(.*?)\n~~~", knowledge.read(topic), re.S)
            self.assertEqual(len(examples), 2)
            for source in examples:
                namespace = {}
                exec(source, namespace)
                if "table" in namespace:
                    self.assertEqual(namespace["table"].to_rows(), [{"value": 1}, {"value": 2}])
                if "prepare" in namespace:
                    self.assertIsNone(namespace["prepare"](Context()))

    def test_context_cannot_fake_runtime_capabilities(self):
        with self.assertRaises(RuntimeError):
            Context().run("sample", "prepare")
        with self.assertRaises(RuntimeError):
            _ = Context().scratch_root

    def test_sdk_includes_runtime_and_datasource_but_no_cli(self):
        self.assertIsNone(importlib.util.find_spec("_kat_cli"))
        for old in ("kat_sdk", "_kat_runtime", "kat_datasource", "kat.api", "kat.runtime", "kat.datasource"):
            self.assertIsNone(importlib.util.find_spec(old), old)
        for name in ("kat._runtime", "kat.providers.decoding"):
            self.assertIsNotNone(importlib.util.find_spec(name), name)


if __name__ == "__main__":
    unittest.main()
