#!/usr/bin/env python3
"""Marketplace Studio: the owner's local editor for the storefront (stdlib only).

    python3 -I tools/studio/studio.py [serve] [--offline] [--no-browser]
    python3 -I tools/studio/studio.py render-preview --out DIR [--as-of T] [--drafts]
    python3 -I tools/studio/studio.py export-fixture [--platform P] [--locale L] [--as-of T] [--drafts]
    python3 -I tools/studio/studio.py check

Maintainer-only (docs/studio.md). It edits editorial/editorial.json,
editorial/media/, registry/plugins/<id>/listing.json and
registry/plugins/<id>/media/, and nothing else in the repository. Scratch
output goes to .work/studio/ (ignored by git).

serve: a local web UI on 127.0.0.1 and a random port. Every request needs
  the per-launch token (in the URL printed at start, then a SameSite=Strict
  cookie; writes also need it in the X-Studio-Token header), the Host must
  be this server's, a write's Origin must be this server's, and every
  answer carries its own CSP. Paths are confined to the allowed files
  (no `..`, no absolute paths, no symlinks anywhere on the way). Studio
  never reads or handles keys, never tags and never publishes: "Prepare PR"
  runs git switch -c / add / commit / push and gh pr create, each only
  after its own confirmation.
render-preview: a static HTML wireframe of Discover for each platform and
  locale (published entries as of now, or --as-of), with a summary.md for
  a CI job summary.
export-fixture: .work/studio/storefront-fixture.json, the StorefrontFixture
  that DeckPilot's fixtureFromJson() loads (storefront-mocks.ts; the views
  of storefront-types.ts at CONTRACT_COMMIT, images as data: URLs), for
  the desktop dev build.
check: tools/check-storefront.py's checks plus `deckpilot-registry
  storefront check` when the pinned CLI has it.

The plugins Studio offers are the ones of registry/ at HEAD, marked with
their state in the live signed index (mirrored with tools/mirror-site.sh
and verified with the pinned verify-site when the CLI is built). The
resolver applies the client's rules (design §6): status, UTC windows,
platform, locale fallback (exact tag, language, en), order then id, and
drops placements of unknown, revoked, yanked-only or platform-incompatible
plugins. Studio cannot know the user's DeckPilot version, so host
compatibility (engines) is not applied.
"""

import argparse
import base64
import datetime as dt
import hashlib
import hmac
import html
import http.server
import json
import os
import re
import secrets
import shutil
import subprocess
import sys
import tempfile
import threading
import urllib.parse
import webbrowser
from pathlib import Path

sys.dont_write_bytecode = True
STUDIO_DIR = Path(__file__).resolve().parent
TOOLS = STUDIO_DIR.parent
sys.path.insert(0, str(TOOLS / "lib"))
import storefront  # noqa: E402

REPO = TOOLS.parent
STATIC = STUDIO_DIR / "static"
LIVE_URL = "https://knittingcode.github.io/deckpilot-plugins"
MAX_BODY = 4 * 1024 * 1024
MAX_VIEW_BYTES = 48 * 1024
ID = r"[a-z][a-z0-9]*(?:-[a-z][a-z0-9]*)*\.[a-z][a-z0-9]*(?:-[a-z][a-z0-9]*)*"
NAME = r"[a-z0-9-]{1,48}\.(?:png|webp|jpg)"
WRITABLE = (
    re.compile(r"^editorial/editorial\.json$"),
    re.compile(rf"^editorial/media/{NAME}$"),
    re.compile(rf"^registry/plugins/{ID}/listing\.json$"),
    re.compile(rf"^registry/plugins/{ID}/media/{NAME}$"),
)
MEDIA_TYPES = {"png": "image/png", "webp": "image/webp", "jpg": "image/jpeg"}
STATIC_TYPES = {".html": "text/html; charset=utf-8", ".js": "text/javascript; charset=utf-8",
                ".css": "text/css; charset=utf-8"}
CSP = ("default-src 'self'; img-src 'self' blob:; style-src 'self'; script-src 'self'; connect-src 'self'; "
       "frame-src 'self'; frame-ancestors 'self'; base-uri 'none'; form-action 'none'; object-src 'none'")


class StudioError(Exception):
    status = 400


class PathError(StudioError):
    status = 403


# --------------------------------------------------------------------------
# Paths and files
# --------------------------------------------------------------------------


def confine(root, rel, kinds=WRITABLE):
    """The path of `rel` under `root`, or PathError.

    `rel` is a relative POSIX path matching one of `kinds`, without `.`,
    `..`, empty or absolute parts, and no existing part of it (directory or
    file) is a symlink; the result is inside the real root."""
    root = Path(root).resolve()
    if not isinstance(rel, str) or not rel or "\0" in rel or "\\" in rel:
        raise PathError("bad path")
    if rel.startswith("/") or re.match(r"^[A-Za-z]:", rel) or rel.startswith("~"):
        raise PathError(f"{rel}: absolute paths are refused")
    parts = rel.split("/")
    if any(p in ("", ".", "..") for p in parts):
        raise PathError(f"{rel}: '.', '..' and empty parts are refused")
    if not any(k.match(rel) for k in kinds):
        raise PathError(f"{rel}: Studio writes only editorial/editorial.json, editorial/media/<name>, "
                        "registry/plugins/<id>/listing.json and registry/plugins/<id>/media/<name>")
    current = root
    for part in parts:
        current = current / part
        if current.is_symlink():
            raise PathError(f"{rel}: {current.relative_to(root)} is a symlink")
    real = Path(os.path.realpath(current))
    if real != root and root not in real.parents:
        raise PathError(f"{rel}: outside the repository")
    if rel.startswith("registry/plugins/"):
        plugin_dir = root / "registry" / "plugins" / parts[2]
        if not (plugin_dir / "plugin.json").is_file():
            raise PathError(f"{rel}: {parts[2]} is not a plugin of registry/plugins/")
    return current


def _mkdirs(root, target):
    """Creates target's parent directories, refusing symlinks on the way."""
    root = Path(root).resolve()
    current = root
    for part in target.relative_to(root).parts[:-1]:
        current = current / part
        if current.is_symlink():
            raise PathError(f"{current.relative_to(root)} is a symlink")
        if not current.exists():
            current.mkdir()
        elif not current.is_dir():
            raise PathError(f"{current.relative_to(root)} is not a directory")


def write_bytes(root, rel, data):
    target = confine(root, rel)
    _mkdirs(root, target)
    fd, tmp = tempfile.mkstemp(dir=target.parent, prefix=".studio-")
    try:
        with os.fdopen(fd, "wb") as f:
            f.write(data)
            f.flush()
            os.fsync(f.fileno())
        confine(root, rel)  # nothing became a symlink meanwhile
        os.replace(tmp, target)
    except BaseException:
        if os.path.exists(tmp):
            os.unlink(tmp)
        raise
    return target


def delete_file(root, rel):
    target = confine(root, rel)
    if target.is_file():
        target.unlink()


# --------------------------------------------------------------------------
# JSON: stable key order (the schema's), 2-space indent, trailing newline
# --------------------------------------------------------------------------


class Canonical:
    def __init__(self, schemas):
        self.schemas = schemas

    def _branches(self, value, schema, base, depth=0):
        """({key: (subschema, base)} in the schema's order, additionalProperties) of a node.

        A node's own properties, its $ref's and those of the allOf branches
        that apply (if/then) are candidate orders; the longest one wins
        (a section's kind schema over the one-field base), and the keys only
        the others name follow it."""
        if not isinstance(schema, dict) or depth > 16:
            return {}, None
        candidates, extra = [], None
        if "$ref" in schema:
            target, tbase = self.schemas._resolve(schema["$ref"], base)
            p, e = self._branches(value, target, tbase, depth + 1)
            candidates.append(p)
            extra = e
        own = {k: (v, base) for k, v in schema.get("properties", {}).items()}
        candidates.append(own)
        if isinstance(schema.get("additionalProperties"), dict):
            extra = (schema["additionalProperties"], base)
        for sub in schema.get("allOf", []):
            if "if" in sub:
                if not self.schemas._ok(value, sub["if"], base) or "then" not in sub:
                    continue
                sub = sub["then"]
            p, e = self._branches(value, sub, base, depth + 1)
            candidates.append(p)
            extra = extra or e
        merged = {}
        for cand in sorted(candidates, key=len, reverse=True):
            for k, v in cand.items():
                merged.setdefault(k, v)
        return merged, extra

    def order(self, value, schema, base):
        if isinstance(value, list):
            items = None
            if isinstance(schema, dict):
                node, nbase = schema, base
                while isinstance(node, dict) and "$ref" in node and "items" not in node:
                    node, nbase = self.schemas._resolve(node["$ref"], nbase)
                if isinstance(node, dict) and "items" in node:
                    items = (node["items"], nbase)
            return [self.order(v, *(items or (None, base))) for v in value]
        if not isinstance(value, dict):
            return value
        props, extra = self._branches(value, schema, base)
        out = {}
        for k, (sub, sbase) in props.items():
            if k in value:
                out[k] = self.order(value[k], sub, sbase)
        rest = [k for k in value if k not in out]
        if all(k in storefront.LOCALES for k in rest):
            rest.sort(key=storefront.LOCALES.index)
        else:
            rest.sort(key=lambda k: (semver_key(k) is None, semver_key(k) or (), k))
        for k in rest:
            sub = extra or (None, base)
            out[k] = self.order(value[k], *sub)
        return out

    def dump(self, value, ref):
        _, base = self.schemas._resolve(ref, None)
        schema, _ = self.schemas._resolve(ref, None)
        ordered = self.order(value, schema, base)
        return json.dumps(ordered, indent=2, ensure_ascii=False) + "\n"


def semver_key(text):
    m = re.match(r"^(\d+)\.(\d+)\.(\d+)(?:-(alpha|beta|rc)\.(\d+))?$", text or "")
    if not m:
        return None
    core = tuple(int(x) for x in m.group(1, 2, 3))
    if m.group(4) is None:
        return core + (1, "", 0)
    return core + (0, m.group(4), int(m.group(5)))


# --------------------------------------------------------------------------
# Plugins: registry/ at HEAD and the live signed index
# --------------------------------------------------------------------------


def git(root, *args, check=True):
    r = subprocess.run(["git", "-C", str(root), *args], capture_output=True, text=True)
    if check and r.returncode != 0:
        raise StudioError(f"git {' '.join(args)}: {r.stderr.strip()}")
    return r.stdout


def registry_at_head(root):
    """{id: {name, categories, versions: [{version, published_at, platforms, yanked}]}}."""
    out = {}
    try:
        names = git(root, "ls-tree", "-r", "--name-only", "HEAD", "--", "registry/plugins/").splitlines()
    except StudioError:
        return out
    for path in names:
        m = re.match(rf"^registry/plugins/({ID})/(plugin\.json|versions/([^/]+)\.json)$", path)
        if not m:
            continue
        pid = m.group(1)
        entry = out.setdefault(pid, {"id": pid, "name": pid, "summary": "", "categories": [], "versions": []})
        try:
            doc = json.loads(git(root, "show", f"HEAD:{path}"))
        except (StudioError, ValueError):
            continue
        if not isinstance(doc, dict):
            continue
        if m.group(2) == "plugin.json":
            entry["name"] = doc.get("name") or pid
            entry["summary"] = doc.get("summary") or ""
            entry["categories"] = [c for c in doc.get("categories") or [] if isinstance(c, str)]
        else:
            entry["versions"].append({"version": doc.get("version") or m.group(3),
                                      "published_at": doc.get("published_at"),
                                      "platforms": doc.get("platforms"),
                                      "yanked": bool(doc.get("yanked"))})
    try:
        revocations = json.loads(git(root, "show", "HEAD:registry/revocations.json"))
    except (StudioError, ValueError):
        revocations = []
    for entry in out.values():
        entry["versions"].sort(key=lambda v: semver_key(v["version"]) or ())
        entry["revocations"] = [r for r in revocations if isinstance(r, dict) and r.get("id") == entry["id"]]
    return out


def cli_path(root):
    path = Path(root) / ".work" / "bin" / "deckpilot-registry"
    return path if path.is_file() and os.access(path, os.X_OK) else None


def cli_has_storefront(root):
    cli = cli_path(root)
    if not cli:
        return False
    r = subprocess.run([str(cli), "help"], capture_output=True, text=True)
    return bool(re.search(r"^\s*storefront check", r.stdout + r.stderr, re.M))


def mirror_live(root, base_url=LIVE_URL):
    """Mirrors the live registry into .work/studio/mirror (tools/mirror-site.sh) and
    verifies it with the pinned verify-site when built. Returns the live status."""
    work = Path(root) / ".work" / "studio" / "mirror"
    if work.exists():
        shutil.rmtree(work)
    work.mkdir(parents=True)
    r = subprocess.run([str(Path(root) / "tools" / "mirror-site.sh"), base_url, str(work)], capture_output=True,
                       text=True, env={**os.environ, "REGISTRY_FRESHNESS_ATTEMPTS": "1"})
    status = {"mirrored": r.returncode == 0, "state": r.stdout.strip(), "log": r.stderr[-2000:], "verified": None}
    cli = cli_path(root)
    roots = Path(root) / "keys" / "roots.json"   # public trust roots, handed to verify-site by path
    if status["mirrored"] and status["state"] == "live" and cli and roots.is_file():
        v = subprocess.run([str(cli), "verify-site", "--dir", str(work), "--roots", str(roots)],
                           capture_output=True, text=True)
        status["verified"] = v.returncode == 0
        status["verify_log"] = (v.stdout + v.stderr)[-2000:]
    (work / "status.json").write_text(json.dumps(status, indent=2) + "\n")
    return status


def live_index(root):
    """(status, {id: index plugin entry}, revocations) of the last mirror, if any."""
    work = Path(root) / ".work" / "studio" / "mirror"
    try:
        status = json.loads((work / "status.json").read_text())
        ts = json.loads((work / "site" / "v1" / "timestamp.json").read_text())
        index_path = ts["index"]["path"] if isinstance(ts.get("index"), dict) else f"index/{ts['seq']}.json"
        if not re.fullmatch(r"index/[1-9][0-9]*\.json", index_path):
            return None, {}, []
        index = json.loads((work / "site" / "v1" / index_path).read_text())
    except (OSError, ValueError, KeyError, TypeError):
        return None, {}, []
    status = dict(status, seq=index.get("seq"))
    plugins = {p["id"]: p for p in index.get("plugins") or [] if isinstance(p, dict) and isinstance(p.get("id"), str)}
    return status, plugins, [r for r in index.get("revocations") or [] if isinstance(r, dict)]


def plugin_catalog(root, use_live=True):
    """The plugins Studio offers: registry/ at HEAD, marked with the live index."""
    head = registry_at_head(root)
    status, live, live_revocations = live_index(root) if use_live else (None, {}, [])
    out = {}
    for pid, entry in head.items():
        e = dict(entry)
        e["live"] = pid in live
        e["live_latest"] = (live.get(pid) or {}).get("latest")
        e["revocations"] = entry.get("revocations", []) + [r for r in live_revocations if r.get("id") == pid]
        out[pid] = e
    return status, out


def installable(entry, platform):
    """The newest version a client on `platform` could install, or None."""
    revoked = set()
    for r in entry.get("revocations") or []:
        if r.get("versions") == "*":
            return None
        revoked.update(r.get("versions") or [])
    for v in reversed(entry.get("versions") or []):
        if v.get("yanked") or v.get("version") in revoked:
            continue
        platforms = v.get("platforms")
        if platform and isinstance(platforms, list) and platform not in platforms:
            continue
        return v
    return None


# --------------------------------------------------------------------------
# Resolver: what a client would show (design §6)
# --------------------------------------------------------------------------


def parse_utc(text):
    if text is None:
        return None
    if isinstance(text, dt.datetime):
        return text
    m = re.fullmatch(r"(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2})(?::(\d{2}))?Z", text)
    if not m:
        raise StudioError(f"{text!r} is not a UTC time like 2026-12-01T00:00:00Z")
    return dt.datetime(*(int(x or 0) for x in m.groups()), tzinfo=dt.timezone.utc)


def utc_now():
    return dt.datetime.now(dt.timezone.utc).replace(microsecond=0)


def locale_chain(locale):
    chain = []
    for tag in (locale, (locale or "").split("-")[0], "en"):
        if tag and tag not in chain:
            chain.append(tag)
    return chain


def localize(value, chain):
    if not isinstance(value, dict):
        return None
    for tag in chain:
        if isinstance(value.get(tag), str):
            return value[tag]
    return None


def visible(entry, now, platform, drafts):
    """(shown, reason) for a section or collection."""
    status = entry.get("status")
    if status == "archived":
        return False, "archived"
    if status != "published" and not (drafts and status == "draft"):
        return False, status or "no status"
    w = entry.get("window") if isinstance(entry.get("window"), dict) else {}
    if w.get("starts_at") and now < parse_utc(w["starts_at"]):
        return False, f"starts {w['starts_at']}"
    if w.get("ends_at") and now >= parse_utc(w["ends_at"]):
        return False, f"ended {w['ends_at']}"
    platforms = entry.get("platforms")
    if isinstance(platforms, list) and platform not in platforms:
        return False, f"not on {platform}"
    return True, ""


def sort_key(entry):
    order = entry.get("order")
    return (order if isinstance(order, (int, float)) and not isinstance(order, bool) else 1 << 62, str(entry.get("id")))


class Media:
    """sha256 → file for every media file Studio knows, with its MediaRef."""

    def __init__(self, root):
        self.root = Path(root)
        self.by_sha = {}
        self.by_path = {}

    def ref(self, rel):
        """MediaRef for a repository path, or None if missing or not an image."""
        if rel in self.by_path:
            return self.by_path[rel]
        path = self.root / rel
        ref = None
        if path.is_file() and not path.is_symlink():
            data = path.read_bytes()
            try:
                info = storefront.inspect_media(data)
            except storefront.MediaError:
                info = None
            if info:
                sha = hashlib.sha256(data).hexdigest()
                ref = {"sha256": sha, "mediaType": MEDIA_TYPES[info["format"]],
                       "width": info["width"], "height": info["height"]}
                self.by_sha[sha] = (path, info["format"], len(data))
        self.by_path[rel] = ref
        return ref


# Editorial source kinds (snake_case) → the IPC SectionKind of DeckPilot's
# storefront-types.ts (camelCase).
SECTION_KINDS = {
    "banner_carousel": "bannerCarousel",
    "collection_row": "collectionRow",
    "spotlight": "spotlight",
    "recently_added": "recentlyAdded",
    "category_highlights": "categoryHighlights",
    "catalog": "catalog",
}
# The DeckPilot commit whose apps/desktop/src/plugins/storefront-types.ts
# (frozen, WP2) the views below follow field for field.
CONTRACT_COMMIT = "97bceac79b52932fc9c17891d68b25c853e2e72f"


def resolve(editorial, plugins, *, now=None, platform="macos", locale="en", drafts=False, media=None, seq=0):
    """(StorefrontView, notes): what the agent would answer to getStorefront.

    The view is exactly DeckPilot's StorefrontView (storefront-types.ts at
    CONTRACT_COMMIT): camelCase kinds; a spotlight's headline is `title`,
    its body `description`; `recentlyAdded` and `catalog` carry no plugins
    (the UI derives them from AvailablePlugin). `plugins` is
    plugin_catalog()'s; `media` a Media (artwork refs)."""
    now = parse_utc(now) if now is not None else utc_now()
    chain = locale_chain(locale)
    notes = []
    doc = editorial if isinstance(editorial, dict) else {}
    ok = {pid: v for pid, v in ((pid, installable(e, platform)) for pid, e in plugins.items()) if v}

    def plugin_ids(ids, where):
        out = []
        for pid in ids or []:
            if pid in ok:
                if pid not in out:
                    out.append(pid)
            else:
                notes.append(f"{where}: {pid} dropped ({'unknown' if pid not in plugins else 'not installable on ' + platform})")
        return out

    collections = []
    for c in sorted((c for c in doc.get("collections") or [] if isinstance(c, dict)), key=sort_key):
        shown, why = visible(c, now, platform, drafts)
        if not shown:
            notes.append(f"collection {c.get('id')}: hidden ({why})")
            continue
        ids = plugin_ids(c.get("plugins"), f"collection {c.get('id')}")
        if not ids:
            notes.append(f"collection {c.get('id')}: dropped (no plugin left)")
            continue
        view = {"id": c.get("id"), "title": localize(c.get("title"), chain) or "", "plugins": ids}
        if localize(c.get("description"), chain):
            view["description"] = localize(c.get("description"), chain)
        collections.append(view)
    collection_ids = {c["id"] for c in collections}
    categories = {}
    for pid in ok:
        for cat in plugins[pid].get("categories") or []:
            categories.setdefault(cat, []).append(pid)

    sections = []
    for s in sorted((s for s in doc.get("sections") or [] if isinstance(s, dict)), key=sort_key):
        sid, kind = s.get("id"), s.get("kind")
        shown, why = visible(s, now, platform, drafts)
        if not shown:
            notes.append(f"section {sid}: hidden ({why})")
            continue
        if kind not in SECTION_KINDS:
            notes.append(f"section {sid}: unknown kind {kind!r} skipped")
            continue
        view = {"id": sid, "kind": SECTION_KINDS[kind]}
        for field in ("title", "description"):
            text = localize(s.get(field), chain)
            if text is not None:
                view[field] = text
        view.update({"items": [], "plugins": [], "categories": []})
        if kind == "banner_carousel":
            for item in s.get("items") or []:
                if not isinstance(item, dict):
                    continue
                target = item.get("target") if isinstance(item.get("target"), dict) else {}
                ttype, tid = target.get("type"), target.get("id")
                valid = ((ttype == "plugin" and tid in ok) or (ttype == "collection" and tid in collection_ids)
                         or (ttype == "category" and tid in categories))
                if not valid:
                    notes.append(f"section {sid}: banner {item.get('id')} dropped (target {ttype} {tid} not shown)")
                    continue
                art = item.get("artwork") if isinstance(item.get("artwork"), dict) else {}
                ref = media.ref(f"editorial/{art.get('file')}") if media and art.get("file") else None
                if ref is None:
                    notes.append(f"section {sid}: banner {item.get('id')} dropped (artwork missing or invalid)")
                    continue
                banner = {"id": item.get("id"), "title": localize(item.get("title"), chain) or ""}
                for field in ("body", "cta"):
                    text = localize(item.get(field), chain)
                    if text is not None:
                        banner[field] = text
                decorative = art.get("decorative") is True
                banner["artwork"] = ref
                banner["alt"] = "" if decorative else (localize(art.get("alt"), chain) or "")
                if decorative:
                    banner["decorative"] = True
                banner["target"] = {"type": ttype, "id": tid}
                view["items"].append(banner)
            if not view["items"]:
                notes.append(f"section {sid}: dropped (no banner left)")
                continue
        elif kind == "collection_row":
            view["layout"] = s.get("layout") if s.get("layout") in ("cards", "compact") else "cards"
            view["plugins"] = plugin_ids(s.get("plugins"), f"section {sid}")
            if not view["plugins"]:
                notes.append(f"section {sid}: dropped (no plugin left)")
                continue
        elif kind == "spotlight":
            pid = s.get("plugin")
            if pid not in ok:
                notes.append(f"section {sid}: dropped (spotlight plugin {pid} not shown)")
                continue
            view["plugins"] = [pid]
            # The contract's spotlight: `title` is the headline, `description` the body.
            view["title"] = localize(s.get("headline"), chain) or ""
            body = localize(s.get("body"), chain)
            if body is not None:
                view["description"] = body
            version = s.get("version")
            recorded = {v["version"]: v for v in plugins[pid].get("versions") or []}
            if version and version in recorded and not recorded[version].get("yanked"):
                view["version"] = version
            elif version:
                notes.append(f"section {sid}: version {version} not shown (not recorded or yanked)")
        elif kind == "recently_added":
            limit = s.get("limit") if isinstance(s.get("limit"), int) else 8
            view["limit"] = limit  # the UI picks the plugins from AvailablePlugin
        elif kind == "category_highlights":
            for c in s.get("categories") or []:
                if isinstance(c, dict) and c.get("id") in categories:
                    cat = {"id": c["id"]}
                    blurb = localize(c.get("blurb"), chain)
                    if blurb is not None:
                        cat["blurb"] = blurb
                    view["categories"].append(cat)
                elif isinstance(c, dict):
                    notes.append(f"section {sid}: category {c.get('id')} dropped (no plugin shown)")
            if not view["categories"]:
                notes.append(f"section {sid}: dropped (no category left)")
                continue
        sections.append(view)  # catalog: the UI shows the full catalog

    result = {"seq": seq, "sections": sections, "collections": collections}
    while len(json.dumps(result, ensure_ascii=False).encode()) > MAX_VIEW_BYTES and result["sections"]:
        dropped = result["sections"].pop()
        notes.append(f"section {dropped['id']}: dropped (the view is capped at {MAX_VIEW_BYTES} bytes)")
    return result, notes


def listing_view(listing, *, platform="macos", locale="en", media=None, pid=None):
    """The ListingView of one listing (storefront-types.ts at CONTRACT_COMMIT): highlights,
    screenshots (filtered to `platform`), links and releaseNotes (newest first) always present."""
    if not isinstance(listing, dict):
        return None
    chain = locale_chain(locale)
    pid = pid or listing.get("id")
    base = f"registry/plugins/{pid}/"

    def ref(entry):
        if not isinstance(entry, dict) or not isinstance(entry.get("file"), str) or not media:
            return None
        return media.ref(base + entry["file"])

    view = {"highlights": [], "screenshots": [], "links": {}, "releaseNotes": []}
    for field, key in (("promo", "promo"), ("description_md", "descriptionMd")):
        text = localize(listing.get(field), chain)
        if text is not None:
            view[key] = text
    for h in listing.get("highlights") or []:
        if isinstance(h, dict):
            view["highlights"].append({"title": localize(h.get("title"), chain) or "",
                                       "text": localize(h.get("text"), chain) or ""})
    icon = ref(listing.get("icon"))
    if icon:
        view["icon"] = icon
    for s in listing.get("screenshots") or []:
        if not isinstance(s, dict):
            continue
        if isinstance(s.get("platforms"), list) and platform not in s["platforms"]:
            continue
        r = ref(s)
        if r:
            shot = {"media": r, "alt": localize(s.get("alt"), chain) or ""}
            if localize(s.get("caption"), chain) is not None:
                shot["caption"] = localize(s.get("caption"), chain)
            view["screenshots"].append(shot)
    promo = ref(listing.get("promo_art"))
    if promo:
        view["promoArt"] = {"media": promo, "alt": localize(listing["promo_art"].get("alt"), chain) or ""}
    for k in ("documentation", "support", "changelog"):
        url = (listing.get("links") or {}).get(k) if isinstance(listing.get("links"), dict) else None
        if isinstance(url, str):
            view["links"][k] = url
    notes = listing.get("release_notes") if isinstance(listing.get("release_notes"), dict) else {}
    for version in sorted(notes, key=lambda v: semver_key(v) or (), reverse=True):
        text = localize(notes[version], chain)
        if text is not None:
            view["releaseNotes"].append({"version": version, "text": text})
    return view


# --------------------------------------------------------------------------
# The workspace: reading the inputs, validation, preview, fixture
# --------------------------------------------------------------------------


class Workspace:
    def __init__(self, root=REPO):
        self.root = Path(root).resolve()
        self.schemas = storefront.SchemaSet(self.root / "schemas")
        self.canonical = Canonical(self.schemas)
        self.lock = threading.Lock()

    def read_json(self, rel):
        path = confine(self.root, rel)
        if not path.is_file():
            return None
        return json.loads(path.read_text(encoding="utf-8"))

    def editorial(self):
        doc = self.read_json("editorial/editorial.json")
        return doc if doc is not None else {"$schema": "../schemas/inputs/editorial.schema.json",
                                            "editorial_version": 1, "sections": [], "collections": []}

    def listings(self):
        out = {}
        pdir = self.root / "registry" / "plugins"
        for d in sorted(pdir.iterdir()) if pdir.is_dir() else []:
            if re.fullmatch(ID, d.name) and (d / "listing.json").is_file() and not d.is_symlink():
                try:
                    out[d.name] = self.read_json(f"registry/plugins/{d.name}/listing.json")
                except (PathError, ValueError):
                    continue
        return out

    def media_files(self):
        out = []
        dirs = [("editorial/media", self.root / "editorial" / "media")]
        pdir = self.root / "registry" / "plugins"
        for d in sorted(pdir.iterdir()) if pdir.is_dir() else []:
            if re.fullmatch(ID, d.name):
                dirs.append((f"registry/plugins/{d.name}/media", d / "media"))
        for rel, d in dirs:
            if d.is_dir() and not d.is_symlink():
                for f in sorted(d.iterdir()):
                    if f.is_file() and not f.is_symlink() and re.fullmatch(NAME, f.name):
                        out.append(f"{rel}/{f.name}")
        return out

    def save_json(self, rel, doc):
        """Writes listing.json or editorial.json in the canonical form; returns its schema errors."""
        confine(self.root, rel)
        if rel == "editorial/editorial.json":
            ref = storefront.EDITORIAL_SCHEMA
        elif re.fullmatch(rf"registry/plugins/{ID}/listing\.json", rel):
            ref = storefront.LISTING_SCHEMA
        else:
            raise PathError(f"{rel}: only editorial.json and listing.json are JSON documents")
        text = self.canonical.dump(doc, ref)
        with self.lock:
            write_bytes(self.root, rel, text.encode("utf-8"))
        return self.schemas.validate(doc, ref)

    def import_media(self, rel, data, kind):
        """Checks one image and writes it; refuses it on any finding."""
        if kind not in storefront.ASSETS:
            raise StudioError(f"unknown asset kind {kind!r}")
        target = confine(self.root, rel)
        ext = target.suffix[1:]
        info, problems = storefront.check_asset(kind, data, ext)
        if problems:
            return {"written": False, "problems": problems, "info": info}
        with self.lock:
            write_bytes(self.root, rel, data)
        return {"written": True, "problems": [], "info": info,
                "note": "header-level check; the pinned deckpilot-registry storefront check (CI) decodes it fully"}

    def validate(self):
        report = storefront.check_tree(self.root, self.schemas)
        result = {"errors": report.errors, "warnings": report.warnings, "rust": {"ran": False}}
        cli = cli_path(self.root)
        if cli and cli_has_storefront(self.root):
            r = subprocess.run([str(cli), "storefront", "check", "--dir", str(self.root)],
                               capture_output=True, text=True)
            result["rust"] = {"ran": True, "ok": r.returncode == 0, "output": (r.stdout + r.stderr)[-8000:]}
        else:
            result["rust"]["why"] = ("the pinned deckpilot-registry is not built (tools/build-cli.sh)" if not cli
                                     else "the pinned deckpilot-registry has no 'storefront check' yet")
        return result

    def catalog(self, use_live=True):
        return plugin_catalog(self.root, use_live)

    def view(self, *, now=None, platform="macos", locale="en", drafts=False, media=None):
        _, plugins = self.catalog()
        media = media or Media(self.root)
        return resolve(self.editorial(), plugins, now=now, platform=platform, locale=locale, drafts=drafts,
                       media=media)

    def export_fixture(self, out=None, *, now=None, platform="macos", locale="en", drafts=False):
        """.work/studio/storefront-fixture.json: DeckPilot's StorefrontFixture
        (storefront-mocks.ts fixtureFromJson at CONTRACT_COMMIT): `storefront`
        (StorefrontView), `listings` ({id: ListingView}) and `media` ({sha256:
        data:image/...;base64 URL}). The other top-level fields are metadata the
        loader ignores."""
        out = Path(out) if out else self.root / ".work" / "studio"
        out.mkdir(parents=True, exist_ok=True)
        media = Media(self.root)
        status, _, _ = live_index(self.root)
        seq = status.get("seq") if status and isinstance(status.get("seq"), int) else 0
        _, plugins = self.catalog()
        view, notes = resolve(self.editorial(), plugins, now=now, platform=platform, locale=locale, drafts=drafts,
                              media=media, seq=seq)
        listings = {}
        for pid, doc in self.listings().items():
            lv = listing_view(doc, platform=platform, locale=locale, media=media, pid=pid)
            if lv is not None:
                listings[pid] = lv
        files = {}
        for sha, (path, fmt, _) in sorted(media.by_sha.items()):
            files[sha] = f"data:{MEDIA_TYPES[fmt]};base64,{base64.b64encode(path.read_bytes()).decode('ascii')}"
        fixture = {
            "_type": "deckpilot-storefront-fixture",
            "contract": f"knittingcode/deckpilot@{CONTRACT_COMMIT} apps/desktop/src/plugins/storefront-types.ts",
            "generatedAt": utc_now().strftime("%Y-%m-%dT%H:%M:%SZ"),
            "asOf": (parse_utc(now) if now else utc_now()).strftime("%Y-%m-%dT%H:%M:%SZ"),
            "platform": platform, "locale": locale, "drafts": drafts,
            "storefront": view, "listings": listings, "media": files, "notes": notes,
        }
        path = out / "storefront-fixture.json"
        path.write_text(json.dumps(fixture, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
        return path, fixture


# --------------------------------------------------------------------------
# Wireframe HTML (served by Studio and written by render-preview)
# --------------------------------------------------------------------------


def render_html(view, notes, plugins, *, title, media_url, css_href, listings=None, platform="macos"):
    """A StorefrontView as a wireframe. recentlyAdded, catalog and the category
    counts are derived here from the catalog, as the UI derives them from
    AvailablePlugin."""
    e = html.escape
    listings = listings or {}
    ok = {pid: v for pid, v in ((pid, installable(p, platform)) for pid, p in plugins.items()) if v}

    def name(pid):
        return (plugins.get(pid) or {}).get("name") or pid

    def card(pid, compact=False):
        promo = localize((listings.get(pid) or {}).get("promo"), ["en"]) or (plugins.get(pid) or {}).get("summary", "")
        cls = "card compact" if compact else "card"
        return (f'<li class="{cls}"><span class="monogram" aria-hidden="true">{e(name(pid)[:1].upper())}</span>'
                f'<span class="name">{e(name(pid))}</span><span class="id">{e(pid)}</span>'
                f'<span class="promo">{e(promo[:120])}</span></li>')

    out = ["<!doctype html>", '<html lang="en"><head><meta charset="utf-8">',
           '<meta name="viewport" content="width=device-width, initial-scale=1">',
           f"<title>{e(title)}</title>", f'<link rel="stylesheet" href="{e(css_href)}"></head><body>',
           f'<header class="bar"><h1>{e(title)}</h1><p class="meta">Wireframe of Discover; not the real client '
           "UI. Cards show the registry name and the listing promo.</p></header><main>"]
    if not view["sections"]:
        out.append('<p class="empty">No editorial section is shown: Discover is catalog-only.</p>')
    for s in view["sections"]:
        out.append(f'<section class="section kind-{e(s["kind"])}" aria-label="{e(s.get("title") or s["id"])}">')
        out.append(f'<p class="tag">{e(s["kind"])} · {e(s["id"])}</p>')
        if s.get("title"):
            out.append(f"<h2>{e(s['title'])}</h2>")
        if s.get("description"):
            out.append(f"<p>{e(s['description'])}</p>")
        if s["kind"] == "bannerCarousel":
            out.append('<ol class="banners">')
            for i, b in enumerate(s["items"], 1):
                a = b["artwork"]
                alt = "" if b.get("decorative") else b.get("alt", "")
                ext = {"image/png": "png", "image/webp": "webp", "image/jpeg": "jpg"}[a["mediaType"]]
                out.append(f'<li class="banner"><img src="{e(media_url(a["sha256"], ext))}" alt="{e(alt)}" '
                           f'width="{a["width"]}" height="{a["height"]}"><div class="copy">'
                           f'<p class="tag">{i} of {len(s["items"])}</p><h3>{e(b.get("title") or "")}</h3>'
                           + (f"<p>{e(b['body'])}</p>" if b.get("body") else "")
                           + f'<p class="cta">{e(b.get("cta") or "Open")} → {e(b["target"]["type"])} '
                           f'{e(b["target"]["id"])}</p></div></li>')
            out.append("</ol>")
        elif s["kind"] == "spotlight":
            pid = s["plugins"][0]
            out.append(f'<div class="spotlight"><ul class="cards">{card(pid)}</ul>'
                       + (f'<p class="meta">Release notes of {e(s["version"])}</p>' if s.get("version") else "")
                       + "</div>")
        elif s["kind"] == "categoryHighlights":
            out.append('<ul class="categories">')
            for c in s["categories"]:
                count = sum(1 for pid in ok if c["id"] in (plugins[pid].get("categories") or []))
                out.append(f'<li><h3>{e(c["id"])}</h3>' + (f"<p>{e(c['blurb'])}</p>" if c.get("blurb") else "")
                           + f'<p class="meta">{count} plugin(s)</p></li>')
            out.append("</ul>")
        else:
            ids = s["plugins"]
            if s["kind"] == "recentlyAdded":
                ids = sorted(ok, key=lambda p: (ok[p].get("published_at") or "", p), reverse=True)[:s.get("limit") or 8]
            elif s["kind"] == "catalog":
                ids = sorted(ok, key=lambda p: (plugins[p].get("name") or p).lower())
            compact = s.get("layout") == "compact"
            label = s.get("title") or s["id"]
            out.append(f'<ul class="cards{" compact" if compact else ""}" aria-label="{e(label)}">'
                       + "".join(card(p, compact) for p in ids) + "</ul>")
        out.append("</section>")
    if view["collections"]:
        out.append('<aside class="collections"><h2>Collections (sidebar)</h2><ul>')
        for c in view["collections"]:
            out.append(f'<li><strong>{e(c["title"] or c["id"])}</strong> · {len(c["plugins"])} plugin(s)</li>')
        out.append("</ul></aside>")
    if notes:
        out.append('<details class="notes"><summary>Dropped or hidden ({})</summary><ul>'.format(len(notes)))
        out.extend(f"<li>{e(n)}</li>" for n in notes)
        out.append("</ul></details>")
    out.append("</main></body></html>")
    return "\n".join(out) + "\n"


def render_preview(ws, out, *, now=None, drafts=False):
    """Static wireframes (one per platform and locale), media/ and summary.md."""
    out = Path(out)
    (out / "media").mkdir(parents=True, exist_ok=True)
    shutil.copyfile(STATIC / "preview.css", out / "preview.css")
    _, plugins = ws.catalog()
    listings = ws.listings()
    media = Media(ws.root)
    now = parse_utc(now) if now else utc_now()
    stamp = now.strftime("%Y-%m-%dT%H:%M:%SZ")
    pages = []
    for platform in storefront.PLATFORMS:
        for locale in storefront.LOCALES:
            view, notes = resolve(ws.editorial(), plugins, now=now, platform=platform, locale=locale,
                                  drafts=drafts, media=media)
            name = f"{platform}-{locale}.html"
            (out / name).write_text(render_html(
                view, notes, plugins, title=f"Discover · {platform} · {locale} · as of {stamp}",
                media_url=lambda sha, ext: f"media/{sha}.{ext}", css_href="preview.css", listings=listings,
                platform=platform),
                encoding="utf-8")
            pages.append((platform, locale, name, view, notes))
    for sha, (path, fmt, _) in media.by_sha.items():
        shutil.copyfile(path, out / "media" / f"{sha}.{fmt}")
    e = html.escape
    index = ["<!doctype html>", '<html lang="en"><head><meta charset="utf-8"><title>Storefront preview</title>',
             '<link rel="stylesheet" href="preview.css"></head><body><main>',
             f"<h1>Storefront preview as of {e(stamp)}{' (with drafts)' if drafts else ''}</h1><ul>"]
    index += [f'<li><a href="{e(n)}">{e(p)} · {e(loc)}</a>: {len(v["sections"])} section(s)</li>'
              for p, loc, n, v, _ in pages]
    index.append("</ul></main></body></html>")
    (out / "index.html").write_text("\n".join(index) + "\n", encoding="utf-8")
    editorial = ws.editorial()
    lines = [f"### Storefront preview (as of {stamp})", "",
             "| Section | Kind | Status | Order | Window (UTC) | Platforms |", "| --- | --- | --- | --- | --- | --- |"]
    for s in sorted((s for s in editorial.get("sections") or [] if isinstance(s, dict)), key=sort_key):
        w = s.get("window") or {}
        window = f"{w.get('starts_at', '…')} → {w.get('ends_at', '…')}" if w else "always"
        lines.append(f"| `{s.get('id')}` | {s.get('kind')} | {s.get('status')} | {s.get('order')} | {window} | "
                     f"{', '.join(s.get('platforms') or ['all'])} |")
    if len(lines) == 4:
        lines.append("| — | no editorial/editorial.json | | | | |")
    lines += ["", "Shown per platform and locale: " + "; ".join(
        f"{p}/{loc} {len(v['sections'])}" for p, loc, _, v, _ in pages), ""]
    (out / "summary.md").write_text("\n".join(lines) + "\n", encoding="utf-8")
    return out / "index.html"


# --------------------------------------------------------------------------
# Prepare PR: each step separately, each confirmed by the caller
# --------------------------------------------------------------------------

PR_STEPS = ("status", "branch", "add", "commit", "push", "pr")


def changed_inputs(root):
    """(allowed changed paths, other changed paths) from git status."""
    out = git(root, "status", "--porcelain", "-z", "--no-renames", "--untracked-files=all")
    allowed, other = [], []
    for entry in [x for x in out.split("\0") if x]:
        path = entry[3:]
        if any(k.match(path) for k in WRITABLE):
            allowed.append(path)
        elif not path.startswith(".work/"):
            other.append(path)
    return sorted(set(allowed)), sorted(set(other))


def pr_step(root, step, args, *, run=subprocess.run):
    """Runs one Prepare-PR step. Never tags, never pushes anything but HEAD."""
    if step not in PR_STEPS:
        raise StudioError(f"unknown step {step!r}")
    allowed, other = changed_inputs(root)
    if step == "status":
        branch = git(root, "rev-parse", "--abbrev-ref", "HEAD").strip()
        return {"branch": branch, "changed": allowed, "other": other}
    if args.get("confirm") is not True:
        raise StudioError("each step needs an explicit confirmation")
    if step == "branch":
        slug = args.get("slug") or ""
        if not re.fullmatch(r"[a-z0-9][a-z0-9-]{0,40}", slug):
            raise StudioError("the slug is [a-z0-9-], at most 41 characters")
        name = f"editorial/{utc_now():%Y-%m-%d}-{slug}"
        cmd = ["git", "-C", str(root), "switch", "-c", name]
    elif step == "add":
        if not allowed:
            raise StudioError("nothing to add: no change to editorial/ or a listing")
        cmd = ["git", "-C", str(root), "add", "--", *allowed]
    elif step == "commit":
        message = (args.get("message") or "").strip()
        if not message or len(message) > 200 or "\n" in message:
            raise StudioError("a one-line commit message (at most 200 characters) is needed")
        if not allowed:
            raise StudioError("nothing to commit")
        cmd = ["git", "-C", str(root), "commit", "-m", message, "--", *allowed]
    elif step == "push":
        branch = git(root, "rev-parse", "--abbrev-ref", "HEAD").strip()
        if not branch.startswith("editorial/"):
            raise StudioError(f"refusing to push {branch}: Studio pushes only its editorial/<date>-<slug> branches")
        cmd = ["git", "-C", str(root), "push", "-u", "origin", f"HEAD:refs/heads/{branch}"]
    else:
        branch = git(root, "rev-parse", "--abbrev-ref", "HEAD").strip()
        if not branch.startswith("editorial/"):
            raise StudioError(f"refusing to open a pull request from {branch}")
        cmd = ["gh", "pr", "create", "--fill", "--base", "main", "--head", branch]
    r = run(cmd, cwd=str(root), capture_output=True, text=True)
    return {"command": cmd, "ok": r.returncode == 0, "output": (r.stdout + r.stderr)[-4000:]}


# --------------------------------------------------------------------------
# HTTP server
# --------------------------------------------------------------------------


class Handler(http.server.BaseHTTPRequestHandler):
    server_version = "MarketplaceStudio"
    sys_version = ""

    # Set on the class by make_server.
    ws = None
    token = ""
    port = 0

    def log_message(self, fmt, *args):
        if os.environ.get("STUDIO_LOG"):
            sys.stderr.write("studio: " + (fmt % args) + "\n")

    # -- security checks --------------------------------------------------

    def _origins(self):
        return {f"http://127.0.0.1:{self.port}", f"http://localhost:{self.port}"}

    def _host_ok(self):
        return self.headers.get("Host") in {f"127.0.0.1:{self.port}", f"localhost:{self.port}"}

    def _token_ok(self, query, write):
        given = self.headers.get("X-Studio-Token")
        if write:
            return bool(given) and hmac.compare_digest(given, self.token)
        candidates = [given] + query.get("token", [])
        cookie = self.headers.get("Cookie") or ""
        for part in cookie.split(";"):
            k, _, v = part.strip().partition("=")
            if k == "studio_token":
                candidates.append(v)
        return any(c and hmac.compare_digest(c, self.token) for c in candidates)

    def _origin_ok(self, write):
        origin = self.headers.get("Origin")
        if write:
            return origin in self._origins()
        return origin is None or origin in self._origins()

    def _headers(self, status, ctype, length, extra=()):
        self.send_response(status)
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(length))
        self.send_header("Content-Security-Policy", CSP)
        self.send_header("X-Content-Type-Options", "nosniff")
        self.send_header("Referrer-Policy", "no-referrer")
        self.send_header("Cache-Control", "no-store")
        self.send_header("Cross-Origin-Resource-Policy", "same-origin")
        self.send_header("X-Frame-Options", "SAMEORIGIN")
        for k, v in extra:
            self.send_header(k, v)
        self.end_headers()

    def _send(self, status, body, ctype="application/json", extra=()):
        if not isinstance(body, bytes):
            body = (json.dumps(body, ensure_ascii=False) + "\n").encode("utf-8")
        self._headers(status, ctype, len(body), extra)
        if self.command != "HEAD":
            self.wfile.write(body)

    def _error(self, status, message):
        self._send(status, {"error": message})

    def _guard(self, write):
        url = urllib.parse.urlsplit(self.path)
        query = urllib.parse.parse_qs(url.query)
        if not self._host_ok():
            self._error(403, "bad Host")
            return None
        if not self._origin_ok(write):
            self._error(403, "bad Origin")
            return None
        if not self._token_ok(query, write):
            self._error(403, "missing or wrong token")
            return None
        return url.path, query

    def _body(self):
        length = int(self.headers.get("Content-Length") or 0)
        if length > MAX_BODY:
            raise StudioError(f"request larger than {MAX_BODY} bytes")
        if self.headers.get("Content-Type", "").split(";")[0].strip() != "application/json":
            raise StudioError("application/json expected")
        try:
            return json.loads(self.rfile.read(length) or b"{}")
        except ValueError:
            raise StudioError("invalid JSON") from None

    # -- dispatch ---------------------------------------------------------

    def do_GET(self):
        guarded = self._guard(write=False)
        if guarded is None:
            return
        path, query = guarded
        try:
            self._get(path, query)
        except StudioError as e:
            self._error(e.status, str(e))
        except (OSError, ValueError) as e:
            self._error(500, str(e))

    def do_HEAD(self):
        self.do_GET()

    def _write(self, method):
        guarded = self._guard(write=True)
        if guarded is None:
            return
        path, query = guarded
        try:
            body = self._body()
            self._send(200, getattr(self, f"_{method}")(path, query, body))
        except StudioError as e:
            self._error(e.status, str(e))
        except (OSError, ValueError) as e:
            self._error(500, str(e))

    def do_POST(self):
        self._write("post")

    def do_PUT(self):
        self._write("put")

    def do_DELETE(self):
        self._write("delete")

    # -- GET ----------------------------------------------------------------

    def _get(self, path, query):
        ws = self.ws
        if path == "/":
            if "token" in query:
                cookie = f"studio_token={self.token}; Path=/; HttpOnly; SameSite=Strict"
                self._send(303, b"", "text/plain", extra=[("Location", "/"), ("Set-Cookie", cookie)])
                return
            page = (STATIC / "index.html").read_text(encoding="utf-8").replace("__STUDIO_TOKEN__", self.token)
            self._send(200, page.encode("utf-8"), STATIC_TYPES[".html"])
        elif path.startswith("/static/"):
            name = path[len("/static/"):]
            if not re.fullmatch(r"[a-z0-9-]+\.(js|css)", name) or not (STATIC / name).is_file():
                raise StudioError("not found")
            self._send(200, (STATIC / name).read_bytes(), STATIC_TYPES[Path(name).suffix])
        elif path == "/api/state":
            status, plugins = ws.catalog()
            self._send(200, {
                "root": str(ws.root), "branch": git(ws.root, "rev-parse", "--abbrev-ref", "HEAD", check=False).strip(),
                "live": status, "plugins": plugins, "editorial": ws.editorial(),
                "editorialExists": (ws.root / "editorial" / "editorial.json").is_file(),
                "listings": ws.listings(), "media": ws.media_files(),
                "cli": {"built": bool(cli_path(ws.root)), "storefrontCheck": cli_has_storefront(ws.root)},
                "locales": list(storefront.LOCALES), "platforms": list(storefront.PLATFORMS),
                "assets": {k: {kk: vv for kk, vv in v.items()} for k, v in storefront.ASSETS.items()},
            })
        elif path == "/api/file":
            rel = (query.get("path") or [""])[0]
            target = confine(ws.root, rel)
            if not target.is_file():
                raise StudioError("not found")
            ext = target.suffix[1:]
            if ext not in MEDIA_TYPES:
                raise StudioError("only media files are served")
            data = target.read_bytes()
            if storefront.sniff(data) != ext:
                raise StudioError("the bytes do not match the extension")
            self._send(200, data, MEDIA_TYPES[ext])
        elif path.startswith("/media/"):
            m = re.fullmatch(r"/media/([0-9a-f]{64})\.(png|webp|jpg)", path)
            if not m:
                raise StudioError("not found")
            media = Media(ws.root)
            for rel in ws.media_files():
                media.ref(rel)
            hit = media.by_sha.get(m.group(1))
            if not hit or hit[1] != m.group(2):
                raise StudioError("not found")
            self._send(200, hit[0].read_bytes(), MEDIA_TYPES[hit[1]])
        elif path == "/preview":
            params = {k: v[0] for k, v in query.items()}
            platform = params.get("platform", "macos")
            locale = params.get("locale", "en")
            if platform not in storefront.PLATFORMS or not re.fullmatch(r"[a-z]{2}(-[A-Za-z0-9]{2,8})?", locale):
                raise StudioError("bad platform or locale")
            now = params.get("as_of") or None
            drafts = params.get("drafts") == "1"
            _, plugins = ws.catalog()
            view, notes = ws.view(now=now, platform=platform, locale=locale, drafts=drafts)
            page = render_html(view, notes, plugins, title=f"Discover · {platform} · {locale}",
                               media_url=lambda sha, ext: f"/media/{sha}.{ext}", css_href="/static/preview.css",
                               listings=ws.listings(), platform=platform)
            self._send(200, page.encode("utf-8"), STATIC_TYPES[".html"])
        else:
            raise StudioError("not found")

    # -- writes -------------------------------------------------------------

    def _put(self, path, query, body):
        if path != "/api/file":
            raise StudioError("not found")
        rel, doc = body.get("path"), body.get("json")
        if not isinstance(doc, dict):
            raise StudioError("a JSON object is expected")
        return {"saved": rel, "errors": self.ws.save_json(rel, doc)}

    def _delete(self, path, query, body):
        if path != "/api/media":
            raise StudioError("not found")
        rel = body.get("path")
        with self.ws.lock:
            delete_file(self.ws.root, rel)
        return {"deleted": rel}

    def _post(self, path, query, body):
        ws = self.ws
        if path == "/api/media":
            try:
                data = base64.b64decode(body.get("data") or "", validate=True)
            except ValueError:
                raise StudioError("data must be base64") from None
            return ws.import_media(body.get("path"), data, body.get("kind"))
        if path == "/api/check-media":
            try:
                data = base64.b64decode(body.get("data") or "", validate=True)
            except ValueError:
                raise StudioError("data must be base64") from None
            ext = body.get("ext") if body.get("ext") in MEDIA_TYPES else "png"
            info, problems = storefront.check_asset(body.get("kind") or "screenshot", data, ext)
            return {"info": info, "problems": problems}
        if path == "/api/validate":
            return ws.validate()
        if path == "/api/preview":
            view, notes = ws.view(now=body.get("as_of") or None, platform=body.get("platform") or "macos",
                                  locale=body.get("locale") or "en", drafts=bool(body.get("drafts")))
            return {"view": view, "notes": notes}
        if path == "/api/mirror":
            return mirror_live(ws.root)
        if path == "/api/export-fixture":
            out, fixture = ws.export_fixture(now=body.get("as_of") or None, platform=body.get("platform") or "macos",
                                             locale=body.get("locale") or "en", drafts=bool(body.get("drafts")))
            return {"path": str(out), "sections": len(fixture["storefront"]["sections"]), "notes": fixture["notes"]}
        if path == "/api/pr":
            return pr_step(ws.root, body.get("step"), body)
        raise StudioError("not found")


def make_server(ws, port=0):
    token = secrets.token_urlsafe(32)
    handler = type("StudioHandler", (Handler,), {"ws": ws, "token": token})
    server = http.server.ThreadingHTTPServer(("127.0.0.1", port), handler)
    handler.port = server.server_address[1]
    server.daemon_threads = True
    return server, token


# --------------------------------------------------------------------------
# CLI
# --------------------------------------------------------------------------


def main(argv=None):
    parser = argparse.ArgumentParser(prog="studio.py", description="Marketplace Studio (maintainer-only)")
    parser.add_argument("--root", default=str(REPO), help=argparse.SUPPRESS)
    sub = parser.add_subparsers(dest="command")
    serve = sub.add_parser("serve", help="the local editor (default)")
    serve.add_argument("--offline", action="store_true", help="do not mirror the live site at start")
    serve.add_argument("--no-browser", action="store_true")
    rp = sub.add_parser("render-preview", help="static HTML wireframes for CI artifacts")
    rp.add_argument("--out", required=True)
    rp.add_argument("--as-of")
    rp.add_argument("--drafts", action="store_true")
    ef = sub.add_parser("export-fixture", help=".work/studio/storefront-fixture.json (fixtureFromJson) for the desktop dev build")
    ef.add_argument("--platform", default={"darwin": "macos", "win32": "windows"}.get(sys.platform, "linux"),
                    choices=storefront.PLATFORMS)
    ef.add_argument("--locale", default="en")
    ef.add_argument("--as-of")
    ef.add_argument("--drafts", action="store_true")
    sub.add_parser("check", help="the storefront checks (and the Rust check when the pinned CLI has it)")
    args = parser.parse_args(argv)
    ws = Workspace(args.root)
    command = args.command or "serve"
    if command == "render-preview":
        index = render_preview(ws, args.out, now=args.as_of, drafts=args.drafts)
        print(index)
        return 0
    if command == "export-fixture":
        path, fixture = ws.export_fixture(now=args.as_of, platform=args.platform, locale=args.locale,
                                          drafts=args.drafts)
        print(f"{path}: {len(fixture['storefront']['sections'])} section(s), {len(fixture['media'])} image(s)")
        return 0
    if command == "check":
        result = ws.validate()
        for w in result["warnings"]:
            print(f"warning: {w}", file=sys.stderr)
        for e in result["errors"]:
            print(f"error: {e}", file=sys.stderr)
        rust = result["rust"]
        print(rust.get("output", "") if rust["ran"] else f"note: Rust storefront check skipped: {rust['why']}",
              file=sys.stderr)
        return 1 if result["errors"] or (rust["ran"] and not rust["ok"]) else 0
    if not getattr(args, "offline", False):
        print("Studio: mirroring the live registry (tools/mirror-site.sh)…", file=sys.stderr)
        status = mirror_live(ws.root)
        print(f"Studio: live registry {status['state'] or 'unavailable'}"
              f"{', verified' if status.get('verified') else ''}", file=sys.stderr)
    server, token = make_server(ws)
    url = f"http://127.0.0.1:{server.server_address[1]}/?token={token}"
    print(f"Marketplace Studio: {url}\n(Ctrl-C stops it; the token is valid for this launch only.)", flush=True)
    if not getattr(args, "no_browser", False):
        webbrowser.open(url)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()
    return 0


if __name__ == "__main__":
    sys.exit(main())
