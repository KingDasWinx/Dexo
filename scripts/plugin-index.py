#!/usr/bin/env python3
"""Add one plugin release to Dexo's plugin index, written to stdout.

    python3 scripts/plugin-index.py <current index.json> <dexo-plugin.toml> <dist dir> <release url>

The current index may be missing: the first release starts one. Every
<id>-<version>*.tar.gz in <dist dir> is listed with its sha256 and size. Refused: an
index of another schema, a manifest Dexo would refuse, a version already listed, an
artifact for a platform Dexo is not built for.
"""
import datetime
import hashlib
import json
import os
import re
import sys
import tomllib

TARGETS = {
    "x86_64-unknown-linux-gnu",
    "aarch64-unknown-linux-gnu",
    "x86_64-apple-darwin",
    "aarch64-apple-darwin",
    "x86_64-pc-windows-msvc",
}
ID = re.compile(r"^[a-z0-9][a-z0-9-]{1,31}$")
VERSION = re.compile(r"^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$")
MANIFEST_KEYS = {"schema", "id", "kind", "version", "name", "description", "authors",
                 "license", "homepage", "min_dexo", "driver", "theme"}
DRIVER_KEYS = {"protocol", "drivers", "display_name", "dialect", "file", "url_schemes", "executable"}
LISTED_DRIVER_KEYS = ("protocol", "drivers", "display_name", "dialect", "file", "url_schemes")


def die(message):
    print(f"plugin-index: {message}", file=sys.stderr)
    sys.exit(1)


def read_manifest(path):
    with open(path, "rb") as file:
        manifest = tomllib.load(file)
    unknown = set(manifest) - MANIFEST_KEYS
    if unknown:
        die(f"unknown keys in the manifest: {sorted(unknown)}")
    if manifest.get("schema") != 1:
        die("the manifest is not schema 1")
    if not ID.match(manifest.get("id", "")):
        die(f"{manifest.get('id')!r} is not a plugin id")
    for field in ("version", "min_dexo"):
        if not VERSION.match(manifest.get(field, "")):
            die(f"{field} {manifest.get(field)!r} is not a version")
    kind = manifest.get("kind")
    if kind not in ("driver", "theme"):
        die(f"unknown kind {kind!r}")
    if ("driver" in manifest) != (kind == "driver") or ("theme" in manifest) != (kind == "theme"):
        die("the manifest's section does not match its kind")
    if kind == "driver" and set(manifest["driver"]) - DRIVER_KEYS:
        die(f"unknown [driver] keys: {sorted(set(manifest['driver']) - DRIVER_KEYS)}")
    return manifest


def main():
    if len(sys.argv) != 5:
        die("usage: plugin-index.py <current index.json> <dexo-plugin.toml> <dist dir> <release url>")
    current, manifest_path, dist, release_url = sys.argv[1:]
    index = {"schema": 1, "generated": "", "plugins": []}
    if os.path.exists(current):
        with open(current, encoding="utf-8") as file:
            index = json.load(file)
    if index.get("schema") != 1:
        die("the current index is not schema 1")
    manifest = read_manifest(manifest_path)
    plugin_id, kind, version = manifest["id"], manifest["kind"], manifest["version"]
    entry = {"version": version, "min_dexo": manifest["min_dexo"], "artifacts": {}}
    if kind == "driver":
        entry["driver"] = {key: manifest["driver"][key] for key in LISTED_DRIVER_KEYS if key in manifest["driver"]}
    prefix = f"{plugin_id}-{version}"
    for name in sorted(os.listdir(dist)):
        if not (name.startswith(prefix) and name.endswith(".tar.gz")):
            continue
        target = name[len(prefix):-len(".tar.gz")].lstrip("-") or "any"
        if kind == "driver" and target not in TARGETS:
            die(f"{name}: {target!r} is not a platform Dexo is built for")
        if kind == "theme" and target != "any":
            die(f"{name}: a theme pack has one artifact, for any platform")
        with open(os.path.join(dist, name), "rb") as file:
            data = file.read()
        entry["artifacts"][target] = {
            "url": f"{release_url}/{name}",
            "sha256": hashlib.sha256(data).hexdigest(),
            "size": len(data),
        }
    if not entry["artifacts"]:
        die(f"no {prefix}*.tar.gz in {dist}")
    plugin = next((p for p in index["plugins"] if p["id"] == plugin_id), None)
    if plugin is None:
        plugin = {"id": plugin_id, "versions": []}
        index["plugins"].append(plugin)
    if plugin.get("kind", kind) != kind:
        die(f"{plugin_id} was a {plugin['kind']} plugin")
    if any(listed["version"] == version for listed in plugin["versions"]):
        die(f"{plugin_id} {version} is already in the index")
    for key in ("kind", "name", "description", "license", "homepage"):
        plugin[key] = manifest.get(key, "")
    plugin["authors"] = manifest.get("authors", [])
    plugin["versions"].append(entry)
    index["plugins"].sort(key=lambda listed: listed["id"])
    index["generated"] = datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
    json.dump(index, sys.stdout, indent=2, ensure_ascii=False)
    sys.stdout.write("\n")


if __name__ == "__main__":
    main()
