#!/usr/bin/env python3
"""Capture actual Flux egui windows, never browser mockups.

Usage: python3 scripts/native-visual-qa.py <fixture-directory> <output-directory>
Requires a running desktop session. Each case gets isolated preferences.
Optional FLUX_QA_BINARY overrides target/release/flux-files.
"""
import os
from pathlib import Path
import subprocess
import sys

fixture = Path(sys.argv[1]).resolve()
output = Path(sys.argv[2]).resolve()
output.mkdir(parents=True, exist_ok=True)
binary = Path(os.environ.get("FLUX_QA_BINARY", "target/release/flux-files")).resolve()
cases = [
    ("explorer-dark", {"THEME": "dark"}),
    ("explorer-light", {"THEME": "light"}),
    ("grid-dark", {"THEME": "dark", "VIEW": "grid"}),
    ("gallery-light", {"THEME": "light", "VIEW": "gallery"}),
    ("preview-dark", {"THEME": "dark", "PREVIEW": "Autumn.jpg"}),
    ("terminal-dark", {"THEME": "dark", "SCREEN": "terminal"}),
    ("rename-dark", {"THEME": "dark", "SCREEN": "rename"}),
    ("settings-dark", {"THEME": "dark", "SCREEN": "settings"}),
    ("settings-light", {"THEME": "light", "SCREEN": "settings"}),
    ("typography-dark", {"THEME": "dark", "SCREEN": "typography"}),
    ("menu-dark", {"THEME": "dark", "MENU": "row"}),
    ("menu-light", {"THEME": "light", "MENU": "bg"}),
    ("small-rename", {"THEME": "dark", "SCREEN": "rename", "SIZE": "1024x768"}),
    ("small-settings", {"THEME": "light", "SCREEN": "settings", "SIZE": "1024x768"}),
    ("scaled-grid", {"THEME": "dark", "VIEW": "grid", "SCALE": "2", "SIZE": "1024x768"}),
]
for category in ["files", "views", "keys", "advanced", "about"]:
    cases.append((f"settings-{category}", {"THEME": "dark", "SCREEN": category}))
for name, overrides in cases:
    env = dict(os.environ)
    for key in list(env):
        if key.startswith("FLUX_SHOT_"):
            del env[key]
    config = output / f"{name}.json"
    config.unlink(missing_ok=True)
    env.update({"FLUX_SETTINGS_PATH": str(config), "FLUX_SHOT_OUT": str(output / f"{name}.png"),
                "FLUX_SHOT_DIR": str(fixture), "FLUX_SHOT_SIZE": "1440x900", "FLUX_SHOT_WAIT": "90"})
    if "PREVIEW" not in overrides:
        env["FLUX_SHOT_NO_PREVIEW"] = "1"
    env.update({f"FLUX_SHOT_{key}": value for key, value in overrides.items()})
    with (output / f"{name}.log").open("w") as log:
        subprocess.run([str(binary)], env=env, stdout=log, stderr=log, timeout=40, check=True)
    if not (output / f"{name}.png").is_file():
        raise RuntimeError(f"No native screenshot for {name}")
    print(name, flush=True)
