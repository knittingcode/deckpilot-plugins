#!/usr/bin/env python3
"""Tests of tools/check-workflows.py on one-workflow repositories (stdlib
only; run with python3 -I). No network.

    python3 -I tools/tests/test_check_workflows.py
"""

import re
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
CHECK = ROOT / "tools/check-workflows.py"
CHECKOUT = "      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1\n" \
           "        with:\n          persist-credentials: false\n"

passed = failed = 0


def workflow(job_extra="", step="      - run: echo hi\n"):
    return ("name: t\non:\n  push:\npermissions: {}\njobs:\n  j:\n    runs-on: ubuntu-24.04\n"
            f"{job_extra}    steps:\n{CHECKOUT}{step}")


def case(name, text, ok, message="", file="t.yml"):
    global passed, failed
    with tempfile.TemporaryDirectory() as tmp:
        d = Path(tmp) / ".github/workflows"
        d.mkdir(parents=True)
        (d / file).write_text(text)
        p = subprocess.run([sys.executable, "-I", str(CHECK)], cwd=tmp, capture_output=True, text=True)
    out = p.stdout + p.stderr
    if (p.returncode == 0) == ok and re.search(message, out):
        passed += 1
        print(f"ok   {name}")
    else:
        failed += 1
        print(f"FAIL {name}: exit {p.returncode}: {out.strip()}")


def env_step(expr):
    return f"      - env:\n          K: ${{{{ {expr} }}}}\n        run: echo \"$K\"\n"


case("a plain workflow", workflow(), True, "1 workflow")
case("a registry secret", workflow(step=env_step("secrets.REGISTRY_INDEX_KEY")), True)
case("a registry secret, index syntax", workflow(step=env_step("secrets['REGISTRY_INDEX_KEY']")), True)
case("another secret", workflow(step=env_step("secrets.OTHER")), False, "secret OTHER is not a registry role key")
case("another secret, index syntax", workflow(step=env_step("secrets['OTHER']")), False, "secret OTHER")
case("another secret, double quotes", workflow(step=env_step('secrets["OTHER"]')), False, "secret OTHER")
case("toJSON(secrets)", workflow(step=env_step("toJSON(secrets)")), False, "secrets context")
case("a computed index", workflow(step=env_step("secrets[format('REGISTRY_{0}', 'X')]")), False, "secrets context")
case("prose about secrets is not an expression", workflow(step="      - run: echo \"the secrets are not set\"\n"), True)
case("an environment by expression", workflow("    environment: ${{ inputs.env }}\n"), False, "an expression")
case("an environment block named by expression",
     workflow("    environment:\n      name: ${{ github.ref_name }}\n"), False, "an expression")
case("an unknown environment", workflow("    environment: staging\n"), False, "environment staging is not allowed")
case("the publisher key outside official-release.yml",
     workflow(step=env_step("secrets.DECKPILOT_PUBLISHER_KEY")), False, "belongs to official-release.yml only")
case("the publisher key, index syntax, outside official-release.yml",
     workflow(step=env_step("secrets['DECKPILOT_PUBLISHER_KEY']")), False, "belongs to official-release.yml only")
case("official-release outside official-release.yml", workflow("    environment: official-release\n"), False, "not allowed")
case("the publisher key in official-release.yml",
     workflow("    environment: official-release\n", env_step("secrets.DECKPILOT_PUBLISHER_KEY")), True, file="official-release.yml")
case("a registry secret in official-release.yml",
     workflow(step=env_step("secrets.REGISTRY_INDEX_KEY")), False, "not the publisher key", file="official-release.yml")
case("a registry environment in official-release.yml",
     workflow("    environment: registry-release\n"), False, "not allowed", file="official-release.yml")
case("an if: condition without ${{ }} using the whole context",
     workflow(step="      - if: toJSON(secrets) != '{}'\n        run: echo hi\n"), False, "secrets context")
case("an if: condition naming a registry secret", workflow(step="      - if: secrets.REGISTRY_INDEX_KEY != ''\n        run: echo hi\n"), True)
case("a multi-line expression using the whole context",
     workflow(step="      - env:\n          K: ${{\n            toJSON(secrets)\n          }}\n        run: echo \"$K\"\n"), False, "secrets context")
case("a signing job that compiles the CLI",
     workflow("    environment: registry-release\n", "      - run: tools/build-cli.sh\n"), False, "nothing compiles where a signing key is")
case("a signing job that runs cargo in a block",
     workflow("    environment: official-release\n", "      - run: |\n          echo start\n          cargo build --release\n"),
     False, "nothing compiles", file="official-release.yml")
case("a signing job that runs an official.sh build step",
     workflow("    environment: official-release\n", "      - run: tools/official.sh release-build deckpilot.git 1.0.0\n"),
     False, "nothing compiles", file="official-release.yml")
case("a signing job that installs the cli job's binaries and signs",
     workflow("    environment: official-release\n",
              "      - run: tools/cli-artifact.sh install \"$D\" \"$A\" \"$B\"\n      - run: tools/official.sh release-sign x d 1 s 2\n"),
     True, file="official-release.yml")
case("an unprivileged job may compile", workflow(step="      - run: tools/build-cli.sh\n"), True)
case("pull_request_target", workflow().replace("  push:", "  pull_request_target:"), False, "pull_request_target")

print(f"\n{passed} passed, {failed} failed")
sys.exit(1 if failed else 0)
