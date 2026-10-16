#!/usr/bin/env python3
"""Copy GUI help and numeric wheel settings from the pinned reference (no imports).

The generated JSON is compiled into the Rust GUI; Python is extraction tooling only.
Copied text retains the reference repository's GPL-3.0 license (see LICENSE).
"""
import ast
import hashlib
import json
import pathlib
import subprocess

ROOT = pathlib.Path(__file__).resolve().parents[1]
REFERENCE = ROOT.parent / "spektrafilm"
BASELINE = "28bf883e1672e884307edc75852549376e13644e"
PREFIXES = {
    "film_render.dir_couplers": "dir_couplers",
    "film_render.halation": "halation",
    "film_render.grain": "grain",
    "film_render.base": "film_base",
    "film_render.convert": "convert",
    "film_render.chemistry": "film_chemistry",
    "print_render.base": "print_base",
    "print_render.glare": "print_glare",
    "print_render.chemistry": "print_chemistry",
    "selection.film_stock": "film_profile",
    "selection.print_paper": "print_profile",
    "special.print_illuminant": "enlarger.illuminant",
    "film_channel_swap": "special.film_channel_swap",
    "print_channel_swap": "special.print_channel_swap",
    "route": "workflow.route",
}


def literal(node, constants):
    if isinstance(node, ast.Name):
        return constants[node.id]
    if isinstance(node, ast.JoinedStr):
        return "".join(str(literal(v.value, constants)) if isinstance(v, ast.FormattedValue)
                       else v.value for v in node.values)
    return ast.literal_eval(node)


def extract(relative, raw=False):
    source = subprocess.check_output(["git", "-C", str(REFERENCE), "show", f"{BASELINE}:{relative}"])
    tree = ast.parse(source)
    constants = {}
    for node in tree.body:
        if isinstance(node, ast.Assign) and len(node.targets) == 1 and isinstance(node.targets[0], ast.Name):
            try:
                constants[node.targets[0].id] = literal(node.value, constants)
            except (ValueError, KeyError):
                pass
    fields = {}
    for node in ast.walk(tree):
        if not isinstance(node, ast.Call) or not isinstance(node.func, ast.Name) or node.func.id != "ParamSpec":
            continue
        # _diffusion_manifest expands once for each lens.
        for diffusion in ("camera.diffusion_filter", "enlarger.diffusion_filter"):
            path = literal(node.args[0], constants | {"_D": diffusion})
            if raw:
                if "." in path:
                    continue
                path = "raw." + path
            for old, new in PREFIXES.items():
                if path == old or path.startswith(old + "."):
                    path = new + path[len(old):]
                    break
            metadata = {}
            for kw in node.keywords:
                if kw.arg in ("tooltip", "min", "max", "step"):
                    metadata[kw.arg] = literal(kw.value, constants)
            fields["/" + path.replace(".", "/")] = metadata
    return fields, hashlib.sha256(source).hexdigest()


if __name__ == "__main__":
    fields, manifest_hash = extract("src/spektrafilm_gui/params_manifest.py")
    raw, sections_hash = extract("src/spektrafilm_gui/widget_sections.py", raw=True)
    fields.update(raw)
    result = {
        "baseline": BASELINE,
        "license": "GPL-3.0-only; copied from spektrafilm (see LICENSE)",
        "source_sha256": {"params_manifest.py": manifest_hash, "widget_sections.py": sections_hash},
        "fields": [{"path": path, **spec} for path, spec in fields.items()],
    }
    (ROOT / "src/gui/reference_parameters.json").write_text(json.dumps(result, indent=2, ensure_ascii=False) + "\n")
