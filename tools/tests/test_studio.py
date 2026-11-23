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
import re
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
import storefront_contract as contract  # noqa: E402

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

    def test_delete_removes_media_only(self):
        root = self.scratch.root
        write(root, "editorial/editorial.json", {"editorial_version": 1, "sections": []})
        write(root, "registry/plugins/deckpilot.weather/listing.json", {"listing_version": 1, "id": "deckpilot.weather"})
        write(root, "editorial/media/gone.png", images.png(8, 8))
        write(root, "registry/plugins/deckpilot.weather/media/gone.png", images.png(8, 8))
        for rel in ("editorial/editorial.json", "registry/plugins/deckpilot.weather/listing.json",
                    "registry/plugins/deckpilot.weather/plugin.json", "editorial/media", "editorial/media/../editorial.json",
                    "editorial/media/gone.png\n"):
            with self.subTest(path=rel):
                status, _, _ = self.request("DELETE", "/api/media", {"path": rel}, headers=self.write_headers())
                self.assertEqual(status, 403)
        self.assertTrue((root / "editorial/editorial.json").is_file())
        self.assertTrue((root / "registry/plugins/deckpilot.weather/listing.json").is_file())
        for rel in ("editorial/media/gone.png", "registry/plugins/deckpilot.weather/media/gone.png"):
            status, _, _ = self.request("DELETE", "/api/media", {"path": rel}, headers=self.write_headers())
            self.assertEqual(status, 200)
            self.assertFalse((root / rel).exists())

    def test_media_import_writes_media_paths_only(self):
        data = base64.b64encode(images.png(1800, 600)).decode()
        for rel in ("editorial/editorial.json", "registry/plugins/deckpilot.weather/listing.json"):
            with self.subTest(path=rel):
                status, _, _ = self.request("POST", "/api/media", {"path": rel, "kind": "banner", "data": data},
                                            headers=self.write_headers())
                self.assertEqual(status, 403)

    def raw(self, request):
        import socket
        with socket.create_connection(("127.0.0.1", self.port), timeout=30) as s:
            s.sendall(request)
            s.shutdown(socket.SHUT_WR)
            reply = b""
            while chunk := s.recv(65536):
                reply += chunk
        return reply

    def test_malformed_requests_get_400(self):
        head = (f"POST /api/validate HTTP/1.1\r\nHost: 127.0.0.1:{self.port}\r\nOrigin: http://127.0.0.1:{self.port}\r\n"
                f"X-Studio-Token: {self.token}\r\nContent-Type: application/json\r\n").encode()
        cases = {
            "missing length": head + b"\r\n{}",
            "negative length": head + b"Content-Length: -5\r\n\r\n{}",
            "not a number": head + b"Content-Length: 2x\r\n\r\n{}",
            "too large": head + f"Content-Length: {studio.MAX_BODY + 1}\r\n\r\n".encode() + b"{}",
            "shorter than promised": head + b"Content-Length: 50\r\n\r\n{}",
            "not an object": head + b"Content-Length: 2\r\n\r\n[]",
            "not JSON": head + b"Content-Length: 3\r\n\r\n{x}",
            "two lengths": head + b"Content-Length: 2\r\nContent-Length: 2\r\n\r\n{}",
            "two different lengths": head + b"Content-Length: 2\r\nContent-Length: 40\r\n\r\n{}",
            "chunked": head + b"Transfer-Encoding: chunked\r\n\r\n2\r\n{}\r\n0\r\n\r\n",
        }
        for name, request in cases.items():
            with self.subTest(case=name):
                reply = self.raw(request)
                self.assertTrue(reply.startswith(b"HTTP/1.0 400") or reply.startswith(b"HTTP/1.1 400"), reply[:80])
        for path, body in (("/api/media", {"path": "editorial/media/a.png", "kind": "poster", "data": ""}),
                           ("/api/media", {"path": "editorial/media/a.png", "kind": ["banner"], "data": ""}),
                           ("/api/check-media", {"kind": "poster", "data": ""}),
                           ("/api/check-media", {"kind": {}, "data": ""}),
                           ("/api/pr", {"step": ["status"]}),
                           ("/api/preview", {"platform": "ios"}),
                           ("/api/preview", {"locale": ["en"]}),
                           ("/api/file", {"path": "editorial/editorial.json", "json": []})):
            with self.subTest(path=path, body=body):
                method = "PUT" if path == "/api/file" else "POST"
                status, _, _ = self.request(method, path, body, headers=self.write_headers())
                self.assertEqual(status, 400)

    def test_a_body_that_never_arrives_times_out_with_408(self):
        import socket
        head = (f"POST /api/validate HTTP/1.1\r\nHost: 127.0.0.1:{self.port}\r\nOrigin: http://127.0.0.1:{self.port}\r\n"
                f"X-Studio-Token: {self.token}\r\nContent-Type: application/json\r\nContent-Length: 50\r\n\r\n{{").encode()
        old = studio.Handler.timeout
        self.server.RequestHandlerClass.timeout = 1
        try:
            with socket.create_connection(("127.0.0.1", self.port), timeout=30) as s:
                s.sendall(head)  # and keep the connection open without sending the rest
                reply = s.recv(65536)
        finally:
            self.server.RequestHandlerClass.timeout = old
        self.assertTrue(reply.startswith(b"HTTP/1.0 408") or reply.startswith(b"HTTP/1.1 408"), reply[:80])

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
                          ("editorial/editorial.json\0", "bad path"), ("", "bad path"),
                          ("editorial/editorial.json\n", "control"), ("editorial/media/x.png\n", "control"),
                          ("editorial/media/x.png\r", "control"), ("editorial/media/x\tpng", "control"),
                          ("editorial/media/x.png\x85", "control"), ("editorial/media/x.png\u2028", "control")):
            with self.subTest(rel=rel):
                self.refused(rel, text)

    def test_allowed_paths_match_whole_strings(self):
        for pattern in studio.WRITABLE:
            self.assertFalse(pattern.pattern.startswith("^") or pattern.pattern.endswith("$"))
        for rel in ("editorial/media/x.png\n", "editorial/editorial.json\n"):
            self.assertFalse(any(k.fullmatch(rel) for k in studio.WRITABLE))
        source = (REPO / "tools/studio/studio.py").read_text()
        self.assertNotIn("k.match(", source)

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


def plugins_catalog(live=True):
    """plugin_catalog()-shaped data from Scratch.PLUGINS without git. `live`:
    True marks every plugin with a recorded version as live in the mirrored
    index (latest = its newest version), None as no mirror at all."""
    out = {}
    for pid, p in Scratch.PLUGINS.items():
        versions = [{"version": v, "published_at": d, "platforms": pl, "yanked": bool(p.get("yanked"))}
                    for v, d, pl in p["versions"]]
        is_live = None if live is None else bool(versions)
        out[pid] = {"id": pid, "name": p["name"], "summary": "", "categories": p["categories"], "revocations": [],
                    "versions": versions, "live": is_live,
                    "live_latest": {"stable": versions[-1]["version"]} if is_live else None}
    return out


def write_mirror(root, plugins, revocations=(), seq=7):
    """A mirrored live index in .work/studio/mirror, as mirror_live() leaves it."""
    work = Path(root) / ".work" / "studio" / "mirror"
    write(work, "status.json", {"mirrored": True, "state": "live", "log": "", "verified": None})
    write(work, "site/v1/timestamp.json", {"seq": seq, "index": {"path": f"index/{seq}.json"}})
    write(work, f"site/v1/index/{seq}.json", {"seq": seq, "revocations": list(revocations),
                                              "plugins": [{"id": pid, "latest": {"stable": v}} for pid, v in plugins.items()]})


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
        self.assertEqual(set(art), {"sha256", "mediaType", "width", "height"})  # MediaRef only
        self.assertEqual(banner["alt"], "Rain clouds over a deck")
        self.assertNotIn("decorative", banner)
        doc = copy.deepcopy(EDITORIAL)
        art = doc["sections"][0]["items"][0]["artwork"]
        del art["alt"]
        art["decorative"] = True
        banner = self.resolve(doc)[0]["sections"][0]["items"][0]
        self.assertEqual((banner["alt"], banner["decorative"]), ("", True))

    def test_spotlight_version(self):
        doc = copy.deepcopy(EDITORIAL)
        doc["sections"][2]["status"] = "published"
        spot = next(s for s in self.resolve(doc)[0]["sections"] if s["id"] == "spot-memory")
        self.assertEqual((spot["kind"], spot["plugins"], spot["version"], spot["title"], spot["description"]),
                         ("spotlight", ["deckpilot.memory"], "0.1.0", "New: Memory", "Notes on a key."))
        self.assertNotIn("headline", spot)
        self.assertNotIn("body", spot)
        self.assertNotIn("plugin", spot)
        doc["sections"][2]["version"] = "9.9.9"
        spot = next(s for s in self.resolve(doc)[0]["sections"] if s["id"] == "spot-memory")
        self.assertNotIn("version", spot)

    def test_recently_added_and_categories(self):
        view, _ = self.resolve()
        new = next(s for s in view["sections"] if s["id"] == "new")
        self.assertEqual((new["kind"], new["limit"], new["plugins"]), ("recentlyAdded", 8, []))  # the UI picks them
        cats = next(s for s in view["sections"] if s["id"] == "cats")
        self.assertEqual((cats["kind"], cats["categories"]),
                         ("categoryHighlights", [{"id": "monitoring", "blurb": "Watch what matters."}]))
        everything = next(s for s in view["sections"] if s["id"] == "all")
        self.assertEqual((everything["kind"], everything["plugins"]), ("catalog", []))

    def test_kinds_are_the_contracts(self):
        view, _ = self.resolve(drafts=True)
        self.assertEqual([s["kind"] for s in view["sections"]],
                         ["bannerCarousel", "collectionRow", "spotlight", "recentlyAdded", "categoryHighlights",
                          "catalog"])

    def test_unknown_kind_is_skipped(self):
        doc = copy.deepcopy(EDITORIAL)
        doc["sections"].append({"id": "future", "status": "published", "kind": "hero_video", "order": 1})
        view, notes = self.resolve(doc)
        self.assertNotIn("future", self.ids(view))
        self.assertIn("section future: unknown kind 'hero_video' skipped", notes)

    def test_view_is_capped(self):
        doc = copy.deepcopy(EDITORIAL)
        # Past the schema's text limits on purpose (the resolver does not rely on them).
        for s in doc["sections"]:
            s["description"] = {"en": "x" * 12000}
        view, notes = studio.resolve(doc, plugins_catalog(), now=self.NOW, media=self.media)
        self.assertLessEqual(len(json.dumps(view, ensure_ascii=False).encode()), studio.MAX_VIEW_BYTES)
        self.assertIn("section all: dropped (the view is capped at 49152 bytes)", notes)
        self.assertEqual(self.ids(view), ["launch", "focus", "new"])

    def test_collections_follow_the_same_rules(self):
        doc = copy.deepcopy(EDITORIAL)
        doc["collections"].append({"id": "a-later", "status": "published", "order": 10, "title": {"en": "Later"},
                                   "plugins": ["deckpilot.weather"], "window": {"starts_at": "2030-01-01T00:00:00Z"}})
        doc["collections"].append({"id": "a-first", "status": "published", "order": 10, "title": {"en": "First"},
                                   "plugins": ["deckpilot.weather"]})
        view, _ = self.resolve(doc)
        self.assertEqual([c["id"] for c in view["collections"]], ["a-first", "productivity"])


class LivePublication(unittest.TestCase):
    """Issue #87: the live preview and the fixture export show only what the
    live signed index serves; recorded-only releases need the pending mode."""
    NOW = "2026-12-10T12:00:00Z"
    DOC = {"editorial_version": 1, "sections": [], "collections": [
        {"id": "picks", "status": "published", "order": 10, "title": {"en": "Picks"},
         "plugins": ["deckpilot.weather", "deckpilot.pomodoro"]}]}

    def catalog(self, **kw):
        catalog = plugins_catalog(**kw)
        if kw.get("live", True) is not None:
            catalog["deckpilot.pomodoro"]["live"] = False  # recorded 0.2.1, absent from the live index
            catalog["deckpilot.pomodoro"]["live_latest"] = None
        return catalog

    def picks(self, view):
        return next((c["plugins"] for c in view["collections"] if c["id"] == "picks"), [])

    def test_recorded_but_not_live_plugin_is_dropped(self):
        catalog = self.catalog()
        self.assertTrue(catalog["deckpilot.pomodoro"]["versions"])  # a nonempty recorded version list
        self.assertIsNone(studio.installable(catalog["deckpilot.pomodoro"], "macos"))
        view, notes = studio.resolve(copy.deepcopy(self.DOC), catalog, now=self.NOW, platform="macos")
        self.assertEqual(self.picks(view), ["deckpilot.weather"])
        self.assertIn("collection picks: deckpilot.pomodoro dropped (not in the live catalog)", notes)

    def test_live_compatible_placements_render(self):
        catalog = self.catalog()
        self.assertEqual(studio.installable(catalog["deckpilot.weather"], "macos")["version"], "0.2.1")
        view, _ = studio.resolve(copy.deepcopy(self.DOC), catalog, now=self.NOW, platform="macos")
        self.assertIn("deckpilot.weather", self.picks(view))

    def test_a_recorded_version_newer_than_the_live_one_does_not_count(self):
        catalog = self.catalog()
        weather = catalog["deckpilot.weather"]
        weather["versions"].append({"version": "0.3.0", "published_at": "2026-12-01T00:00:00Z",
                                    "platforms": ["windows"], "yanked": False})
        self.assertEqual(studio.installable(weather, "macos")["version"], "0.2.1")
        self.assertEqual(studio.installable(weather, "windows")["version"], "0.2.1")
        self.assertEqual(studio.installable(weather, "windows", pending=True)["version"], "0.3.0")

    def test_missing_mirror_is_not_confirmed_absence(self):
        catalog = self.catalog(live=None)
        view, notes = studio.resolve(copy.deepcopy(self.DOC), catalog, now=self.NOW, platform="macos")
        self.assertEqual(self.picks(view), [])
        self.assertTrue(any(n.startswith("live mirror data unavailable") for n in notes), notes)
        self.assertIn("collection picks: deckpilot.weather dropped (live data unavailable)", notes)
        self.assertNotIn("collection picks: deckpilot.weather dropped (not in the live catalog)", notes)
        page = studio.render_html(view, notes, catalog, title="t", media_url=lambda s, e: "", css_href="x")
        self.assertIn("Live mirror data unavailable", page)

    def test_pending_mode_is_explicit_and_labelled(self):
        catalog = self.catalog()
        view, notes = studio.resolve(copy.deepcopy(self.DOC), catalog, now=self.NOW, platform="macos", pending=True)
        self.assertEqual(self.picks(view), ["deckpilot.weather", "deckpilot.pomodoro"])
        self.assertTrue(any(n.startswith("pending releases included") for n in notes), notes)
        page = studio.render_html(view, notes, catalog, title="t", media_url=lambda s, e: "", css_href="x",
                                  pending=True)
        self.assertIn("Pending releases included", page)

    def test_preview_and_fixture_export_agree(self):
        scratch = Scratch()
        self.addCleanup(scratch.cleanup)
        root = scratch.root
        write(root, "editorial/editorial.json", self.DOC)
        ws = scratch.ws
        # No mirror: nothing is confirmed live, in the preview and in the export.
        _, fixture = ws.export_fixture(now=self.NOW, platform="macos")
        self.assertEqual(fixture["storefront"]["collections"], [])
        self.assertFalse(fixture["live"]["available"])
        # The mirror serves weather only; pomodoro is recorded at HEAD but not live.
        write_mirror(root, {"deckpilot.weather": "0.2.1"})
        view, notes = ws.view(now=self.NOW, platform="macos")
        _, fixture = ws.export_fixture(now=self.NOW, platform="macos")
        self.assertEqual(self.picks(view), ["deckpilot.weather"])
        self.assertEqual(fixture["storefront"], view)
        self.assertEqual(fixture["notes"], notes)
        self.assertTrue(fixture["live"]["available"])
        self.assertFalse(fixture["pending"])
        out = root / ".work" / "preview"
        studio.render_preview(ws, out, now=self.NOW)
        self.assertIn("deckpilot.pomodoro dropped (not in the live catalog)", (out / "macos-en.html").read_text())
        # The pending mode, on request only.
        _, fixture = ws.export_fixture(now=self.NOW, platform="macos", pending=True)
        self.assertEqual(self.picks(fixture["storefront"]), ["deckpilot.weather", "deckpilot.pomodoro"])
        self.assertTrue(fixture["pending"])

    def test_catalog_states(self):
        scratch = Scratch()
        self.addCleanup(scratch.cleanup)
        status, plugins = scratch.ws.catalog()
        self.assertIsNone(status)
        self.assertIsNone(plugins["deckpilot.weather"]["live"])  # unknown, not "absent"
        write_mirror(scratch.root, {"deckpilot.weather": "0.2.1"})
        status, plugins = scratch.ws.catalog()
        self.assertEqual(status["seq"], 7)
        self.assertIs(plugins["deckpilot.weather"]["live"], True)
        self.assertIs(plugins["deckpilot.pomodoro"]["live"], False)


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
        # Every plugin with a recorded version is live in the mirrored index.
        write_mirror(self.root, {pid: p["versions"][-1][0] for pid, p in Scratch.PLUGINS.items() if p["versions"]})

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
        path, fixture = self.scratch.ws.export_fixture(now="2026-12-10T12:00:00Z", platform="macos", locale="en",
                                                       drafts=True)
        self.assertEqual(path, self.root / ".work" / "studio" / "storefront-fixture.json")
        on_disk = json.loads(path.read_text())
        self.assertEqual(on_disk["_type"], "deckpilot-storefront-fixture")
        self.assertNotIn("provisional", on_disk)
        self.assertIn(studio.CONTRACT_COMMIT, on_disk["contract"])
        self.assertEqual([s["id"] for s in on_disk["storefront"]["sections"]],
                         ["launch", "focus", "spot-memory", "new", "cats", "all"])
        listing = on_disk["listings"]["deckpilot.weather"]
        self.assertEqual(listing["icon"]["mediaType"], "image/png")
        self.assertEqual(len(listing["screenshots"]), 1)  # macos
        self.assertEqual(listing["promoArt"]["alt"], "Weather icons over a sky gradient")
        self.assertEqual(listing["releaseNotes"], [{"version": "0.2.1", "text": "Fixes the wind unit in Polish."}])
        self.assertEqual(len(on_disk["media"]), 4)
        for sha, url in on_disk["media"].items():
            header, _, data = url.partition(",")
            self.assertEqual(hashlib.sha256(base64.b64decode(data)).hexdigest(), sha)
        linux = self.scratch.ws.export_fixture(now="2026-12-10T12:00:00Z", platform="linux")[1]
        self.assertEqual(linux["listings"]["deckpilot.weather"]["screenshots"], [])
        # A listing with nothing optional still has every array and links.
        write(self.root, "registry/plugins/deckpilot.weather/listing.json", {"listing_version": 1, "id": "deckpilot.weather"})
        bare = self.scratch.ws.export_fixture(now="2026-12-10T12:00:00Z")[1]["listings"]["deckpilot.weather"]
        self.assertEqual(bare, {"highlights": [], "screenshots": [], "links": {}, "releaseNotes": []})

    def test_export_round_trips_through_the_contract(self):
        for platform in storefront.PLATFORMS:
            for locale in ("en", "pl", "pl-PL"):
                for drafts in (False, True):
                    with self.subTest(platform=platform, locale=locale, drafts=drafts):
                        path, _ = self.scratch.ws.export_fixture(now="2026-12-10T12:00:00Z", platform=platform,
                                                                 locale=locale, drafts=drafts)
                        raw = json.loads(path.read_text(encoding="utf-8"))
                        loaded = contract.fixture_from_json(raw)
                        self.assertEqual(contract.check_fixture(loaded), [])
                        # Nothing is lost in the round trip: what fixtureFromJson returns is the export.
                        self.assertEqual(loaded, {k: raw[k] for k in ("storefront", "listings", "media")})
        bad = json.loads(path.read_text(encoding="utf-8"))
        bad["storefront"]["sections"][0]["kind"] = "banner_carousel"
        bad["storefront"]["sections"][0]["items"][0]["artwork"]["alt"] = "x"
        bad["listings"]["deckpilot.weather"].pop("links")
        problems = contract.check_fixture(contract.fixture_from_json(bad))
        self.assertTrue(any("kind" in p for p in problems), problems)
        self.assertTrue(any("artwork: unknown field alt" in p for p in problems), problems)
        self.assertTrue(any("links: missing" in p for p in problems), problems)

    def test_catalog_marks_unpublished_plugins(self):
        _, plugins = self.scratch.ws.catalog()
        self.assertEqual(plugins["deckpilot.unpublished"]["versions"], [])
        self.assertIs(plugins["deckpilot.unpublished"]["live"], False)
        self.assertIs(plugins["deckpilot.weather"]["live"], True)
        _, plugins = self.scratch.ws.catalog(use_live=False)
        self.assertIsNone(plugins["deckpilot.weather"]["live"])  # no live data asked for: unknown


class Contract(unittest.TestCase):
    """The Python mirror of DeckPilot's storefront-types.ts (storefront_contract.py)."""

    def test_studio_and_mirror_name_the_same_contract(self):
        self.assertEqual(studio.CONTRACT_COMMIT, contract.CONTRACT_COMMIT)
        self.assertEqual(tuple(studio.SECTION_KINDS.values()), contract.SECTION_KINDS)
        self.assertEqual(set(studio.SECTION_KINDS), set(storefront.SchemaSet(REPO / "schemas").docs[
            storefront.EDITORIAL_SCHEMA]["$defs"]["section"]["properties"]["kind"]["enum"]))

    def test_mirror_equals_the_typescript(self):
        # STOREFRONT_TYPES_TS=<DeckPilot>/apps/desktop/src/plugins/storefront-types.ts at CONTRACT_COMMIT
        path = os.environ.get("STOREFRONT_TYPES_TS")
        if not path:
            self.skipTest("STOREFRONT_TYPES_TS is not set (DeckPilot's storefront-types.ts)")
        source = Path(path).read_text(encoding="utf-8")
        ts = contract.interfaces_of(source)
        ts.pop("AvailablePluginStorefrontFields", None)  # agent fields of AvailablePlugin, not in a fixture
        mirror = {name: {f: opt for f, (opt, _) in fields.items()} for name, fields in contract.INTERFACES.items()}
        self.assertEqual(ts, mirror)
        kinds = re.search(r"SECTION_KINDS: readonly SectionKind\[\] = \[(.*?)\]", source, re.S).group(1)
        self.assertEqual(tuple(re.findall(r'"(\w+)"', kinds)), contract.SECTION_KINDS)

    def test_the_contracts_own_fixture_passes_the_mirror(self):
        # The default fixture of storefront-mocks.ts, transcribed (media as data URLs).
        ref = {"sha256": "0" * 63 + "1", "mediaType": "image/png", "width": 512, "height": 512}
        fixture = {"storefront": {"seq": 42, "sections": [
            {"id": "launch", "kind": "bannerCarousel", "items": [
                {"id": "b", "title": "T", "artwork": ref, "alt": "", "decorative": True,
                 "target": {"type": "collection", "id": "productivity"}}], "plugins": [], "categories": []},
            {"id": "spot", "kind": "spotlight", "title": "New", "description": "D", "items": [],
             "plugins": ["deckpilot.weather"], "categories": [], "version": "0.2.1"}],
            "collections": [{"id": "dev", "title": "Development", "plugins": ["acme.github-prs"]}]},
            "listings": {"acme.clock": {"promo": "A clock", "highlights": [], "screenshots": [], "links": {},
                                        "releaseNotes": []}},
            "media": {ref["sha256"]: "data:image/png;base64,iVBORw0KGgo="}}
        self.assertEqual(contract.check_fixture(contract.fixture_from_json(fixture)), [])

    def test_fixture_from_json_outer_shape(self):
        for bad in ([], {"storefront": {"sections": {}}}, {"listings": []}, {"media": "x"}):
            with self.subTest(bad=bad):
                with self.assertRaises(ValueError):
                    contract.fixture_from_json(bad)
        self.assertEqual(contract.fixture_from_json({}), {"storefront": None, "listings": {}, "media": {}})
        self.assertEqual(contract.fixture_from_json({"media": {"a": 1, "b": "x"}})["media"], {"b": "x"})


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
