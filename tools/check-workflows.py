#!/usr/bin/env python3
"""Repository-specific workflow rules (stdlib only; run with python3 -I).

Complements actionlint and zizmor (tools/lint-workflows.sh) with the rules
of docs/architecture.md and docs/separation.md:

* secrets: only the registry role keys, and the first-party publisher key
  in official-release.yml alone; never DeckPilot's application signing
  secrets;
* environments: only registry-release, registry-timestamp, github-pages,
  and official-release in official-release.yml alone; never
  production-release;
* no pull_request_target or workflow_run triggers, no actions/cache;
* every remote action pinned to a full commit SHA with a version comment;
* every actions/checkout sets persist-credentials: false;
* top-level `permissions: {}` in every workflow;
* a reusable (workflow_call) workflow declares every secret it reads under
  `on.workflow_call.secrets`, or GitHub hands its jobs an empty value (also
  in a job that names an environment), and no caller uses `secrets: inherit`.
"""

import re
import sys
from pathlib import Path

ALLOWED_SECRETS = {
    "REGISTRY_INDEX_KEY",
    "REGISTRY_INDEX_KEY_PASSWORD",
    "REGISTRY_TIMESTAMP_KEY",
    "REGISTRY_TIMESTAMP_KEY_PASSWORD",
}
ALLOWED_ENVIRONMENTS = {"registry-release", "registry-timestamp", "github-pages"}
# The first-party publisher key (DeckPilot docs/marketplace/
# publisher-signature.md) signs official release artifacts only: its secrets
# and its environment belong to official-release.yml and nothing else, and
# that workflow reads no registry secret.
PUBLISHER_WORKFLOW = "official-release.yml"
PUBLISHER_SECRETS = {"DECKPILOT_PUBLISHER_KEY", "DECKPILOT_PUBLISHER_KEY_PASSWORD"}
PUBLISHER_ENVIRONMENT = "official-release"
FORBIDDEN = {
    "pull_request_target": "pull_request_target runs with secrets on untrusted code",
    "workflow_run": "workflow_run can carry untrusted artifacts into a trusted context",
    "actions/cache": "no caches anywhere in this repository",
    "production-release": "DeckPilot's application release environment is never used here",
    "TAURI_SIGNING": "DeckPilot's application updater key is never used here",
    "APPLE_": "DeckPilot's Apple signing secrets are never used here",
}
USES = re.compile(r"^\s*-?\s*uses:\s*([^\s#]+)\s*(#.*)?$")
# A secret in either expression syntax: secrets.X, secrets['X'], secrets["X"].
SECRET = re.compile(r"""secrets\s*(?:\.\s*([A-Za-z0-9_]+)|\[\s*['"]([A-Za-z0-9_]+)['"]\s*\])""")
# Any other use of the secrets context: a computed index (secrets[x]), the
# whole context (toJSON(secrets), secrets in an expression), so no rule here
# can be passed by naming a secret indirectly.
SECRETS_CONTEXT = re.compile(r"\bsecrets\b(?!\s*(?:\.\s*[A-Za-z0-9_]+|\[\s*['\"][A-Za-z0-9_]+['\"]\s*\]))")
# An expression, also across lines.
EXPRESSION = re.compile(r"\$\{\{(.*?)\}\}", re.S)
# `if:` is an expression even without ${{ }}; its value may continue on
# more-indented lines (a block scalar or a folded condition).
IF_CONDITION = re.compile(r"^(\s*)(?:-\s+)?if:\s*(.*(?:\n\1\s+.*)*)", re.M)
# The environments whose jobs hold signing keys: nothing compiles there.
SIGNING_ENVIRONMENTS = {"registry-release", "registry-timestamp", "official-release"}
COMPILES = re.compile(r"\b(build-cli\.sh|build-plugin\.sh|cargo|rustup|rustc)\b|official\.sh\s+(?!release-sign|release-verify|release-compare|fetch-deckpilot)")
JOB = re.compile(r"^  ([A-Za-z0-9_-]+):\s*$", re.M)
ENV_INLINE = re.compile(r"^\s*environment:\s*([A-Za-z0-9_.-]+)\s*$")
ENV_BLOCK = re.compile(r"^(\s*)environment:\s*$")
NAME = re.compile(r"^\s*name:\s*([A-Za-z0-9_.-]+)\s*$")


def declared_secrets(body):
    """The block under `on.workflow_call.secrets` (empty when there is none)."""
    m = re.search(r"^    secrets:\s*\n((?:      .*\n|\s*\n)*)", body.split("\npermissions:", 1)[0] + "\n", re.M)
    return m.group(1) if m else ""


def main():
    problems = []
    files = sorted(Path(".github/workflows").glob("*.yml")) + sorted(Path(".github/workflows").glob("*.yaml"))
    for path in files:
        text = path.read_text(encoding="utf-8")
        lines = text.splitlines()
        code = [line.split(" #", 1)[0] if not line.lstrip().startswith("#") else "" for line in lines]
        body = "\n".join(code)
        for word, why in FORBIDDEN.items():
            if word in body:
                problems.append(f"{path}: `{word}`: {why}")
        if not re.search(r"^permissions:\s*\{\}\s*$", text, re.M):
            problems.append(f"{path}: top-level `permissions: {{}}` is required")
        publisher = path.name == PUBLISHER_WORKFLOW
        names = [a or b for a, b in SECRET.findall(body)]
        expressions = EXPRESSION.findall(body) + [m.group(2) for m in IF_CONDITION.finditer(body)]
        if any(SECRETS_CONTEXT.search(e) for e in expressions):
            problems.append(f"{path}: the secrets context is used other than as secrets.NAME "
                            "or secrets['NAME'] (toJSON(secrets), a computed index, also in `if:` "
                            "conditions and multi-line expressions): name each secret")
        jobs = body.split("\njobs:\n", 1)[1] if "\njobs:\n" in body else ""
        starts = [m for m in JOB.finditer(jobs)]
        for k, m in enumerate(starts):
            chunk = jobs[m.end(): starts[k + 1].start() if k + 1 < len(starts) else len(jobs)]
            env = re.search(r"^    environment:\s*([A-Za-z0-9_.-]+)\s*$", chunk, re.M) or \
                re.search(r"^    environment:\s*\n\s+name:\s*([A-Za-z0-9_.-]+)", chunk, re.M)
            if env and env.group(1) in SIGNING_ENVIRONMENTS:
                for run in re.findall(r"run:\s*(\|[^\n]*\n(?:\s{10,}.*\n?)*|.*)", chunk):
                    hit = COMPILES.search(run)
                    if hit:
                        problems.append(f"{path}: job {m.group(1)} (environment {env.group(1)}) runs "
                                        f"`{hit.group(0).strip()}`: nothing compiles where a signing key is; "
                                        "build in an unprivileged job and install the result (tools/cli-artifact.sh)")
        for name in names:
            if publisher:
                if name not in PUBLISHER_SECRETS:
                    problems.append(f"{path}: secret {name} is not the publisher key")
            elif name in PUBLISHER_SECRETS:
                problems.append(f"{path}: the publisher key belongs to {PUBLISHER_WORKFLOW} only")
            elif name not in ALLOWED_SECRETS:
                problems.append(f"{path}: secret {name} is not a registry role key")
        if re.search(r"^\s*secrets:\s*inherit\s*$", body, re.M):
            problems.append(f"{path}: `secrets: inherit` passes every secret; name the ones needed")
        if re.search(r"^  workflow_call:\s*$", body, re.M):
            declared = set(re.findall(r"^      ([A-Za-z0-9_]+):\s*$", declared_secrets(body), re.M))
            for name in sorted(set(names) - declared):
                problems.append(f"{path}: secret {name} is read but not declared under on.workflow_call.secrets")
        for i, line in enumerate(code):
            m = USES.match(lines[i])
            if m and not m.group(1).startswith("./"):
                ref = m.group(1)
                if not re.fullmatch(r"[\w.-]+/[\w./-]+@[0-9a-f]{40}", ref):
                    problems.append(f"{path}:{i + 1}: {ref} is not pinned to a full commit SHA")
                elif not (m.group(2) or "").startswith("# v"):
                    problems.append(f"{path}:{i + 1}: {ref} needs a `# v<version>` comment")
                if ref.startswith("actions/checkout@"):
                    window = "\n".join(code[i + 1 : i + 8])
                    if "persist-credentials: false" not in window:
                        problems.append(f"{path}:{i + 1}: actions/checkout without persist-credentials: false")
            m = ENV_INLINE.match(line)
            env = m.group(1) if m else None
            if re.match(r"^\s*environment:", line) and "${{" in line:
                env = "<an expression>"
            b = ENV_BLOCK.match(line)
            if b and i + 1 < len(code):
                n = NAME.match(code[i + 1])
                env = n.group(1) if n else "<unnamed>"
                if "${{" in code[i + 1]:
                    env = "<an expression>"
            allowed = {PUBLISHER_ENVIRONMENT} if publisher else ALLOWED_ENVIRONMENTS
            if env is not None and env not in allowed:
                problems.append(f"{path}:{i + 1}: environment {env} is not allowed")
    for problem in problems:
        print(f"error: {problem}", file=sys.stderr)
    if problems:
        return 1
    print(f"workflow rules: {len(files)} workflow(s) checked")
    return 0


if __name__ == "__main__":
    sys.exit(main())
