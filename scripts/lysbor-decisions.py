#!/usr/bin/env python3
"""Applique les décisions de risque de config/lysbor-decisions.json au projet Lysbor de l'image.

    python3 scripts/lysbor-decisions.py            # simulation : ce qui serait enregistré
    python3 scripts/lysbor-decisions.py --apply    # enregistre les décisions

Variables : LYSBOR_URL, LYSBOR_API_KEY (clé du projet image), LYSBOR_PROJECT.

Une décision par composant (paquet binaire) dont le paquet source est listé et qui porte des vulnérabilités, avec
« jusqu'au correctif » : un correctif publié rouvre les vulnérabilités du composant et les fait entrer dans le plan.
Un composant déjà couvert par la même décision est laissé tel quel ; un paquet source vulnérable sans décision est
signalé (à analyser avant de l'ajouter au fichier).
"""
from __future__ import annotations

import json
import os
import re
import sys
import urllib.error
import urllib.request
from urllib.parse import unquote

HERE = os.path.dirname(os.path.abspath(__file__))
DECISIONS = os.path.join(HERE, "..", "config", "lysbor-decisions.json")


def api(method: str, path: str, body: dict | None = None):
    url = os.environ["LYSBOR_URL"].rstrip("/") + "/api/v1" + path
    data = json.dumps(body).encode() if body is not None else None
    req = urllib.request.Request(url, data=data, method=method, headers={
        "Authorization": "Bearer " + os.environ["LYSBOR_API_KEY"],
        "Content-Type": "application/json",
        "User-Agent": "aurion-ci/1 lysbor-decisions",
    })
    try:
        with urllib.request.urlopen(req, timeout=120) as resp:
            raw = resp.read()
    except urllib.error.HTTPError as e:
        # Never the key: the code and the start of the answer say who refuses
        sys.exit(f"Lysbor a refusé {method} {path} : HTTP {e.code} {e.read()[:300].decode('utf-8', 'replace')}")
    return json.loads(raw) if raw else None


def source_of(component: dict) -> str:
    """Debian source package of a component: the purl qualifier upstream (without its version), else the binary name."""
    m = re.search(r"[?&]upstream=([^&#]+)", component.get("purl") or "")
    if not m:
        return component["name"]
    value = unquote(m.group(1)).strip()
    value = re.sub(r"\s*\(.*\)$", "", value)
    return value.split("@", 1)[0]


def main() -> None:
    apply = "--apply" in sys.argv[1:]
    for var in ("LYSBOR_URL", "LYSBOR_API_KEY", "LYSBOR_PROJECT"):
        if not os.environ.get(var):
            sys.exit(f"{var} manquant")
    project = os.environ["LYSBOR_PROJECT"]
    if not re.fullmatch(r"[0-9a-f-]{36}", project):
        sys.exit("LYSBOR_PROJECT n'est pas un identifiant de projet")

    cfg = json.load(open(DECISIONS, encoding="utf-8"))
    decisions, until_fix = cfg["decisions"], bool(cfg.get("until_fix", True))

    p = api("GET", f"/projects/{project}")
    sbom = p.get("latest_sbom_id")
    if not sbom:
        sys.exit("Aucun inventaire dans ce projet")
    components = api("GET", f"/projects/{project}/sboms/{sbom}/components")

    todo, kept, undecided = [], 0, {}
    for c in components:
        if not c.get("ecosystem") or not (c.get("findings_count") or c.get("ignored_count")):
            continue
        src = source_of(c)
        d = decisions.get(src)
        if d is None:
            if c.get("findings_count"):
                undecided.setdefault(src, []).append(c["name"])
            continue
        w = c.get("waiver") or {}
        if w.get("reason") == d["reason"] and w.get("vex_state") == d["state"] and bool(w.get("until_fix")) == until_fix:
            kept += 1
            continue
        todo.append((src, c, d))

    print(f"Projet {p.get('name', project)}, inventaire {sbom}")
    print(f"{len(todo)} décision(s) à enregistrer, {kept} déjà en place")
    for src, c, d in todo:
        print(f"  {src:18} {c['name']}@{c['version']} : {d['state']} ({c.get('findings_count', 0)} ouvertes)")
    if undecided:
        print("Paquets sources vulnérables sans décision (à analyser) :")
        for src, names in sorted(undecided.items()):
            print(f"  {src} : {', '.join(sorted(names))}")
    if not apply:
        print("Simulation : relancer avec --apply pour enregistrer.")
        return
    for src, c, d in todo:
        api("POST", f"/projects/{project}/waivers", {
            "component_id": c["id"],
            "reason": d["reason"],
            "until_fix": until_fix,
            "vex_state": d["state"],
            "vex_justification": d.get("justification", ""),
        })
        print(f"  enregistré : {c['name']}")
    print("Décisions enregistrées.")


if __name__ == "__main__":
    main()
