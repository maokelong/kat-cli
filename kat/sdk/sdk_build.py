"""SDK 构建期知识生成；不进入安装包。"""

from ast import literal_eval
from pathlib import Path
from urllib.parse import unquote, urlsplit

from griffe import visit
from griffe2md import render_object_docs
from markdown_it import MarkdownIt
from setuptools.command.build_py import build_py


def generate_knowledge(package: Path) -> None:
    config = {
        "show_signature_annotations": True,
        "show_submodules": False,
        "show_root_heading": True,
        "heading_level": 1,
    }
    for category in ("providers", "workflows"):
        source = package / category
        if not source.is_dir():
            continue
        for path in sorted(source.rglob("*.py")):
            if path.name.startswith("_"):
                continue
            relative = path.relative_to(package).with_suffix("")
            obj = visit(".".join(("kat_sdk", *relative.parts)), path,
                        path.read_text(encoding="utf-8"), docstring_parser="google")
            validate_declarations(obj, package, category)
            target = package / "knowledge" / relative.with_suffix(".api.md")
            if target.exists():
                raise ValueError(f"Generated API document would overwrite a source: {target}")
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text(render_object_docs(obj, config), encoding="utf-8", newline="\n")
    validate_links(package / "knowledge")


def validate_declarations(module, package: Path, category: str) -> None:
    for member in module.members.values():
        if member.is_alias:
            continue
        for decorator in getattr(member, "decorators", ()):
            value = decorator.value
            if str(getattr(value, "function", "")).split(".")[-1] not in ("provider", "workflow"):
                continue
            keywords = {argument.name: argument.value for argument in value.arguments
                        if hasattr(argument, "name")}
            if "guide" not in keywords:
                if category == "providers":
                    raise ValueError(f"Provider must declare a Guide: {member.path}")
                continue
            reference = literal_eval(str(keywords["guide"]))
            if reference is None and category == "workflows":
                continue
            if not isinstance(reference, str):
                raise ValueError(f"Guide must be a string: {member.path}")
            relative = Path(reference)
            knowledge = (package / "knowledge").resolve()
            target = (knowledge / relative).resolve()
            if (relative.is_absolute() or ".." in relative.parts
                    or not relative.parts or relative.parts[0] != category
                    or relative.suffix != ".md" or not target.is_relative_to(knowledge)
                    or not target.is_file() or not target.read_text(encoding="utf-8").strip()):
                raise ValueError(f"Missing or invalid Guide: {member.path}: {reference}")


def validate_links(knowledge: Path) -> None:
    parser = MarkdownIt("commonmark")
    root = knowledge.resolve()
    for document in sorted(knowledge.rglob("*.md")):
        text = document.read_text(encoding="utf-8")
        if not text.strip():
            raise ValueError(f"Knowledge document is empty: {document}")
        for block in parser.parse(text):
            for token in block.children or ():
                if token.type not in ("link_open", "image"):
                    continue
                reference = token.attrGet("href" if token.type == "link_open" else "src")
                parsed = urlsplit(reference or "")
                if parsed.scheme or parsed.netloc or not parsed.path:
                    continue
                target = (document.parent / unquote(parsed.path)).resolve()
                if not target.is_relative_to(root) or not target.is_file():
                    raise ValueError(f"Broken knowledge link in {document}: {reference}")


class BuildPy(build_py):
    def find_package_modules(self, package, package_dir):
        return [
            module for module in super().find_package_modules(package, package_dir)
            if module[1] != "sdk_build"
        ]

    def run(self) -> None:
        super().run()
        generate_knowledge(Path(self.build_lib) / "kat_sdk")
