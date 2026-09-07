import json
from pathlib import Path



def record(ctx, evidence, name):
    scratch = ctx.scratch_root
    Path(evidence, f"{name}.json").write_text(json.dumps({"scratch": str(scratch)}))
    scratch.joinpath("temporary").write_text("discardable")
    return scratch


def replace_with_file(scratch):
    scratch.joinpath("temporary").unlink()
    scratch.rmdir()
    scratch.write_text("replacement")
