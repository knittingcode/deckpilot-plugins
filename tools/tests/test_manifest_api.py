#!/usr/bin/env python3
"""Plugin API 1.3 and 1.9 manifest rules, checked with the pinned CLI (stdlib only).

    python3 -I tools/tests/test_manifest_api.py --cli .work/bin/deckpilot-registry \\
        --module .work/first-party/target/wasm32-unknown-unknown/guest/deckpilot_template.wasm

Each case copies first-party/template to a scratch directory, changes its
manifest.json (or an icon) and runs the real `deckpilot-registry package
--first-party` on it, as tools/first-party.sh does: the manifest rules, the
package rules and the navigation icon checks of the pinned DeckPilot
commit, including the refusal of the plugin API 1.9 permissions in any
registry package (only the plugins bundled with DeckPilot may declare
them). The module is the template's, built by `tools/first-party.sh
package`. Without --cli and --module the cases are skipped.
"""

import json
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

TEMPLATE = Path(__file__).resolve().parent.parent.parent / "first-party" / "template"
CLI = None
MODULE = None

SCRIPTED_SVG = (
    '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24">'
    '<script>alert(1)</script><path d="M5 5v14"/></svg>\n'
)
HANDLER_SVG = (
    '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" onload="alert(1)">'
    '<path d="M5 5v14"/></svg>\n'
)


class ManifestApi(unittest.TestCase):
    def setUp(self):
        if not (CLI and MODULE):
            self.skipTest("no CLI and module given (--cli PATH --module PATH)")
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = Path(self.tmp.name) / "plugin"
        shutil.copytree(TEMPLATE, self.dir, ignore=shutil.ignore_patterns("target"))
        self.manifest = json.loads((self.dir / "manifest.json").read_text(encoding="utf-8"))

    def tearDown(self):
        self.tmp.cleanup()

    def package(self):
        (self.dir / "manifest.json").write_text(json.dumps(self.manifest, indent=2) + "\n", encoding="utf-8")
        result = subprocess.run(
            [CLI, "package", "--plugin-dir", str(self.dir), "--module", MODULE,
             "--out", str(Path(self.tmp.name) / "out.dpplugin"), "--first-party"],
            capture_output=True, text=True,
        )
        return result.returncode, result.stdout + result.stderr

    def assertPackaged(self):
        status, out = self.package()
        self.assertEqual(status, 0, out)
        self.assertIn("deckpilot.template", out)

    def assertRefused(self, *needles):
        status, out = self.package()
        self.assertNotEqual(status, 0, out)
        for needle in needles:
            self.assertIn(needle, out)

    def as_api_1_2(self):
        """The template as a plugin API 1.2 manifest: no controls, no navigation."""
        self.manifest["engines"]["pluginApi"] = "^1.2"
        for action in self.manifest["contributes"]["actions"]:
            action.pop("controls", None)
        self.manifest["contributes"].pop("navigation")
        self.manifest["contributes"].pop("views")

    def test_api_1_2_without_controls_still_packages(self):
        self.as_api_1_2()
        self.assertPackaged()

    def test_api_1_3_with_controls_and_navigation_packages(self):
        self.assertEqual(self.manifest["engines"]["pluginApi"], "^1.3")
        self.assertIn("controls", self.manifest["contributes"]["actions"][0])
        self.assertPackaged()

    def test_controls_below_api_1_3_are_refused(self):
        self.as_api_1_2()
        self.manifest["contributes"]["actions"][0]["controls"] = {"key": ["press"], "encoder": ["rotate-clockwise"]}
        self.assertRefused("contributes.actions[0].controls", "needs plugin API 1.3")

    def test_navigation_below_api_1_3_is_refused(self):
        self.manifest["engines"]["pluginApi"] = "^1.2"
        for action in self.manifest["contributes"]["actions"]:
            action.pop("controls", None)
        self.assertRefused("contributes.navigation", "contributes.views", "needs plugin API 1.3")

    def test_an_input_the_control_cannot_produce_is_refused(self):
        self.manifest["contributes"]["actions"][0]["controls"] = {"key": ["rotate-clockwise"]}
        self.assertRefused("contributes.actions[0].controls.key[0]")

    def test_navigation_icon_outside_assets_is_refused(self):
        shutil.copy(self.dir / "assets/nav-counter.svg", self.dir / "nav-counter.svg")
        self.manifest["contributes"]["navigation"][0]["icon"] = "nav-counter.svg"
        self.assertRefused("contributes.navigation[0].icon", "under assets/")

    def test_navigation_icon_escaping_the_package_is_refused(self):
        # The file exists, so the refusal is the path rule, not a missing file.
        shutil.copy(self.dir / "assets/nav-counter.svg", self.dir.parent / "nav.svg")
        self.manifest["contributes"]["navigation"][0]["icon"] = {"light": "assets/../../nav.svg"}
        self.assertRefused("assets/../../nav.svg", "could leave the package")

    def test_scripted_svg_icon_is_refused(self):
        for name, svg in (("script", SCRIPTED_SVG), ("event handler", HANDLER_SVG)):
            with self.subTest(svg=name):
                (self.dir / "assets/nav-counter.svg").write_text(svg, encoding="utf-8")
                self.assertRefused("assets/nav-counter.svg", "is not a safe SVG")


    def test_api_1_9_permissions_are_refused_in_a_registry_package(self):
        # Plugin API 1.9 (DeckPilot 0.10.0): only the plugins shipped inside
        # the app may declare these; a registry package, even deckpilot.*, may not.
        permissions = {
            "tools": {"kind": "tools", "tools": [{"executable": "git", "subcommands": [{"name": "status"}]}],
                      "justification": "Reads the status of the repositories you bind to keys."},
            "resources": {"kind": "resources", "justification": "Shows the folder bound to a key."},
            "projectContext": {"kind": "projectContext", "justification": "Shows the active project."},
            "hostViews": {"kind": "hostViews", "justification": "Shows the projects view."},
        }
        original = json.loads(json.dumps(self.manifest))
        for kind, permission in permissions.items():
            with self.subTest(kind=kind):
                self.manifest = json.loads(json.dumps(original))
                self.manifest["engines"]["pluginApi"] = "^1.9"
                self.manifest["permissions"].append(permission)
                self.assertRefused(f"the `{kind}` permission is only for DeckPilot's own plugins shipped inside the app")

    def test_api_1_9_permission_below_api_1_9_is_refused(self):
        self.manifest["engines"]["pluginApi"] = "^1.8"
        self.manifest["permissions"].append({"kind": "hostViews", "justification": "Shows the projects view."})
        self.assertRefused("needs plugin API 1.9")

if __name__ == "__main__":
    args = sys.argv[1:]
    while len(args) >= 2 and args[0] in ("--cli", "--module"):
        if args[0] == "--cli":
            CLI = str(Path(args[1]).resolve())
        else:
            MODULE = str(Path(args[1]).resolve())
        args = args[2:]
    for given in (CLI, MODULE):
        if given and not Path(given).is_file():
            sys.exit(f"error: {given} does not exist")
    sys.argv[1:] = args
    unittest.main(verbosity=2)
