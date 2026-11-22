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
    """`file=None`: the repository's own .github/workflows."""
    global passed, failed
    with tempfile.TemporaryDirectory() as tmp:
        if file is not None:
            d = Path(tmp) / ".github/workflows"
            d.mkdir(parents=True)
            (d / file).write_text(text)
        p = subprocess.run([sys.executable, "-I", str(CHECK)], cwd=tmp if file else ROOT, capture_output=True, text=True)
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

# The storefront switch (docs/storefront.md).
SF_ENV = "      - env:\n          REGISTRY_STOREFRONT_ENABLED: ${{ vars.REGISTRY_STOREFRONT_ENABLED }}\n        run: tools/publish-site.sh index\n"
case("storefront: the repository variable in publish.yml", workflow(step=SF_ENV), True, "1 workflow", file="publish.yml")
case("storefront: the variable elsewhere (the timestamp refresh keeps any storefront)", workflow(step=SF_ENV), False,
     "belongs to publish.yml alone", file="_site.yml")
case("storefront: a literal true", workflow(step="      - env:\n          REGISTRY_STOREFRONT_ENABLED: true\n        run: x\n"),
     False, "only as `REGISTRY_STOREFRONT_ENABLED", file="publish.yml")
case("storefront: from an input", workflow(step="      - env:\n          REGISTRY_STOREFRONT_ENABLED: ${{ inputs.sf }}\n        run: x\n"),
     False, "only as `REGISTRY_STOREFRONT_ENABLED", file="publish.yml")
case("storefront: set in a run line", workflow(step="      - run: REGISTRY_STOREFRONT_ENABLED=true tools/publish-site.sh index\n"),
     False, "only as `REGISTRY_STOREFRONT_ENABLED", file="publish.yml")
case("storefront: a workflow that signs a storefront itself",
     workflow(step="      - run: .work/bin/deckpilot-registry storefront sign --key k\n"), False, "only tools/publish-site.sh")
case("storefront: prose in a comment is not a use", workflow(step="      # storefront sign, REGISTRY_STOREFRONT_ENABLED\n      - run: echo\n"),
     True, "1 workflow", file="_site.yml")
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
     workflow(step=env_step("secrets.DECKPILOT_PUBLISHER_KEY_A")), False, "belongs to official-release.yml and official-test-sign.yml only")
case("the publisher key, index syntax, outside official-release.yml",
     workflow(step=env_step("secrets['DECKPILOT_PUBLISHER_KEY_B']")), False, "belongs to official-release.yml and official-test-sign.yml only")
case("the publisher key in lower case outside official-release.yml (GitHub's names are case-insensitive)",
     workflow(step=env_step("secrets.deckpilot_publisher_key_a_password")), False, "belongs to official-release.yml and official-test-sign.yml only")
# The retired single key; its name is assembled so nothing references it.
RETIRED = "DECKPILOT_PUBLISHER_" + "KEY"
case("the retired single publisher key outside official-release.yml",
     workflow(step=env_step(f"secrets.{RETIRED}")), False, "is not one of the two publisher keys")
case("official-release outside official-release.yml", workflow("    environment: official-release\n"), False, "not allowed")
case("a registry environment in official-release.yml",
     workflow("    environment: registry-release\n"), False, "not allowed", file="official-release.yml")
case("an if: condition without ${{ }} using the whole context",
     workflow(step="      - if: toJSON(secrets) != '{}'\n        run: echo hi\n"), False, "secrets context")
case("an if: condition naming a registry secret", workflow(step="      - if: secrets.REGISTRY_INDEX_KEY != ''\n        run: echo hi\n"), True)
case("a multi-line expression using the whole context",
     workflow(step="      - env:\n          K: ${{\n            toJSON(secrets)\n          }}\n        run: echo \"$K\"\n"), False, "secrets context")
case("a signing job that compiles the CLI",
     workflow("    environment: registry-release\n", "      - run: tools/build-cli.sh\n"), False, "nothing compiles where a signing key is")
case("an unprivileged job may compile", workflow(step="      - run: tools/build-cli.sh\n"), True)
case("pull_request_target", workflow().replace("  push:", "  pull_request_target:"), False, "pull_request_target")


# official-release.yml: two signing jobs, one publisher key each.
IF = ("github.repository == 'knittingcode/deckpilot-plugins' && github.ref_type == 'tag' && "
      "startsWith(github.ref, 'refs/tags/deckpilot.') && contains(github.ref, '--v')")


def sign_job(name, key, extra="", steps=None, env="    environment: official-release\n", cond=f"    if: {IF}\n",
             needs="    needs: [resolve, gate, compare, cli]\n"):
    if steps is None:
        steps = (f"      - run: tools/cli-artifact.sh install \"$D\" \"$A\" \"$B\"\n"
                 f"      - name: Sign\n        env:\n"
                 f"          DECKPILOT_PUBLISHER_KEY_{key}: ${{{{ secrets.DECKPILOT_PUBLISHER_KEY_{key} }}}}\n"
                 f"          DECKPILOT_PUBLISHER_KEY_{key}_PASSWORD: ${{{{ secrets.DECKPILOT_PUBLISHER_KEY_{key}_PASSWORD }}}}\n"
                 f"        run: tools/official.sh release-sign {key} \"$ID\" d \"$V\" \"$S\" \"$N\" out\n")
    return (f"  {name}:\n{needs}{cond}    runs-on: ubuntu-24.04\n{env}{extra}    steps:\n{CHECKOUT}{steps}")


def plain_job(name, needs, extra="", step="      - run: echo hi\n"):
    return f"  {name}:\n    needs: [{needs}]\n    runs-on: ubuntu-24.04\n{extra}    steps:\n{CHECKOUT}{step}"


ON = ("on:\n  push:\n    tags:\n      - \"deckpilot.git--v*\"\n      - \"deckpilot.jira--v*\"\n"
      "  workflow_dispatch:\n")


def release_workflow(on=ON, a=None, b=None, verify=None, release=None, others="", head=""):
    a = sign_job("sign-a", "A") if a is None else a
    b = sign_job("sign-b", "B") if b is None else b
    verify = plain_job("verify", "resolve, compare, cli, sign-a, sign-b",
                       step="      - run: tools/official.sh release-verify x d 1 s 2 \"$KA\" \"$KB\"\n") if verify is None else verify
    release = plain_job("release", "resolve, compare, verify") if release is None else release
    return (f"name: Official release\n{on}permissions: {{}}\n{head}jobs:\n"
            f"  gate:\n    runs-on: ubuntu-24.04\n    steps:\n{CHECKOUT}      - run: tools/check-environment.sh official-release\n"
            f"{others}{a}{b}{verify}{release}")


def official(name, text, ok, message=""):
    case(name, text, ok, message, file="official-release.yml")


KEY_A = "${{ secrets.DECKPILOT_PUBLISHER_KEY_A }}"
official("two signing jobs, one key each, a keyless verify, release after verify", release_workflow(), True, "1 workflow")
case("the repository's own workflows", None, True, "workflow rules", file=None)
official("the retired single key in official-release.yml",
         release_workflow(a=sign_job("sign-a", "A", steps=f"      - env:\n          {RETIRED}: ${{{{ secrets.{RETIRED} }}}}\n        run: x\n")),
         False, f"secret {RETIRED} is not one of the two publisher keys")
official("a third publisher key",
         release_workflow(others=sign_job("sign-c", "C")), False, "DECKPILOT_PUBLISHER_KEY_C is not one of the two publisher keys")
official("a registry secret in official-release.yml",
         release_workflow(others=plain_job("other", "gate", step=env_step("secrets.REGISTRY_INDEX_KEY"))), False, "not a publisher key")
official("key A in sign-b", release_workflow(b=sign_job("sign-b", "A")), False, "job sign-b holds DECKPILOT_PUBLISHER_KEY_A")
official("key B's password in sign-a, index syntax and lower case",
         release_workflow(a=sign_job("sign-a", "A", steps=sign_job("x", "A").split("    steps:\n" + CHECKOUT)[1]
                                     + "      - env:\n          K: ${{ secrets['deckpilot_publisher_key_b_password'] }}\n        run: x\n")),
         False, "sign-a holds DECKPILOT_PUBLISHER_KEY_B_PASSWORD")
both = ("      - name: Sign\n        env:\n          DECKPILOT_PUBLISHER_KEY_A: ${{ secrets.DECKPILOT_PUBLISHER_KEY_A }}\n"
        "          DECKPILOT_PUBLISHER_KEY_B: ${{ secrets.DECKPILOT_PUBLISHER_KEY_B }}\n        run: x\n")
official("one job with both keys", release_workflow(a=sign_job("sign-a", "A", steps=both)), False, "holds both publisher keys")
official("a key in the verify job",
         release_workflow(verify=plain_job("verify", "sign-a, sign-b", step=env_step("secrets.DECKPILOT_PUBLISHER_KEY_A"))),
         False, "job verify holds DECKPILOT_PUBLISHER_KEY_A: the publisher keys belong to sign-a and sign-b only")
official("a key in a third job",
         release_workflow(others=sign_job("sign-c", "A")), False, "job sign-c holds .*belong to sign-a and sign-b only")
official("a key job without the environment (no tag policy, no authorization)",
         release_workflow(a=sign_job("sign-a", "A", env="")), False, "without naming environment official-release")
official("a key job in another environment",
         release_workflow(a=sign_job("sign-a", "A", env="    environment: registry-release\n")), False, "environment registry-release is not allowed")
official("a key job without its condition (a run on a branch)",
         release_workflow(a=sign_job("sign-a", "A", cond="")), False, "without the job condition")
official("a key job whose condition does not name this repository (a fork)",
         release_workflow(b=sign_job("sign-b", "B", cond="    if: github.ref_type == 'tag'\n")), False, "without the job condition")
official("a key job that does not need the gate",
         release_workflow(a=sign_job("sign-a", "A", needs="    needs: [resolve, compare, cli]\n")), False, "without needing `gate`")
official("a pull_request trigger", release_workflow(on=ON + "  pull_request:\n"), False, "trigger `pull_request`")
official("a pull_request_target trigger", release_workflow(on=ON + "  pull_request_target:\n"), False, "pull_request_target")
official("a workflow_run trigger", release_workflow(on=ON + "  workflow_run:\n    workflows: [x]\n"), False, "trigger `workflow_run`")
official("a workflow_call trigger (another workflow's inputs)", release_workflow(on=ON + "  workflow_call:\n"), False, "trigger `workflow_call`")
official("a schedule trigger", release_workflow(on=ON + "  schedule:\n    - cron: '0 0 * * *'\n"), False, "trigger `schedule`")
official("an inline trigger list", release_workflow(on="on: [push, pull_request]\n"), False, "lists its triggers as a block")
official("a push of branches", release_workflow(on=ON.replace("    tags:", "    branches: [main]\n    tags:")), False, "push filters")
official("a tag filter for every release tag", release_workflow(on=ON.replace("deckpilot.git--v*", "*--v*")), False, "is not a deckpilot")
official("a tag filter without --v", release_workflow(on=ON.replace("deckpilot.git--v*", "deckpilot.*")), False, "is not a deckpilot")
official("workflow_dispatch with an input (input chosen at run time)",
         release_workflow(on=ON + "    inputs:\n      ref:\n        type: string\n"), False, "takes no inputs")
official("an event payload used by a job",
         release_workflow(others=plain_job("other", "gate", step="      - env:\n          R: ${{ github.event.inputs.ref }}\n        run: x\n")),
         False, "github.event")
official("a key job that compiles the CLI",
         release_workflow(a=sign_job("sign-a", "A", steps="      - run: tools/build-cli.sh\n" + sign_job("x", "A").split("    steps:\n" + CHECKOUT)[1])),
         False, "nothing compiles")
official("a key job that runs cargo in a block",
         release_workflow(b=sign_job("sign-b", "B", steps="      - run: |\n          echo start\n          cargo build --release\n" + sign_job("x", "B").split("    steps:\n" + CHECKOUT)[1])),
         False, "nothing compiles")
official("a key job that runs an official.sh build step",
         release_workflow(a=sign_job("sign-a", "A", steps="      - run: tools/official.sh release-build deckpilot.git 1.0.0\n" + sign_job("x", "A").split("    steps:\n" + CHECKOUT)[1])),
         False, "nothing compiles")
official("a key job that verifies (official.sh other than release-sign)",
         release_workflow(a=sign_job("sign-a", "A", steps="      - run: tools/official.sh fetch-deckpilot\n" + sign_job("x", "A").split("    steps:\n" + CHECKOUT)[1])),
         False, "nothing compiles")
official("a key in the job's env (every step gets it)",
         release_workflow(a=sign_job("sign-a", "A", extra=f"    env:\n      DECKPILOT_PUBLISHER_KEY_A: {KEY_A}\n", steps="      - run: x\n")),
         False, "outside a step")
official("a key passed to an action",
         release_workflow(a=sign_job("sign-a", "A", steps="      - uses: ./.github/actions/sign\n        with:\n"
                                     f"          DECKPILOT_PUBLISHER_KEY_A: {KEY_A}\n")), False, "passed to an action")
official("a key in the run script itself",
         release_workflow(a=sign_job("sign-a", "A", steps=f"      - env:\n          X: 1\n        run: echo {KEY_A}\n")),
         False, "only the step's env variable of the same name")
official("a key under another env name",
         release_workflow(a=sign_job("sign-a", "A", steps=f"      - env:\n          KEY: {KEY_A}\n        run: x\n")),
         False, "only the step's env variable of the same name")
official("a key in two steps",
         release_workflow(a=sign_job("sign-a", "A", steps=2 * f"      - env:\n          DECKPILOT_PUBLISHER_KEY_A: {KEY_A}\n        run: x\n")),
         False, "2 steps get a publisher key")
official("a key in the workflow's env",
         release_workflow(head=f"env:\n  DECKPILOT_PUBLISHER_KEY_A: {KEY_A}\n"), False, "a secret outside the jobs")
official("no sign-b (one key)", release_workflow(b=""), False, "no job sign-b")
official("verify does not wait for sign-b (one key missing: release anyway)",
         release_workflow(verify=plain_job("verify", "sign-a")), False, "verify does not need both")
official("no verify job", release_workflow(verify="", release=plain_job("release", "sign-a, sign-b")), False, "no job `verify`")
official("release does not need verify", release_workflow(release=plain_job("release", "sign-a, sign-b")), False, "release does not need verify")
official("verify in the environment",
         release_workflow(verify=plain_job("verify", "sign-a, sign-b", extra="    environment: official-release\n")),
         False, "job verify names environment official-release")
official("release in the environment",
         release_workflow(release=plain_job("release", "verify", extra="    environment: official-release\n")),
         False, "job release names environment official-release; only sign-a and sign-b do")

official("a signing step that runs another command",
         release_workflow(a=sign_job("sign-a", "A", steps=sign_job("x", "A").split("    steps:\n" + CHECKOUT)[1]
                                     .replace("tools/official.sh release-sign A", "tools/sign-anything.sh A"))),
         False, "does not run tools/official.sh release-sign")
official("a verify job that does not run release-verify",
         release_workflow(verify=plain_job("verify", "resolve, compare, cli, sign-a, sign-b")), False,
         "job verify does not run tools/official.sh release-verify")
official("no gate job checking the environment",
         release_workflow().replace("tools/check-environment.sh official-release", "echo skipped"), False,
         "no job `gate` running tools/check-environment.sh official-release")
official("the key test's signing command in official-release.yml",
         release_workflow(a=sign_job("sign-a", "A", steps=sign_job("x", "A").split("    steps:\n" + CHECKOUT)[1]
                                     .replace("release-sign A \"$ID\" d \"$V\" \"$S\" \"$N\" out", "test-sign A 1 out"))),
         False, "nothing compiles|does not run tools/official.sh release-sign")


# official-test-sign.yml: the key test. The same two jobs and rules, on a
# key test tag, by workflow_dispatch only, and nothing released.
TEST_IF = ("github.repository == 'knittingcode/deckpilot-plugins' && github.event_name == 'workflow_dispatch' && "
           "github.ref_type == 'tag' && startsWith(github.ref, 'refs/tags/deckpilot.publisher-test--v')")
UPLOAD = ("      - uses: actions/upload-artifact@cf430e030ddbb5b0abf93d22962f4752f3646cd9 # v7.0.2\n"
          "        with:\n          name: s\n          path: out\n          retention-days: 1\n")


def test_sign_job(name, key, cond=f"    if: {TEST_IF}\n", env="    environment: official-release\n",
                  needs="    needs: [resolve, gate, cli]\n", upload=UPLOAD, run=None):
    run = f"tools/official.sh test-sign {key} \"$RUN\" out" if run is None else run
    steps = (f"      - run: tools/cli-artifact.sh install \"$D\" \"$A\" \"$B\"\n"
             f"      - name: Sign\n        env:\n"
             f"          DECKPILOT_PUBLISHER_KEY_{key}: ${{{{ secrets.DECKPILOT_PUBLISHER_KEY_{key} }}}}\n"
             f"          DECKPILOT_PUBLISHER_KEY_{key}_PASSWORD: ${{{{ secrets.DECKPILOT_PUBLISHER_KEY_{key}_PASSWORD }}}}\n"
             f"        run: {run}\n{upload}")
    return sign_job(name, key, steps=steps, env=env, cond=cond, needs=needs)


TEST_ON = "on:\n  workflow_dispatch:\n"


def test_workflow(on=TEST_ON, a=None, b=None, verify=None, others=""):
    a = test_sign_job("sign-a", "A") if a is None else a
    b = test_sign_job("sign-b", "B") if b is None else b
    verify = plain_job("verify", "resolve, cli, sign-a, sign-b",
                       step="      - run: tools/official.sh test-verify \"$RUN\" d \"$KA\" \"$KB\"\n") if verify is None else verify
    return (f"name: Official key test\n{on}permissions: {{}}\njobs:\n"
            f"  gate:\n    runs-on: ubuntu-24.04\n    permissions:\n      contents: read\n    steps:\n{CHECKOUT}"
            f"      - run: tools/check-environment.sh official-release\n{others}{a}{b}{verify}")


def key_test(name, text, ok, message=""):
    case(name, text, ok, message, file="official-test-sign.yml")


key_test("the key test: two signing jobs, one key each, a keyless verify", test_workflow(), True, "1 workflow")
key_test("the key test on a push", test_workflow(on=TEST_ON + "  push:\n    tags: [\"deckpilot.publisher-test--v*\"]\n"),
         False, "trigger `push`")
key_test("the key test on a pull request", test_workflow(on=TEST_ON + "  pull_request:\n"), False, "trigger `pull_request`")
key_test("the key test without workflow_dispatch", test_workflow(on="on:\n  schedule:\n    - cron: '0 0 * * *'\n"),
         False, "trigger `schedule`")
key_test("the key test with a dispatch input",
         test_workflow(on=TEST_ON + "    inputs:\n      run:\n        type: string\n"), False, "takes no inputs")
key_test("the key test's key job with official-release's condition (any release tag)",
         test_workflow(a=test_sign_job("sign-a", "A", cond=f"    if: {IF}\n")), False, "without the job condition")
key_test("the key test's key job without a condition", test_workflow(b=test_sign_job("sign-b", "B", cond="")), False,
         "without the job condition")
key_test("the key test's key job without the environment", test_workflow(a=test_sign_job("sign-a", "A", env="")), False,
         "without naming environment official-release")
key_test("the key test's key job without the gate",
         test_workflow(a=test_sign_job("sign-a", "A", needs="    needs: [resolve, cli]\n")), False, "without needing `gate`")
key_test("the key test without a gate job",
         test_workflow().replace("tools/check-environment.sh official-release", "echo skipped"), False, "no job `gate`")
key_test("the key test signing a release",
         test_workflow(a=test_sign_job("sign-a", "A", run="tools/official.sh release-sign A \"$ID\" d \"$V\" \"$S\" \"$N\" out")),
         False, "the key test signs only the test message")
key_test("the key test running release sign directly",
         test_workflow(b=test_sign_job("sign-b", "B", run="\"$CLI\" release sign --package p --key-env DECKPILOT_PUBLISHER_KEY_B")),
         False, "the key test signs only the test message")
key_test("the key test compiling where a key is",
         test_workflow(a=test_sign_job("sign-a", "A", run="tools/build-cli.sh && tools/official.sh test-sign A 1 out")),
         False, "nothing compiles")
key_test("the key test with both keys in one job",
         test_workflow(a=sign_job("sign-a", "A", cond=f"    if: {TEST_IF}\n", needs="    needs: [resolve, gate, cli]\n",
                                  steps=both)), False, "holds both publisher keys")
key_test("the key test with a release job",
         test_workflow(others=plain_job("release", "verify")), False, "a job `release`: the key test releases nothing")
key_test("the key test uploading to a release",
         test_workflow(others=plain_job("upload", "verify", step="      - run: gh release upload t f\n")), False,
         "the key test signs only the test message")
key_test("the key test with a write permission",
         test_workflow(others=plain_job("other", "verify", extra="    permissions:\n      contents: write\n")), False,
         "writes nothing")
key_test("the key test keeping a signature for a week",
         test_workflow(a=test_sign_job("sign-a", "A", upload=UPLOAD.replace("retention-days: 1", "retention-days: 7"))),
         False, "kept longer than one day")
key_test("the key test without a verify job", test_workflow(verify=""), False, "no job `verify`")
key_test("the key test's verify not waiting for sign-b",
         test_workflow(verify=plain_job("verify", "sign-a", step="      - run: tools/official.sh test-verify 1 d a b\n")),
         False, "verify does not need both")
key_test("the key test's verify with a key",
         test_workflow(verify=plain_job("verify", "sign-a, sign-b", step=env_step("secrets.DECKPILOT_PUBLISHER_KEY_A"))),
         False, "belong to sign-a and sign-b only")
key_test("the key test's verify in the environment",
         test_workflow(verify=plain_job("verify", "sign-a, sign-b", extra="    environment: official-release\n",
                                        step="      - run: tools/official.sh test-verify 1 d a b\n")),
         False, "job verify names environment official-release")
key_test("the key test's verify not running test-verify",
         test_workflow(verify=plain_job("verify", "sign-a, sign-b")), False, "does not run tools/official.sh test-verify")
key_test("a registry secret in the key test",
         test_workflow(others=plain_job("other", "gate", step=env_step("secrets.REGISTRY_INDEX_KEY"))), False, "not a publisher key")
key_test("a registry environment in the key test",
         test_workflow(others=plain_job("other", "gate", extra="    environment: registry-release\n")), False, "not allowed")

print(f"\n{passed} passed, {failed} failed")
sys.exit(1 if failed else 0)
