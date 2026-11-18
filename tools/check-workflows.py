#!/usr/bin/env python3
"""Repository-specific workflow rules (stdlib only; run with python3 -I).

Complements actionlint and zizmor (tools/lint-workflows.sh) with the rules
of docs/architecture.md and docs/separation.md:

* secrets: only the registry role keys; never DeckPilot's application
  signing secrets;
* environments: only registry-release, registry-timestamp, github-pages;
  never production-release;
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
FORBIDDEN = {
    "pull_request_target": "pull_request_target runs with secrets on untrusted code",
    "workflow_run": "workflow_run can carry untrusted artifacts into a trusted context",
    "actions/cache": "no caches anywhere in this repository",
    "production-release": "DeckPilot's application release environment is never used here",
    "TAURI_SIGNING": "DeckPilot's application updater key is never used here",
    "APPLE_": "DeckPilot's Apple signing secrets are never used here",
}
USES = re.compile(r"^\s*-?\s*uses:\s*([^\s#]+)\s*(#.*)?$")
SECRET = re.compile(r"secrets\.([A-Za-z0-9_]+)")
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
        for name in SECRET.findall(body):
            if name not in ALLOWED_SECRETS:
                problems.append(f"{path}: secret {name} is not a registry role key")
        if re.search(r"^\s*secrets:\s*inherit\s*$", body, re.M):
            problems.append(f"{path}: `secrets: inherit` passes every secret; name the ones needed")
        if re.search(r"^  workflow_call:\s*$", body, re.M):
            declared = set(re.findall(r"^      ([A-Za-z0-9_]+):\s*$", declared_secrets(body), re.M))
            for name in sorted(set(SECRET.findall(body)) - declared):
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
            b = ENV_BLOCK.match(line)
            if b and i + 1 < len(code):
                n = NAME.match(code[i + 1])
                env = n.group(1) if n else "<unnamed>"
            if env is not None and env not in ALLOWED_ENVIRONMENTS:
                problems.append(f"{path}:{i + 1}: environment {env} is not allowed")
    for problem in problems:
        print(f"error: {problem}", file=sys.stderr)
    if problems:
        return 1
    print(f"workflow rules: {len(files)} workflow(s) checked")
    return 0


if __name__ == "__main__":
    sys.exit(main())
