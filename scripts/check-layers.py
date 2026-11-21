#!/usr/bin/env python3
"""Fails when a crate depends across the layers: psx-* never on BoF3 or app crates, no crate on an app."""
import json
import subprocess
import sys

APPS = {"rebof3", "rebof3-editor"}
SHELLS = {"psx-capi"}
UI = {"gpui", "ratatui", "cpal"}

metadata = json.loads(subprocess.check_output(["cargo", "metadata", "--no-deps", "--format-version", "1", "--locked"]))
members = {package["name"] for package in metadata["packages"]}
failures = []
for package in metadata["packages"]:
    name = package["name"]
    needs = {dep["name"] for dep in package["dependencies"] if dep["kind"] != "dev"}
    for dep in sorted(needs & APPS):
        failures.append(f"{name} depends on the app {dep}")
    if name.startswith("psx-") and name not in SHELLS:
        for dep in sorted(needs & members):
            if not dep.startswith("psx-"):
                failures.append(f"{name} depends on {dep}, which is above the psx layer")
        for dep in sorted(needs & UI):
            failures.append(f"{name} depends on the UI crate {dep}")
print("\n".join(failures) or "layers ok")
sys.exit(1 if failures else 0)
