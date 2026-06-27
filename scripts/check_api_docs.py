#!/usr/bin/env python3
"""API-doc parity check.

The Axum router (`backend/src/server/mod.rs`) is the source of truth for the API
surface. This asserts that both documentation sources stay in lockstep with it
and with each other:

  - the human contract  `docs/api.html`
  - the OpenAPI spec     `backend/src/server/routes/docs.rs` (served at /api/docs)

It checks, per route:
  1. every (method, path) in the router is documented in BOTH docs;
  2. neither doc documents a (method, path) that is not a real route;
  3. every path parameter (`:id`) is declared in the OpenAPI spec (api.html
     documents path params inline in the URL, not as table rows);
  4. the query parameter name sets agree between api.html and the OpenAPI spec.

Request/response body *shapes* are intentionally out of scope here (that lives in
the api.html field tables); this is methods + inputs parity.

api.html param tables must use the `Name | In | Type | Required | Description`
header so the `In` column can be read; response/body field tables use a `Field`
header and are ignored. Run from anywhere:
    python3 scripts/check_api_docs.py
"""

from __future__ import annotations

import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
MOD_RS = ROOT / "backend" / "src" / "server" / "mod.rs"
API_HTML = ROOT / "docs" / "api.html"
DOCS_RS = ROOT / "backend" / "src" / "server" / "routes" / "docs.rs"

METHODS = ("get", "post", "put", "patch", "delete")
_METHOD_ALT = "|".join(METHODS)


def canonical(path: str) -> str:
    """Normalise path params so `:id` (router/html) and `{id}` (OpenAPI) compare
    equal. Param *names* are validated separately."""
    path = re.sub(r":[A-Za-z_][A-Za-z0-9_]*", "{}", path)
    path = re.sub(r"\{[A-Za-z_][A-Za-z0-9_]*\}", "{}", path)
    return path


def path_param_names(path: str) -> set[str]:
    return set(re.findall(r":([A-Za-z_][A-Za-z0-9_]*)", path)) | set(
        re.findall(r"\{([A-Za-z_][A-Za-z0-9_]*)\}", path)
    )


def parse_router() -> dict[tuple[str, str], set[str]]:
    """(method, canonical_path) -> path param names, from mod.rs `.route(...)`."""
    text = MOD_RS.read_text(encoding="utf-8")
    routes: dict[tuple[str, str], set[str]] = {}
    for chunk in text.split(".route(")[1:]:
        m_path = re.search(r'"([^"]+)"', chunk)
        if not m_path:
            continue
        raw = "/api" + m_path.group(1)
        # Method constructors live before the next router-level call.
        seg = re.split(r"\.with_state\(|\.nest\(|\.fallback\(", chunk)[0]
        methods = set(re.findall(r"\b(" + _METHOD_ALT + r")\s*\(", seg))
        if not methods:
            continue
        cp = canonical(raw)
        for method in methods:
            routes[(method, cp)] = path_param_names(raw)
    return routes


def parse_api_html() -> dict[tuple[str, str], set[tuple[str, str]]]:
    """(method, canonical_path) -> {(param_name, in)} for in in {query, path}."""
    text = API_HTML.read_text(encoding="utf-8")
    out: dict[tuple[str, str], set[tuple[str, str]]] = {}
    # Match `class="endpoint"` and `class="endpoint deprecated"` alike.
    for sec in text.split('<section class="endpoint')[1:]:
        sec = sec.split("</section>")[0]
        m_method = re.search(r'data-method="([^"]+)"', sec)
        m_path = re.search(r'data-path="([^"]+)"', sec)
        if not (m_method and m_path):
            continue
        key = (m_method.group(1).lower(), canonical(m_path.group(1)))
        params: set[tuple[str, str]] = set()
        for tr in re.findall(r"<tr>.*?</tr>", sec, re.S):
            cell = re.search(
                r"<td><code>([^<]+)</code></td>\s*<td>([^<]+)</td>", tr
            )
            if cell:
                name, loc = cell.group(1).strip(), cell.group(2).strip().lower()
                # api.html writes path params as `:month`; normalise to the bare name.
                name = name[1:] if name.startswith(":") else name
                if loc in ("query", "path"):
                    params.add((name, loc))
        out[key] = params
    return out


def parse_openapi() -> dict[tuple[str, str], set[tuple[str, str]]]:
    """(method, canonical_path) -> {(param_name, in)} from the docs.rs paths block."""
    text = DOCS_RS.read_text(encoding="utf-8")
    body = text[text.index('"paths": {'):]
    out: dict[tuple[str, str], set[tuple[str, str]]] = {}
    cur_path: str | None = None
    cur_key: tuple[str, str] | None = None
    pending_name: str | None = None  # param "name" awaiting its "in" (objects span lines)
    for line in body.splitlines():
        m_path = re.match(r'\s*"(/api/?[^"]*)":\s*\{', line)
        if m_path:
            cur_path, cur_key, pending_name = m_path.group(1), None, None
            continue
        m_method = re.match(r'\s*"(' + _METHOD_ALT + r')":\s*\{', line)
        if m_method and cur_path:
            cur_key = (m_method.group(1), canonical(cur_path))
            pending_name = None
            out.setdefault(cur_key, set())
            continue
        if cur_key:
            nm = re.search(r'"name":\s*"([^"]+)"', line)
            if nm:
                pending_name = nm.group(1)
            im = re.search(r'"in":\s*"([^"]+)"', line)
            if im and pending_name:
                if im.group(1) in ("query", "path"):
                    out[cur_key].add((pending_name, im.group(1)))
                pending_name = None
    return out


def fmt(key: tuple[str, str]) -> str:
    return f"{key[0].upper()} {key[1]}"


def main() -> int:
    routes = parse_router()
    html = parse_api_html()
    spec = parse_openapi()
    errors: list[str] = []

    route_keys = set(routes)
    # 1. every route documented in both sources
    for key in sorted(route_keys):
        if key not in html:
            errors.append(f"{fmt(key)} is a route but is missing from docs/api.html")
        if key not in spec:
            errors.append(f"{fmt(key)} is a route but is missing from the OpenAPI spec (docs.rs)")

    # 2. no stale documented endpoints
    for key in sorted(set(html) - route_keys):
        errors.append(f"{fmt(key)} is documented in docs/api.html but is not a real route")
    for key in sorted(set(spec) - route_keys):
        errors.append(f"{fmt(key)} is documented in the OpenAPI spec but is not a real route")

    # 3. every route path param is declared in the OpenAPI spec (its convention;
    #    api.html documents path params inline in the URL, not as table rows).
    # 4. query params must agree between the two docs.
    for key in sorted(route_keys):
        if key not in html or key not in spec:
            continue
        spec_path = {n for (n, loc) in spec[key] if loc == "path"}
        for name in sorted(routes[key]):
            if name not in spec_path:
                errors.append(f"{fmt(key)}: path param '{name}' is missing from the OpenAPI spec")
        html_query = {n for (n, loc) in html[key] if loc == "query"}
        spec_query = {n for (n, loc) in spec[key] if loc == "query"}
        only_html = sorted(html_query - spec_query)
        only_spec = sorted(spec_query - html_query)
        if only_html or only_spec:
            detail = []
            if only_html:
                detail.append(f"missing from OpenAPI: {only_html}")
            if only_spec:
                detail.append(f"missing from api.html: {only_spec}")
            errors.append(f"{fmt(key)}: query params disagree ({'; '.join(detail)})")

    if errors:
        for e in errors:
            print(f"::error::{e}")
        print(f"\n{len(errors)} API-doc parity problem(s) found.")
        return 1

    print(f"API-doc parity OK: {len(route_keys)} (method, path) routes match across "
          f"mod.rs, docs/api.html, and the OpenAPI spec.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
