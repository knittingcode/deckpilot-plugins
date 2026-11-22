#!/usr/bin/env python3
"""Tests for tools/studio/studio.py, Marketplace Studio (stdlib only).

    python3 -I tools/tests/test_studio.py

* The server: the per-launch token on every request (query, then cookie;
  a write needs the header), Host and Origin checks, the CSP on every
  answer.
* Path confinement: `..`, absolute paths, symlinks (directory and file),
  and every write outside editorial/** and registry/plugins/<id>/
  listing.json + media/ refused.
* The JSON round trip: the schema's key order, 2-space indent, trailing
  newline, non-ASCII kept.
* The resolver on fixtures: status, windows, platforms, locale fallback,
  ordering, dropped ids, the size cap; render-preview and export-fixture.
* Prepare PR: every step needs a confirmation, never a tag.

Everything runs on a scratch git repository; nothing touches the network.
"""

import base64
import copy
import datetime as dt
import hashlib
import http.client
import importlib.util
import json
import os
import random
import shutil
import subprocess
import sys
import tempfile
import threading
import unittest
from pathlib import Path

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent
sys.path.insert(0, str(HERE))
import images  # noqa: E402

spec = importlib.util.spec_from_file_location("studio", REPO / "tools" / "studio" / "studio.py")
studio = importlib.util.module_from_spec(spec)
spec.loader.exec_module(studio)
storefront = studio.storefront
FIXTURES = HERE / "fixtures" / "storefront"
EDITORIAL = json.loads((FIXTURES / "editorial/valid/full.json").read_text())


def git(root, *args):
    subprocess.run(["git", "-C", str(root), *args], check=True, capture_output=True)


def write(root, rel, content):
    p = Path(root) / rel
    p.parent.mkdir(parents=True, exist_ok=True)
    if isinstance(content, bytes):
        p.write_bytes(content)
    else:
        p.write_text(content if isinstance(content, str) else json.dumps(content, indent=2) + "\n")
    return p


class Scratch:
    """A repository with schemas/ and a few plugins committed at HEAD."""

    PLUGINS = {
        "deckpilot.weather": {"name": "Weather", "categories": ["monitoring"],
                              "versions": [("0.2.1", "2026-11-17T10:00:00Z", None)]},
        "deckpilot.pomodoro": {"name": "Pomodoro", "categories": ["productivity"],
                               "versions": [("0.2.1", "2026-11-18T10:00:00Z", None)]},
        "deckpilot.break-reminder": {"name": "Break reminder", "categories": ["productivity"],
                                     "versions": [("0.1.0", "2026-11-19T10:00:00Z", None)]},
        "deckpilot.memory": {"name": "Memory", "categories": ["productivity"],
                             "versions": [("0.1.0", "2026-11-20T10:00:00Z", None)]},
        "deckpilot.markets": {"name": "Markets", "categories": ["monitoring"],
                              "versions": [("0.1.0", "2026-11-16T10:00:00Z", ["windows"])]},
        "deckpilot.yanked": {"name": "Yanked", "categories": [], "versions": [("0.1.0", "2026-11-16T10:00:00Z", None)],
                             "yanked": True},
        "deckpilot.unpublished": {"name": "Unpublished", "categories": [], "versions": []},
    }

    def __init__(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name).resolve()
        git(self.root, "init", "-q", "-b", "main")
        git(self.root, "config", "user.name", "test")
        git(self.root, "config", "user.email", "test@example.invalid")
        git(self.root, "config", "commit.gpgsign", "false")
        shutil.copytree(REPO / "schemas", self.root / "schemas")
        for pid, p in self.PLUGINS.items():
            write(self.root, f"registry/plugins/{pid}/plugin.json",
                  {"id": pid, "name": p["name"], "summary": f"{p['name']} summary", "categories": p["categories"]})
            for version, published, platforms in p["versions"]:
                record = {"version": version, "published_at": published}
                if platforms:
                    record["platforms"] = platforms
                if p.get("yanked"):
                    record["yanked"] = {"reason": "test"}
                write(self.root, f"registry/plugins/{pid}/versions/{version}.json", record)
        write(self.root, "registry/revocations.json", [])
        git(self.root, "add", "-A")
        git(self.root, "commit", "-qm", "base")
        self.ws = studio.Workspace(self.root)

    def cleanup(self):
        self.tmp.cleanup()


class Server(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.scratch = Scratch()
        cls.server, cls.token = studio.make_server(cls.scratch.ws)
        cls.port = cls.server.server_address[1]
        cls.thread = threading.Thread(target=cls.server.serve_forever, daemon=True)
        cls.thread.start()

    @classmethod
    def tearDownClass(cls):
        cls.server.shutdown()
        cls.server.server_close()
        cls.scratch.cleanup()

    def request(self, method, path, body=None, headers=None, host=None):
        conn = http.client.HTTPConnection("127.0.0.1", self.port, timeout=10)
        hdrs = {"Host": host or f"127.0.0.1:{self.port}"}
        hdrs.update(headers or {})
        data = None
        if body is not None:
            data = json.dumps(body).encode()
            hdrs.setdefault("Content-Type", "application/json")
        conn.request(method, path, body=data, headers=hdrs)
        r = conn.getresponse()
        payload = r.read()
        conn.close()
        return r.status, dict(r.getheaders()), payload

    def write_headers(self, **extra):
        h = {"X-Studio-Token": self.token, "Origin": f"http://127.0.0.1:{self.port}"}
        h.update(extra)
        return h

    # -- token ----------------------------------------------------------------

    def test_every_request_needs_the_token(self):
        for path in ("/", "/api/state", "/static/app.js", "/preview", "/api/file?path=editorial/media/a.png",
                     "/media/" + "0" * 64 + ".png", "/nope"):
            with self.subTest(path=path):
                status, _, _ = self.request("GET", path)
                self.assertEqual(status, 403)
                status, _, _ = self.request("GET", path + ("&" if "?" in path else "?") + "token=wrong")
                self.assertEqual(status, 403)

    def test_query_token_sets_a_strict_cookie(self):
        status, headers, _ = self.request("GET", f"/?token={self.token}")
        self.assertEqual(status, 303)
        self.assertEqual(headers["Location"], "/")
        cookie = headers["Set-Cookie"]
        for part in ("HttpOnly", "SameSite=Strict", "Path=/"):
            self.assertIn(part, cookie)
        status, _, body = self.request("GET", "/", headers={"Cookie": cookie.split(";")[0]})
        self.assertEqual(status, 200)
        self.assertIn(f'content="{self.token}"'.encode(), body)
        status, _, _ = self.request("GET", "/api/state", headers={"Cookie": "studio_token=wrong"})
        self.assertEqual(status, 403)

    def test_a_write_needs_the_header_not_just_the_cookie(self):
        body = {"path": "editorial/editorial.json", "json": {"editorial_version": 1, "sections": []}}
        cookie = {"Cookie": f"studio_token={self.token}", "Origin": f"http://127.0.0.1:{self.port}"}
        status, _, _ = self.request("PUT", "/api/file", body, headers=cookie)
        self.assertEqual(status, 403)
        status, _, _ = self.request("PUT", f"/api/file?token={self.token}", body,
                                    headers={"Origin": f"http://127.0.0.1:{self.port}"})
        self.assertEqual(status, 403)
        status, _, _ = self.request("PUT", "/api/file", body, headers=self.write_headers(**{"X-Studio-Token": "x"}))
        self.assertEqual(status, 403)
        status, _, payload = self.request("PUT", "/api/file", body, headers=self.write_headers())
        self.assertEqual(status, 200, payload)

    # -- Host, Origin, CSP --------------------------------------------------------

    def test_host_must_be_this_server(self):
        for host in ("evil.example", f"evil.example:{self.port}", "127.0.0.1", f"127.0.0.1:{self.port + 1}",
                     f"0.0.0.0:{self.port}"):
            with self.subTest(host=host):
                status, _, _ = self.request("GET", f"/api/state?token={self.token}", host=host)
                self.assertEqual(status, 403)
        status, _, _ = self.request("GET", f"/api/state?token={self.token}", host=f"localhost:{self.port}")
        self.assertEqual(status, 200)

    def test_origin_checks(self):
        body = {"step": "status"}
        for origin in (None, "http://evil.example", f"http://127.0.0.1:{self.port + 1}", "null"):
            with self.subTest(origin=origin):
                headers = {"X-Studio-Token": self.token}
                if origin:
                    headers["Origin"] = origin
                status, _, _ = self.request("POST", "/api/pr", body, headers=headers)
                self.assertEqual(status, 403)
        status, _, _ = self.request("GET", f"/api/state?token={self.token}", headers={"Origin": "http://evil.example"})
        self.assertEqual(status, 403)
        status, _, _ = self.request("POST", "/api/pr", body, headers=self.write_headers())
        self.assertEqual(status, 200)

    def test_csp_and_headers_on_every_answer(self):
        for method, path in (("GET", f"/?token={self.token}"), ("GET", f"/api/state?token={self.token}"),
                             ("GET", "/api/state"), ("GET", f"/static/app.js?token={self.token}"),
                             ("GET", f"/preview?token={self.token}")):
            with self.subTest(path=path):
                _, headers, _ = self.request(method, path)
                self.assertEqual(headers["Content-Security-Policy"], studio.CSP)
                self.assertIn("default-src 'self'", headers["Content-Security-Policy"])
                self.assertIn("img-src 'self' blob:", headers["Content-Security-Policy"])
                self.assertEqual(headers["X-Content-Type-Options"], "nosniff")
                self.assertEqual(headers["Cache-Control"], "no-store")

    def test_binds_loopback_only(self):
        self.assertEqual(self.server.server_address[0], "127.0.0.1")
        self.assertNotEqual(self.port, 0)

    def test_static_names_are_confined(self):
        for name in ("../studio.py", "%2e%2e/studio.py", "app.py", "APP.js", "sub/app.js"):
            with self.subTest(name=name):
                status, _, _ = self.request("GET", f"/static/{name}?token={self.token}")
                self.assertNotEqual(status, 200)

    # -- writes through the API -----------------------------------------------------

    def test_api_refuses_writes_outside_the_allowed_paths(self):
        for rel in ("../outside.json", "/etc/passwd", "keys/roots.json", "registry/plugins/deckpilot.weather/plugin.json",
                    "editorial/other.json", "tools/x.json", ".github/CODEOWNERS",
                    "registry/plugins/acme.unknown/listing.json"):
            with self.subTest(path=rel):
                status, _, _ = self.request("PUT", "/api/file", {"path": rel, "json": {}}, headers=self.write_headers())
                self.assertEqual(status, 403)
        status, _, _ = self.request("POST", "/api/media", {"path": "keys/a.png", "kind": "icon",
                                                           "data": base64.b64encode(images.png(512, 512)).decode()},
                                    headers=self.write_headers())
        self.assertEqual(status, 403)
        self.assertFalse((self.scratch.root / "keys").exists())

    def test_media_import_checks_before_writing(self):
        def post(rel, data, kind):
            return self.request("POST", "/api/media", {"path": rel, "kind": kind, "data": base64.b64encode(data).decode()},
                                headers=self.write_headers())
        status, _, payload = post("editorial/media/bad.png", images.png(1800, 1200), "banner")
        self.assertEqual(status, 200)
        result = json.loads(payload)
        self.assertFalse(result["written"])
        self.assertTrue(any("is not 3:1" in p for p in result["problems"]))
        self.assertFalse((self.scratch.root / "editorial/media/bad.png").exists())
        status, _, payload = post("editorial/media/svg.png", b"<svg xmlns='http://www.w3.org/2000/svg'/>", "banner")
        self.assertIn("SVG is refused", json.loads(payload)["problems"][0])
        status, _, _ = post("editorial/media/art.svg", b"<svg/>", "banner")
        self.assertEqual(status, 403)
        status, _, payload = post("editorial/media/ok-banner.png", images.png(1800, 600), "banner")
        self.assertTrue(json.loads(payload)["written"], payload)
        self.assertEqual((self.scratch.root / "editorial/media/ok-banner.png").read_bytes(), images.png(1800, 600))
        status, headers, body = self.request("GET", f"/api/file?path=editorial/media/ok-banner.png&token={self.token}")
        self.assertEqual((status, headers["Content-Type"]), (200, "image/png"))

    def test_preview_endpoint(self):
        status, _, payload = self.request("POST", "/api/preview", {"platform": "macos", "locale": "en"},
                                          headers=self.write_headers())
        self.assertEqual(status, 200)
        self.assertIn("view", json.loads(payload))


class Confinement(unittest.TestCase):
    def setUp(self):
        self.scratch = Scratch()
        self.root = self.scratch.root
        self.outside = tempfile.TemporaryDirectory()

    def tearDown(self):
        self.scratch.cleanup()
        self.outside.cleanup()

    def refused(self, rel, text=""):
        with self.assertRaises(studio.PathError) as cm:
            studio.write_bytes(self.root, rel, b"{}")
        self.assertIn(text, str(cm.exception))

    def test_dot_dot_absolute_and_odd_paths(self):
        for rel, text in (("../x.json", ".."), ("editorial/../keys/roots.json", ".."),
                          ("editorial/./editorial.json", "'.'"), ("editorial//editorial.json", "empty"),
                          ("/tmp/editorial.json", "absolute"), ("C:/editorial.json", "absolute"),
                          ("~/editorial.json", "absolute"), ("editorial\\editorial.json", "bad path"),
                          ("editorial/editorial.json\0", "bad path"), ("", "bad path")):
            with self.subTest(rel=rel):
                self.refused(rel, text)

    def test_only_the_allowed_files(self):
        for rel in ("editorial/editorial.json.bak", "editorial/notes.md", "editorial/media/a.svg",
                    "editorial/media/A.png", "editorial/media/a.jpeg", "editorial/media/sub/a.png",
                    "registry/plugins/deckpilot.weather/plugin.json",
                    "registry/plugins/deckpilot.weather/versions/9.9.9.json",
                    "registry/plugins/deckpilot.weather/media/a.gif", "registry/publishers/x.json",
                    "registry/revocations.json", "keys/roots.json", "keys/1.json", "tools/studio/studio.py",
                    "schemas/inputs/listing.schema.json", ".github/workflows/x.yml", ".git/config", "README.md"):
            with self.subTest(rel=rel):
                self.refused(rel, "Studio writes only")

    def test_listing_only_for_a_plugin_of_registry(self):
        self.refused("registry/plugins/acme.unknown/listing.json", "not a plugin of registry/plugins/")
        studio.write_bytes(self.root, "registry/plugins/deckpilot.weather/listing.json", b"{}\n")
        studio.write_bytes(self.root, "registry/plugins/deckpilot.weather/media/icon.png", b"x")
        self.assertTrue((self.root / "registry/plugins/deckpilot.weather/media/icon.png").is_file())

    def test_symlinked_directories_are_refused(self):
        os.symlink(self.outside.name, self.root / "editorial")
        self.refused("editorial/editorial.json", "symlink")
        self.refused("editorial/media/a.png", "symlink")
        self.assertEqual(os.listdir(self.outside.name), [])

    def test_symlinked_media_dir_and_files_are_refused(self):
        os.symlink(self.outside.name, self.root / "registry/plugins/deckpilot.weather/media")
        self.refused("registry/plugins/deckpilot.weather/media/icon.png", "symlink")
        (self.root / "editorial").mkdir()
        target = Path(self.outside.name) / "victim.json"
        target.write_text("keep")
        os.symlink(target, self.root / "editorial/editorial.json")
        self.refused("editorial/editorial.json", "symlink")
        self.assertEqual(target.read_text(), "keep")

    def test_symlinked_plugin_dir_is_refused(self):
        shutil.copytree(self.root / "registry/plugins/deckpilot.weather", Path(self.outside.name) / "p")
        os.symlink(Path(self.outside.name) / "p", self.root / "registry/plugins/acme.linked")
        self.refused("registry/plugins/acme.linked/listing.json", "symlink")

    def test_delete_is_confined_too(self):
        with self.assertRaises(studio.PathError):
            studio.delete_file(self.root, "registry/plugins/deckpilot.weather/plugin.json")
        self.assertTrue((self.root / "registry/plugins/deckpilot.weather/plugin.json").is_file())

    def test_save_json_never_reads_or_writes_keys(self):
        (self.root / "keys").mkdir()
        (self.root / "keys" / "roots.json").write_text("{}")
        with self.assertRaises(studio.PathError):
            self.scratch.ws.save_json("keys/roots.json", {})
        with self.assertRaises(studio.PathError):
            self.scratch.ws.read_json("keys/roots.json")
        source = (REPO / "tools/studio/studio.py").read_text()
        self.assertNotIn(".key", source.replace(".keys()", ""))


class RoundTrip(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.scratch = Scratch()
        cls.ws = cls.scratch.ws

    @classmethod
    def tearDownClass(cls):
        cls.scratch.cleanup()

    def test_fixtures_round_trip_byte_for_byte(self):
        for kind, ref in (("editorial", storefront.EDITORIAL_SCHEMA), ("listing", storefront.LISTING_SCHEMA)):
            for path in sorted((FIXTURES / kind / "valid").glob("*.json")):
                with self.subTest(fixture=path.name):
                    text = path.read_text(encoding="utf-8")
                    self.assertEqual(self.ws.canonical.dump(json.loads(text), ref), text)

    def shuffled(self, value, rng):
        if isinstance(value, dict):
            items = list(value.items())
            rng.shuffle(items)
            return {k: self.shuffled(v, rng) for k, v in items}
        if isinstance(value, list):
            return [self.shuffled(v, rng) for v in value]
        return value

    def test_key_order_is_the_schemas_whatever_the_input_order(self):
        expected = (FIXTURES / "editorial/valid/full.json").read_text(encoding="utf-8")
        for seed in range(5):
            doc = self.shuffled(json.loads(expected), random.Random(seed))
            self.assertEqual(self.ws.canonical.dump(doc, storefront.EDITORIAL_SCHEMA), expected)
        listing = (FIXTURES / "listing/valid/full.json").read_text(encoding="utf-8")
        doc = self.shuffled(json.loads(listing), random.Random(7))
        self.assertEqual(self.ws.canonical.dump(doc, storefront.LISTING_SCHEMA), listing)

    def test_saved_file_format(self):
        doc = copy.deepcopy(EDITORIAL)
        errors = self.ws.save_json("editorial/editorial.json", doc)
        self.assertEqual(errors, [])
        raw = (self.scratch.root / "editorial/editorial.json").read_bytes()
        self.assertTrue(raw.endswith(b"}\n") and not raw.endswith(b"\n\n"))
        self.assertIn(b'\n  "editorial_version": 1,', raw)
        self.assertIn("Sprawdź".encode(), raw)  # not \\u-escaped
        self.assertEqual(raw.decode(), (FIXTURES / "editorial/valid/full.json").read_text(encoding="utf-8"))
        self.assertEqual(self.ws.read_json("editorial/editorial.json"), EDITORIAL)
        # Saving what was read changes nothing.
        self.ws.save_json("editorial/editorial.json", self.ws.read_json("editorial/editorial.json"))
        self.assertEqual((self.scratch.root / "editorial/editorial.json").read_bytes(), raw)

    def test_release_notes_and_locales_have_a_stable_order(self):
        doc = {"id": "deckpilot.weather", "listing_version": 1,
               "release_notes": {"0.10.0": {"pl": "b", "en": "a"}, "0.2.1": {"en": "c"}, "0.9.0": {"en": "d"}}}
        out = self.ws.canonical.dump(doc, storefront.LISTING_SCHEMA)
        self.assertLess(out.index('"listing_version"'), out.index('"id"'))
        self.assertLess(out.index('"0.2.1"'), out.index('"0.9.0"'))
        self.assertLess(out.index('"0.9.0"'), out.index('"0.10.0"'))
        self.assertLess(out.index('"en": "a"'), out.index('"pl": "b"'))

    def test_invalid_documents_are_saved_with_their_errors(self):
        doc = {"editorial_version": 1, "sections": [{"id": "x", "status": "draft", "kind": "spotlight", "order": 1}]}
        errors = self.ws.save_json("editorial/editorial.json", doc)
        self.assertTrue(any("'plugin' is required" in e for e in errors), errors)


def plugins_catalog():
    """plugin_catalog()-shaped data from Scratch.PLUGINS without git."""
    out = {}
    for pid, p in Scratch.PLUGINS.items():
        out[pid] = {"id": pid, "name": p["name"], "summary": "", "categories": p["categories"], "revocations": [],
                    "versions": [{"version": v, "published_at": d, "platforms": pl, "yanked": bool(p.get("yanked"))}
                                 for v, d, pl in p["versions"]]}
    return out


class Resolver(unittest.TestCase):
    NOW = "2026-12-10T12:00:00Z"

    def setUp(self):
        self.scratch = Scratch()
        self.media = studio.Media(self.scratch.root)
        write(self.scratch.root, "editorial/media/weather-banner.webp", images.webp(1800, 600))

    def tearDown(self):
        self.scratch.cleanup()

    def resolve(self, editorial=None, **kw):
        kw.setdefault("now", self.NOW)
        kw.setdefault("media", self.media)
        return studio.resolve(copy.deepcopy(editorial or EDITORIAL), plugins_catalog(), **kw)

    def ids(self, view):
        return [s["id"] for s in view["sections"]]

    def test_published_sections_in_order(self):
        view, notes = self.resolve()
        self.assertEqual(self.ids(view), ["launch", "focus", "new", "cats", "all"])
        self.assertIn("section spot-memory: hidden (draft)", notes)
        self.assertEqual(view["collections"][0]["id"], "productivity")

    def test_drafts_toggle_and_archived(self):
        doc = copy.deepcopy(EDITORIAL)
        doc["sections"][3]["status"] = "archived"
        view, _ = self.resolve(doc, drafts=True)
        self.assertEqual(self.ids(view), ["launch", "focus", "spot-memory", "cats", "all"])

    def test_windows(self):
        for now, shown in (("2026-11-30T23:59:59Z", False), ("2026-12-01T00:00:00Z", True),
                           ("2027-01-14T23:59:59Z", True), ("2027-01-15T00:00:00Z", False)):
            with self.subTest(now=now):
                view, notes = self.resolve(now=now)
                self.assertEqual("launch" in self.ids(view), shown, notes)

    def test_platform_targeting(self):
        doc = copy.deepcopy(EDITORIAL)
        doc["sections"][1]["platforms"] = ["windows"]
        doc["sections"][1]["plugins"].append("deckpilot.markets")  # windows only
        view, notes = self.resolve(doc, platform="macos")
        self.assertNotIn("focus", self.ids(view))
        self.assertIn("section focus: hidden (not on macos)", notes)
        view, _ = self.resolve(doc, platform="windows")
        focus = next(s for s in view["sections"] if s["id"] == "focus")
        self.assertEqual(focus["plugins"], ["deckpilot.pomodoro", "deckpilot.break-reminder", "deckpilot.markets"])
        all_mac = next(s for s in self.resolve(platform="macos")[0]["sections"] if s["id"] == "all")
        self.assertNotIn("deckpilot.markets", all_mac["plugins"])

    def test_locale_fallback(self):
        cases = (("pl", "Sprawdź, zanim wyjdziesz", "Weather and air quality on your keys."),
                 ("pl-PL", "Sprawdź, zanim wyjdziesz", "Weather and air quality on your keys."),
                 ("en", "Know before you go", "Weather and air quality on your keys."),
                 ("de", "Know before you go", "Weather and air quality on your keys."))
        for locale, title, body in cases:
            with self.subTest(locale=locale):
                banner = self.resolve(locale=locale)[0]["sections"][0]["items"][0]
                self.assertEqual((banner["title"], banner["body"]), (title, body))
        self.assertEqual(studio.locale_chain("pl-PL"), ["pl-PL", "pl", "en"])

    def test_ties_break_by_id(self):
        doc = copy.deepcopy(EDITORIAL)
        for s in doc["sections"]:
            s["order"] = 5
        view, _ = self.resolve(doc)
        self.assertEqual(self.ids(view), sorted(self.ids(view)))

    def test_unknown_yanked_unpublished_ids_are_dropped(self):
        doc = copy.deepcopy(EDITORIAL)
        doc["sections"][1]["plugins"] = ["acme.unknown", "deckpilot.yanked", "deckpilot.unpublished",
                                         "deckpilot.pomodoro"]
        view, notes = self.resolve(doc)
        focus = next(s for s in view["sections"] if s["id"] == "focus")
        self.assertEqual(focus["plugins"], ["deckpilot.pomodoro"])
        self.assertIn("section focus: acme.unknown dropped (unknown)", notes)
        self.assertIn("section focus: deckpilot.yanked dropped (not installable on macos)", notes)
        doc["sections"][1]["plugins"] = ["acme.unknown"]
        view, notes = self.resolve(doc)
        self.assertNotIn("focus", self.ids(view))
        self.assertIn("section focus: dropped (no plugin left)", notes)

    def test_revoked_plugins_are_dropped(self):
        catalog = plugins_catalog()
        catalog["deckpilot.weather"]["revocations"] = [{"id": "deckpilot.weather", "versions": "*", "action": "disable"}]
        view, notes = studio.resolve(copy.deepcopy(EDITORIAL), catalog, now=self.NOW, media=self.media)
        self.assertNotIn("launch", self.ids(view))  # its only banner targets weather
        self.assertIn("section launch: banner weather-banner dropped (target plugin deckpilot.weather not shown)", notes)

    def test_banner_needs_valid_artwork_and_target(self):
        doc = copy.deepcopy(EDITORIAL)
        doc["sections"][0]["items"][0]["target"] = {"type": "collection", "id": "nope"}
        self.assertNotIn("launch", self.ids(self.resolve(doc)[0]))
        doc["sections"][0]["items"][0]["target"] = {"type": "collection", "id": "productivity"}
        self.assertIn("launch", self.ids(self.resolve(doc)[0]))
        (self.scratch.root / "editorial/media/weather-banner.webp").unlink()
        view, notes = self.resolve(doc, media=studio.Media(self.scratch.root))
        self.assertNotIn("launch", self.ids(view))
        self.assertTrue(any("artwork missing or invalid" in n for n in notes))

    def test_media_refs(self):
        banner = self.resolve()[0]["sections"][0]["items"][0]
        art = banner["artwork"]
        self.assertEqual((art["mediaType"], art["width"], art["height"]), ("image/webp", 1800, 600))
        self.assertRegex(art["sha256"], r"^[0-9a-f]{64}$")
        self.assertEqual(art["alt"], "Rain clouds over a deck")

    def test_spotlight_version(self):
        doc = copy.deepcopy(EDITORIAL)
        doc["sections"][2]["status"] = "published"
        spot = next(s for s in self.resolve(doc)[0]["sections"] if s["id"] == "spot-memory")
        self.assertEqual((spot["plugins"], spot["version"], spot["headline"]), (["deckpilot.memory"], "0.1.0", "New: Memory"))
        doc["sections"][2]["version"] = "9.9.9"
        spot = next(s for s in self.resolve(doc)[0]["sections"] if s["id"] == "spot-memory")
        self.assertNotIn("version", spot)

    def test_recently_added_and_categories(self):
        view, _ = self.resolve()
        new = next(s for s in view["sections"] if s["id"] == "new")
        self.assertEqual(new["plugins"][:3], ["deckpilot.memory", "deckpilot.break-reminder", "deckpilot.pomodoro"])
        cats = next(s for s in view["sections"] if s["id"] == "cats")
        self.assertEqual(cats["categories"][0]["id"], "monitoring")
        self.assertEqual(cats["categories"][0]["plugins"], ["deckpilot.weather"])

    def test_unknown_kind_is_skipped(self):
        doc = copy.deepcopy(EDITORIAL)
        doc["sections"].append({"id": "future", "status": "published", "kind": "hero_video", "order": 1})
        view, notes = self.resolve(doc)
        self.assertNotIn("future", self.ids(view))
        self.assertIn("section future: unknown kind 'hero_video' skipped", notes)

    def test_view_is_capped(self):
        doc = copy.deepcopy(EDITORIAL)
        many = [f"deckpilot.p{i:05d}" for i in range(3000)]  # the catalog section alone exceeds 48 KiB
        catalog = plugins_catalog()
        for pid in many:
            catalog[pid] = {"id": pid, "name": pid, "categories": [], "revocations": [],
                            "versions": [{"version": "1.0.0", "published_at": "2026-01-01T00:00:00Z"}]}
        view, notes = studio.resolve(doc, catalog, now=self.NOW, media=self.media)
        self.assertLessEqual(len(json.dumps(view, ensure_ascii=False).encode()), studio.MAX_VIEW_BYTES)
        self.assertIn("section all: dropped (the view is capped at 49152 bytes)", notes)
        self.assertEqual(self.ids(view), ["launch", "focus", "new", "cats"])

    def test_collections_follow_the_same_rules(self):
        doc = copy.deepcopy(EDITORIAL)
        doc["collections"].append({"id": "a-later", "status": "published", "order": 10, "title": {"en": "Later"},
                                   "plugins": ["deckpilot.weather"], "window": {"starts_at": "2030-01-01T00:00:00Z"}})
        doc["collections"].append({"id": "a-first", "status": "published", "order": 10, "title": {"en": "First"},
                                   "plugins": ["deckpilot.weather"]})
        view, _ = self.resolve(doc)
        self.assertEqual([c["id"] for c in view["collections"]], ["a-first", "productivity"])


class Outputs(unittest.TestCase):
    def setUp(self):
        self.scratch = Scratch()
        self.root = self.scratch.root
        write(self.root, "editorial/editorial.json", EDITORIAL)
        write(self.root, "editorial/media/weather-banner.webp", images.webp(1800, 600))
        listing = json.loads((FIXTURES / "listing/valid/full.json").read_text())
        write(self.root, "registry/plugins/deckpilot.weather/listing.json", listing)
        write(self.root, "registry/plugins/deckpilot.weather/media/icon.png", images.png(512, 512))
        write(self.root, "registry/plugins/deckpilot.weather/media/forecast.png", images.png(1280, 800))
        write(self.root, "registry/plugins/deckpilot.weather/media/promo.webp", images.webp(1600, 800))

    def tearDown(self):
        self.scratch.cleanup()

    def test_check_passes_on_the_fixture_tree(self):
        result = self.scratch.ws.validate()
        self.assertEqual(result["errors"], [])
        self.assertFalse(result["rust"]["ran"])

    def test_render_preview(self):
        out = self.root / ".work" / "preview"
        r = subprocess.run([sys.executable, "-I", str(REPO / "tools/studio/studio.py"), "--root", str(self.root),
                            "render-preview", "--out", str(out), "--as-of", "2026-12-10T12:00:00Z"],
                           capture_output=True, text=True)
        self.assertEqual(r.returncode, 0, r.stderr)
        names = sorted(p.name for p in out.iterdir())
        self.assertIn("index.html", names)
        self.assertIn("summary.md", names)
        self.assertIn("preview.css", names)
        for platform in storefront.PLATFORMS:
            for locale in storefront.LOCALES:
                self.assertIn(f"{platform}-{locale}.html", names)
        page = (out / "macos-pl.html").read_text()
        self.assertIn("Sprawdź, zanim wyjdziesz", page)
        self.assertIn('src="media/', page)
        self.assertNotIn("<script", page)
        self.assertEqual(len(list((out / "media").iterdir())), 1)
        summary = (out / "summary.md").read_text()
        self.assertIn("| `launch` | banner_carousel | published | 10 | 2026-12-01T00:00:00Z → 2027-01-15T00:00:00Z |", summary)

    def test_render_preview_escapes_text(self):
        doc = copy.deepcopy(EDITORIAL)
        doc["sections"][1]["title"] = {"en": "<script>alert(1)</script>"}
        write(self.root, "editorial/editorial.json", doc)
        out = self.root / ".work" / "preview"
        studio.render_preview(self.scratch.ws, out, now="2026-12-10T12:00:00Z")
        page = (out / "macos-en.html").read_text()
        self.assertNotIn("<script>alert", page)
        self.assertIn("&lt;script&gt;", page)

    def test_export_fixture(self):
        path, fixture = self.scratch.ws.export_fixture(now="2026-12-10T12:00:00Z", platform="macos", locale="en")
        self.assertEqual(path, self.root / ".work" / "studio" / "storefront-fixture.json")
        on_disk = json.loads(path.read_text())
        self.assertEqual(on_disk["_type"], "deckpilot-storefront-fixture")
        self.assertIn("provisional", on_disk)
        self.assertEqual([s["id"] for s in on_disk["storefront"]["sections"]], ["launch", "focus", "new", "cats", "all"])
        listing = on_disk["listings"]["deckpilot.weather"]
        self.assertEqual(listing["icon"]["mediaType"], "image/png")
        self.assertEqual(len(listing["screenshots"]), 1)  # macos
        self.assertEqual(listing["releaseNotes"], [{"version": "0.2.1", "text": "Fixes the wind unit in Polish."}])
        self.assertEqual(len(on_disk["media"]), 4)
        for sha, entry in on_disk["media"].items():
            data = (path.parent / entry["file"]).read_bytes()
            self.assertEqual(hashlib.sha256(data).hexdigest(), sha)
            self.assertEqual(len(data), entry["length"])
        linux = self.scratch.ws.export_fixture(now="2026-12-10T12:00:00Z", platform="linux")[1]
        self.assertEqual(linux["listings"]["deckpilot.weather"]["screenshots"], [])

    def test_catalog_marks_unpublished_plugins(self):
        _, plugins = self.scratch.ws.catalog()
        self.assertEqual(plugins["deckpilot.unpublished"]["versions"], [])
        self.assertFalse(plugins["deckpilot.weather"]["live"])  # no mirror in the scratch repository


class Preparepr(unittest.TestCase):
    def setUp(self):
        self.scratch = Scratch()
        self.root = self.scratch.root
        self.calls = []

    def tearDown(self):
        self.scratch.cleanup()

    def fake(self, cmd, **kw):
        self.calls.append(cmd)
        if cmd[0] == "git":
            return subprocess.run(cmd, **kw)
        return subprocess.CompletedProcess(cmd, 0, "https://github.com/example/pull/1\n", "")

    def step(self, name, **args):
        return studio.pr_step(self.root, name, args, run=self.fake)

    def test_steps_need_confirmation(self):
        write(self.root, "editorial/editorial.json", EDITORIAL)
        for name in ("branch", "add", "commit", "push", "pr"):
            with self.subTest(step=name):
                with self.assertRaises(studio.StudioError):
                    self.step(name, slug="x", message="m")
                with self.assertRaises(studio.StudioError):
                    self.step(name, slug="x", message="m", confirm="yes")
        self.assertEqual(self.calls, [])

    def test_full_flow_commits_only_storefront_inputs(self):
        write(self.root, "editorial/editorial.json", EDITORIAL)
        write(self.root, "tools/unrelated.txt", "x")
        status = self.step("status")
        self.assertEqual(status["changed"], ["editorial/editorial.json"])
        self.assertEqual(status["other"], ["tools/unrelated.txt"])
        with self.assertRaises(studio.StudioError):
            self.step("branch", confirm=True, slug="Bad Slug")
        r = self.step("branch", confirm=True, slug="winter-launch")
        self.assertTrue(r["ok"], r)
        branch = subprocess.run(["git", "-C", str(self.root), "branch", "--show-current"], capture_output=True,
                                text=True).stdout.strip()
        self.assertEqual(branch, f"editorial/{dt.datetime.now(dt.timezone.utc):%Y-%m-%d}-winter-launch")
        self.assertTrue(self.step("add", confirm=True)["ok"])
        self.assertTrue(self.step("commit", confirm=True, message="feat(editorial): winter launch")["ok"])
        files = subprocess.run(["git", "-C", str(self.root), "show", "--name-only", "--format=", "HEAD"],
                               capture_output=True, text=True).stdout.split()
        self.assertEqual(files, ["editorial/editorial.json"])
        self.step("push", confirm=True)
        self.step("pr", confirm=True)
        push, pr = self.calls[-2], self.calls[-1]
        self.assertEqual(push[-3:], ["-u", "origin", f"HEAD:refs/heads/{branch}"])
        self.assertEqual(pr[:3], ["gh", "pr", "create"])
        for cmd in self.calls:
            self.assertNotIn("tag", cmd)
            self.assertFalse(any("--tags" in c or "refs/tags" in c for c in cmd))

    def test_push_and_pr_only_from_editorial_branches(self):
        for name in ("push", "pr"):
            with self.subTest(step=name):
                with self.assertRaises(studio.StudioError) as cm:
                    self.step(name, confirm=True)
                self.assertIn("refusing", str(cm.exception))

    def test_there_is_no_tag_step(self):
        self.assertNotIn("tag", studio.PR_STEPS)
        with self.assertRaises(studio.StudioError):
            self.step("tag", confirm=True)


if __name__ == "__main__":
    unittest.main(verbosity=2)
