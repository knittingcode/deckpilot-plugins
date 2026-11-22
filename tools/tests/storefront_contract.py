"""DeckPilot's frozen storefront contract, mirrored in Python (stdlib only).

The interfaces of apps/desktop/src/plugins/storefront-types.ts and
fixtureFromJson() of storefront-mocks.ts at knittingcode/deckpilot
97bceac79b52932fc9c17891d68b25c853e2e72f (branch feat/storefront-agent),
so tools/tests/test_studio.py can prove that a Studio export loads into the
desktop's fixture client and has exactly the contract's shape: every
required field present with its type, no field the contract does not have.

`interfaces_of(ts_source)` reads the field names and optionality from the
TypeScript file itself; test_studio.py compares them with INTERFACES when
STOREFRONT_TYPES_TS names a copy, so this mirror cannot drift silently.
"""

import re

CONTRACT_COMMIT = "97bceac79b52932fc9c17891d68b25c853e2e72f"
MEDIA_TYPES = ("image/png", "image/webp", "image/jpeg")
SECTION_KINDS = ("bannerCarousel", "collectionRow", "spotlight", "recentlyAdded", "categoryHighlights", "catalog")
SHA256 = re.compile(r"^[0-9a-f]{64}$")
# storefront-api.ts MEDIA_DATA_URL
MEDIA_DATA_URL = re.compile(r"^data:image/(png|webp|jpeg);base64,[A-Za-z0-9+/]+={0,2}\Z")

STR, NUM, BOOL = "string", "number", "boolean"

# name: {field: (optional, type)}; a type is STR/NUM/BOOL, an interface name,
# ("list", type), ("enum", values...) or ("record", type).
INTERFACES = {
    "MediaRef": {"sha256": (False, STR), "mediaType": (False, ("enum",) + MEDIA_TYPES),
                 "width": (False, NUM), "height": (False, NUM)},
    "TargetView": {"type": (False, ("enum", "plugin", "collection", "category")), "id": (False, STR)},
    "BannerView": {"id": (False, STR), "title": (False, STR), "body": (True, STR), "cta": (True, STR),
                   "artwork": (False, "MediaRef"), "alt": (False, STR), "decorative": (True, BOOL),
                   "target": (False, "TargetView")},
    "CategoryView": {"id": (False, STR), "blurb": (True, STR)},
    "SectionView": {"id": (False, STR), "kind": (False, STR), "layout": (True, ("enum", "cards", "compact")),
                    "title": (True, STR), "description": (True, STR), "items": (False, ("list", "BannerView")),
                    "plugins": (False, ("list", STR)), "categories": (False, ("list", "CategoryView")),
                    "limit": (True, NUM), "version": (True, STR)},
    "CollectionView": {"id": (False, STR), "title": (False, STR), "description": (True, STR),
                       "plugins": (False, ("list", STR))},
    "StorefrontView": {"seq": (False, NUM), "sections": (False, ("list", "SectionView")),
                       "collections": (False, ("list", "CollectionView"))},
    "HighlightView": {"title": (False, STR), "text": (False, STR)},
    "ScreenshotView": {"media": (False, "MediaRef"), "alt": (False, STR), "caption": (True, STR)},
    "ArtView": {"media": (False, "MediaRef"), "alt": (False, STR)},
    "ListingLinks": {"documentation": (True, STR), "support": (True, STR), "changelog": (True, STR)},
    "ReleaseNoteView": {"version": (False, STR), "text": (False, STR)},
    "ListingView": {"promo": (True, STR), "descriptionMd": (True, STR),
                    "highlights": (False, ("list", "HighlightView")), "icon": (True, "MediaRef"),
                    "screenshots": (False, ("list", "ScreenshotView")), "promoArt": (True, "ArtView"),
                    "links": (False, "ListingLinks"), "releaseNotes": (False, ("list", "ReleaseNoteView"))},
}


def _is_record(value):
    return isinstance(value, dict)


def fixture_from_json(value):
    """storefront-mocks.ts fixtureFromJson(): the outer shape, as the dev build loads it."""
    if not _is_record(value):
        raise ValueError("a storefront fixture is an object")
    storefront = value.get("storefront")
    if storefront is not None and (not _is_record(storefront) or not isinstance(storefront.get("sections"), list)):
        raise ValueError("storefront must be null or have sections")
    listings = value.get("listings", {}) if value.get("listings") is not None else {}
    media = value.get("media", {}) if value.get("media") is not None else {}
    if not _is_record(listings) or not _is_record(media):
        raise ValueError("listings and media are objects")
    return {"storefront": storefront, "listings": listings,
            "media": {k: v for k, v in media.items() if isinstance(v, str)}}


def check(value, kind, where, problems):
    """Appends every difference between `value` and the contract type `kind`."""
    if kind == STR:
        if not isinstance(value, str):
            problems.append(f"{where}: expected a string")
    elif kind == NUM:
        if isinstance(value, bool) or not isinstance(value, (int, float)):
            problems.append(f"{where}: expected a number")
    elif kind == BOOL:
        if not isinstance(value, bool):
            problems.append(f"{where}: expected a boolean")
    elif isinstance(kind, tuple) and kind[0] == "enum":
        if value not in kind[1:]:
            problems.append(f"{where}: {value!r} is not one of {kind[1:]}")
    elif isinstance(kind, tuple) and kind[0] == "list":
        if not isinstance(value, list):
            problems.append(f"{where}: expected an array")
        else:
            for i, item in enumerate(value):
                check(item, kind[1], f"{where}[{i}]", problems)
    elif isinstance(kind, tuple) and kind[0] == "record":
        if not _is_record(value):
            problems.append(f"{where}: expected an object")
        else:
            for k, item in value.items():
                check(item, kind[1], f"{where}.{k}", problems)
    else:
        fields = INTERFACES[kind]
        if not _is_record(value):
            problems.append(f"{where}: expected {kind}")
            return
        for name, (optional, ftype) in fields.items():
            if name not in value:
                if not optional:
                    problems.append(f"{where}.{name}: missing ({kind})")
            elif value[name] is None:
                problems.append(f"{where}.{name}: null (an optional field is left out, never null)")
            else:
                check(value[name], ftype, f"{where}.{name}", problems)
        for name in value:
            if name not in fields:
                problems.append(f"{where}: unknown field {name} ({kind})")
        if kind == "MediaRef" and isinstance(value.get("sha256"), str) and not SHA256.match(value["sha256"]):
            problems.append(f"{where}.sha256: not 64 lowercase hex digits")
        if kind == "SectionView" and value.get("kind") not in SECTION_KINDS:
            problems.append(f"{where}.kind: {value.get('kind')!r} is not a SectionKind of this contract")


def media_refs(value):
    if isinstance(value, dict):
        if set(value) == set(INTERFACES["MediaRef"]):
            yield value
        for v in value.values():
            yield from media_refs(v)
    elif isinstance(value, list):
        for v in value:
            yield from media_refs(v)


def check_fixture(fixture):
    """Problems of a fixture_from_json() result against the contract (empty: it matches)."""
    problems = []
    if fixture["storefront"] is not None:
        check(fixture["storefront"], "StorefrontView", "storefront", problems)
    check(fixture["listings"], ("record", "ListingView"), "listings", problems)
    for sha, url in fixture["media"].items():
        if not SHA256.match(sha):
            problems.append(f"media.{sha}: not a sha256")
        if not MEDIA_DATA_URL.match(url):
            problems.append(f"media.{sha}: not a data:image/(png|webp|jpeg);base64 URL")
    for ref in media_refs({"s": fixture["storefront"], "l": fixture["listings"]}):
        url = fixture["media"].get(ref["sha256"])
        if url is None:
            problems.append(f"media: {ref['sha256']} is referenced but not delivered")
        elif not url.startswith(f"data:{ref['mediaType']};"):
            problems.append(f"media: {ref['sha256']} is not delivered as {ref['mediaType']}")
    return problems


def interfaces_of(ts_source):
    """{interface: {field: optional}} of the exported interfaces in a TypeScript file."""
    out = {}
    for m in re.finditer(r"export interface (\w+) \{(.*?)\n\}", ts_source, re.S):
        fields = {}
        for line in m.group(2).splitlines():
            fm = re.match(r"^\s{2}(\w+)(\?)?:", line)
            if fm:
                fields[fm.group(1)] = bool(fm.group(2))
        out[m.group(1)] = fields
    return out
