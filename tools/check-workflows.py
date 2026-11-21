#!/usr/bin/env python3
"""Repository-specific workflow rules (stdlib only; run with python3 -I).

Complements actionlint and zizmor (tools/lint-workflows.sh) with the rules
of docs/architecture.md and docs/separation.md:

* secrets: only the registry role keys, and the two first-party publisher
  keys in official-release.yml alone; never DeckPilot's application signing
  secrets (secret names compare case-insensitively, as GitHub does);
* the publisher keys (official-release.yml): DECKPILOT_PUBLISHER_KEY_A and
  its password only in job sign-a, _B only in sign-b, no job with both, no
  other DECKPILOT_PUBLISHER_KEY* secret (the retired single key included)
  anywhere. A job holding a key names
  environment official-release, runs only on a deckpilot.*--v* tag of this
  repository (its exact `if:`, after the gate), gives the key to one `run`
  step's env, and compiles nothing; the workflow has no trigger but
  push of deckpilot.*--v* tags and an input-less workflow_dispatch; a
  `verify` job without secrets needs both signing jobs and `release` needs
  `verify` (one key missing: no release);
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
# The first-party publisher keys (DeckPilot docs/marketplace/
# publisher-signature.md) sign official release artifacts only, threshold 2:
# their secrets and their environment belong to official-release.yml and
# nothing else, each key to its own signing job, and that workflow reads no
# registry secret.
PUBLISHER_WORKFLOW = "official-release.yml"
KEY_JOBS = {
    "sign-a": {"DECKPILOT_PUBLISHER_KEY_A", "DECKPILOT_PUBLISHER_KEY_A_PASSWORD"},
    "sign-b": {"DECKPILOT_PUBLISHER_KEY_B", "DECKPILOT_PUBLISHER_KEY_B_PASSWORD"},
}
PUBLISHER_SECRETS = set().union(*KEY_JOBS.values())
# Every other secret of this prefix (the retired single key, a third key).
PUBLISHER_PREFIX = "DECKPILOT_PUBLISHER_KEY"
PUBLISHER_ENVIRONMENT = "official-release"
# The job-level condition of a job holding a publisher key: this
# repository (never a fork's run), a tag, an official release tag.
KEY_JOB_IF = ("github.repository == 'knittingcode/deckpilot-plugins' && github.ref_type == 'tag' && "
              "startsWith(github.ref, 'refs/tags/deckpilot.') && contains(github.ref, '--v')")
# The triggers official-release.yml may have, and its tag filter.
PUBLISHER_TRIGGERS = {"push", "workflow_dispatch"}
RELEASE_TAG = re.compile(r"deckpilot\.[a-z0-9-]+--v\*")
# The one form a publisher key takes in its step: an env variable of its name.
KEY_ENV_LINE = re.compile(r"          ([A-Z0-9_]+):\s*\$\{\{\s*secrets\.([A-Za-z0-9_]+)\s*\}\}\s*")
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
COMPILES = re.compile(r"\b(build-cli\.sh|build-plugin\.sh|cargo|rustup|rustc)\b|official\.sh\s+(?!release-sign\b)")
JOB = re.compile(r"^  ([A-Za-z0-9_-]+):\s*$", re.M)
ENV_INLINE = re.compile(r"^\s*environment:\s*([A-Za-z0-9_.-]+)\s*$")
ENV_BLOCK = re.compile(r"^(\s*)environment:\s*$")
NAME = re.compile(r"^\s*name:\s*([A-Za-z0-9_.-]+)\s*$")


def declared_secrets(body):
    """The block under `on.workflow_call.secrets` (empty when there is none)."""
    m = re.search(r"^    secrets:\s*\n((?:      .*\n|\s*\n)*)", body.split("\npermissions:", 1)[0] + "\n", re.M)
    return m.group(1) if m else ""


def needs_of(chunk):
    """The `needs:` of a job: a name, an inline list or a block list."""
    m = re.search(r"^    needs:[ \t]*(.*)$", chunk, re.M)
    if not m:
        return set()
    value = m.group(1).strip()
    if value.startswith("["):
        return {n.strip().strip("'\"") for n in value.strip("[]").split(",") if n.strip()}
    if value:
        return {value.strip("'\"")}
    rest = chunk[m.end():]
    return set(re.findall(r"^      - *([A-Za-z0-9_-]+)\s*$", re.match(r"((?:\n      - .*)*)", rest).group(1), re.M))


def job_if(chunk):
    m = re.search(r"^    if:\s*(.*)$", chunk, re.M)
    if not m:
        return None
    value = m.group(1).strip()
    e = re.fullmatch(r"\$\{\{\s*(.*?)\s*\}\}", value)
    return e.group(1) if e else value


def steps_of(chunk):
    """(start offset, text) of every step under `steps:`."""
    m = re.search(r"^    steps:\s*$", chunk, re.M)
    if not m:
        return []
    starts = [s.start() for s in re.finditer(r"^      - ", chunk[m.end():], re.M)]
    base = m.end()
    return [(base + a, chunk[base + a: base + (starts[k + 1] if k + 1 < len(starts) else len(chunk) - base)])
            for k, a in enumerate(starts)]


def triggers_of(body):
    """The `on:` block of a workflow, as text."""
    m = re.search(r"^on:[ \t]*(.*)\n((?:[ \t]+.*\n|[ \t]*\n)*)", body + "\n", re.M)
    if not m:
        return None, ""
    return m.group(1).strip(), m.group(2)


def check_publisher_workflow(path, body, jobs):
    """official-release.yml: two signing jobs, one key each (module docstring)."""
    problems = []
    inline, block = triggers_of(body)
    if inline:
        problems.append(f"{path}: `on: {inline}`: official-release.yml lists its triggers as a block "
                        "(push of deckpilot.*--v* tags, workflow_dispatch)")
    events = set(re.findall(r"^  ([A-Za-z_]+):", block, re.M))
    for event in sorted(events - PUBLISHER_TRIGGERS):
        problems.append(f"{path}: trigger `{event}`: the publisher keys run only on a push of a "
                        "deckpilot.*--v* tag (or a workflow_dispatch on one); never on pull requests, "
                        "forks or other workflows")
    if "push" not in events:
        problems.append(f"{path}: no `push` of release tags")
    push = re.search(r"^  push:[ \t]*\n((?:    .*\n|[ \t]*\n)*)", block, re.M)
    filters = set(re.findall(r"^    ([A-Za-z_-]+):", push.group(1), re.M)) if push else set()
    if filters != {"tags"}:
        problems.append(f"{path}: push filters {sorted(filters)}: official-release.yml runs on `tags` only")
    tags = re.findall(r"^      - *['\"]?([^'\"\s]+)['\"]?\s*$", push.group(1), re.M) if push else []
    for tag in tags:
        if not RELEASE_TAG.fullmatch(tag):
            problems.append(f"{path}: tag filter `{tag}` is not a deckpilot.<name>--v* release tag")
    if not tags:
        problems.append(f"{path}: no release tag filter")
    dispatch = re.search(r"^  workflow_dispatch:[ \t]*(.*)\n((?:    .*\n|[ \t]*\n)*)", block, re.M)
    if dispatch and (dispatch.group(1).strip() not in ("", "{}") or dispatch.group(2).strip()):
        problems.append(f"{path}: workflow_dispatch takes no inputs (nothing from outside reaches the keys)")
    if re.search(r"\b(?:inputs|github\.event)\.", "\n".join(c for _, c in jobs.items())):
        problems.append(f"{path}: `inputs.*` or `github.event.*` are used: nothing chosen at run time "
                        "reaches a job of official-release.yml")
    for name in KEY_JOBS:
        if name not in jobs:
            problems.append(f"{path}: no job {name}: each publisher key signs in its own job")
    verify = jobs.get("verify")
    if verify is None:
        problems.append(f"{path}: no job `verify` (both signatures, the pinned keys, threshold 2, no secret)")
    else:
        if not set(KEY_JOBS) <= needs_of(verify):
            problems.append(f"{path}: job verify does not need both sign-a and sign-b")
        if re.search(r"^    environment:", verify, re.M):
            problems.append(f"{path}: job verify names an environment; it runs without any key")
    release = jobs.get("release")
    if release is None or "verify" not in needs_of(release):
        problems.append(f"{path}: job release does not need verify: one signature must never be released")
    for job, chunk in jobs.items():
        held = {(a or b).upper() for a, b in SECRET.findall(chunk)} & PUBLISHER_SECRETS
        env = re.search(r"^    environment:\s*([A-Za-z0-9_.-]+)\s*$", chunk, re.M) or \
            re.search(r"^    environment:\s*\n\s+name:\s*([A-Za-z0-9_.-]+)", chunk, re.M)
        if env and env.group(1) == PUBLISHER_ENVIRONMENT and job not in KEY_JOBS:
            problems.append(f"{path}: job {job} names environment {PUBLISHER_ENVIRONMENT}; only sign-a and sign-b do")
        if not held:
            continue
        where = f"{path}: job {job}"
        if job not in KEY_JOBS:
            problems.append(f"{where} holds {', '.join(sorted(held))}: the publisher keys belong to sign-a and sign-b only")
        else:
            for name in sorted(held - KEY_JOBS[job]):
                problems.append(f"{where} holds {name}: {job} holds only {', '.join(sorted(KEY_JOBS[job]))}")
        if all(held & keys for keys in KEY_JOBS.values()):
            problems.append(f"{where} holds both publisher keys: no job may have both")
        if not env or env.group(1) != PUBLISHER_ENVIRONMENT:
            problems.append(f"{where} holds a publisher key without naming environment {PUBLISHER_ENVIRONMENT} "
                            "(its tag policy and required reviewers)")
        if job_if(chunk) != KEY_JOB_IF:
            problems.append(f"{where} holds a publisher key without the job condition `if: {KEY_JOB_IF}` "
                            "(this repository, a deckpilot.*--v* tag)")
        if "gate" not in needs_of(chunk):
            problems.append(f"{where} holds a publisher key without needing `gate` (the environment policy check)")
        steps = [(start, text) for start, text in steps_of(chunk) if SECRET.search(text)]
        rest = chunk
        for _, text in steps:
            rest = rest.replace(text, "")
        if SECRET.search(rest):
            problems.append(f"{where}: a publisher key outside a step (job-level env gives it to every step)")
        if len(steps) > 1:
            problems.append(f"{where}: {len(steps)} steps get a publisher key; only the one that signs may")
        for _, text in steps:
            if re.search(r"^      -\s+uses:|^        uses:", text, re.M):
                problems.append(f"{where}: a publisher key passed to an action; only a `run` step that signs may have it")
            for line in (line for line in text.splitlines() if SECRET.search(line)):
                m = KEY_ENV_LINE.fullmatch(line)
                if not m or m.group(1) != m.group(2) or not re.search(r"^        env:\s*$", text, re.M):
                    problems.append(f"{where}: `{line.strip()}`: a publisher key is only the step's env "
                                    "variable of the same name (NAME: ${{ secrets.NAME }})")
        for run in re.findall(r"run:\s*(\|[^\n]*\n(?:\s{10,}.*\n?)*|.*)", chunk):
            hit = COMPILES.search(run)
            if hit:
                problems.append(f"{where} runs `{hit.group(0).strip()}`: nothing compiles where a publisher key is")
    return problems


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
        # GitHub's secret names are case-insensitive: compare them upper-cased.
        names = [(a or b).upper() for a, b in SECRET.findall(body)]
        expressions = EXPRESSION.findall(body) + [m.group(2) for m in IF_CONDITION.finditer(body)]
        if any(SECRETS_CONTEXT.search(e) for e in expressions):
            problems.append(f"{path}: the secrets context is used other than as secrets.NAME "
                            "or secrets['NAME'] (toJSON(secrets), a computed index, also in `if:` "
                            "conditions and multi-line expressions): name each secret")
        head, jobs = body.split("\njobs:\n", 1) if "\njobs:\n" in body else (body, "")
        starts = [m for m in JOB.finditer(jobs)]
        chunks = {}
        for k, m in enumerate(starts):
            chunk = jobs[m.end(): starts[k + 1].start() if k + 1 < len(starts) else len(jobs)]
            chunks[m.group(1)] = chunk
            env = re.search(r"^    environment:\s*([A-Za-z0-9_.-]+)\s*$", chunk, re.M) or \
                re.search(r"^    environment:\s*\n\s+name:\s*([A-Za-z0-9_.-]+)", chunk, re.M)
            if env and env.group(1) in SIGNING_ENVIRONMENTS:
                for run in re.findall(r"run:\s*(\|[^\n]*\n(?:\s{10,}.*\n?)*|.*)", chunk):
                    hit = COMPILES.search(run)
                    if hit:
                        problems.append(f"{path}: job {m.group(1)} (environment {env.group(1)}) runs "
                                        f"`{hit.group(0).strip()}`: nothing compiles where a signing key is; "
                                        "build in an unprivileged job and install the result (tools/cli-artifact.sh)")
        if publisher:
            problems += check_publisher_workflow(path, body, chunks)
            if SECRET.search(head):
                problems.append(f"{path}: a secret outside the jobs (workflow-level env): "
                                "the publisher keys belong to one step of sign-a and sign-b")
        for name in names:
            if name.startswith(PUBLISHER_PREFIX) and name not in PUBLISHER_SECRETS:
                problems.append(f"{path}: secret {name} is not one of the two publisher keys: official releases are "
                                "signed by DECKPILOT_PUBLISHER_KEY_A and DECKPILOT_PUBLISHER_KEY_B, in sign-a and sign-b")
            elif publisher:
                if name not in PUBLISHER_SECRETS:
                    problems.append(f"{path}: secret {name} is not a publisher key")
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
