#!/usr/bin/env python3
"""Tests for scripts/check_pii.py.

EVERY VALUE IN THIS FILE IS INVENTED. None of them is, or may be replaced
with, a real identifier. That this file can exist at all is the point of
shape-based matching: a guard built on a denylist of real values could not
have a test suite without publishing the very thing it protects.

This file is in `SELF_EXEMPT` for the obvious reason — it is full of values
shaped exactly like the ones the guard rejects, because a test suite that
cannot trip the rule proves nothing.

Run:
    python3 scripts/test_check_pii.py
"""

from __future__ import annotations

import pathlib
import subprocess
import sys
import tempfile

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))

import check_pii  # noqa: E402

ROOT = pathlib.Path(__file__).resolve().parent.parent

PASSED = 0
FAILED: list[str] = []


def check(name: str, condition: bool, detail: str = "") -> None:
    global PASSED
    if condition:
        PASSED += 1
    else:
        FAILED.append(f"{name}{(': ' + detail) if detail else ''}")


def rules_firing(text: str) -> set[str]:
    return {ident for _, _, ident in check_pii.find_violations(text)}


# ── The four shapes must fire ───────────────────────────────────────────────
# Acceptance: a UTR, an NI number, a sort code and an account number, in both
# a .md and a .rs file. The account-number rule was measured and dropped (28
# false positives); that decision is pinned as a test further down, so the
# acceptance criterion is answered either way rather than silently unmet.

check(
    "utr fires when anchored by the keyword",
    "utr" in rules_firing("The UTR is 4820571963 for this year."),
)
check(
    "utr fires in a markdown table cell",
    "utr" in rules_firing("| UTR | 4820571963 | confirmed |"),
)
check(
    "utr fires in a rust string literal",
    "utr" in rules_firing('    let utr = "4820571963";'),
)
check(
    "utr fires in a rust comment",
    "utr" in rules_firing("// taxpayer reference 4820571963 belongs to the test user"),
)

check(
    "ni number fires without separators",
    "ni-number" in rules_firing("NI: SE847126C on the form"),
)
check(
    "ni number fires with spaces",
    "ni-number" in rules_firing("National Insurance SE 84 71 26 C"),
)
check(
    "ni number fires in a rust literal",
    "ni-number" in rules_firing('    let ni = "SE847126C";'),
)
check(
    "ni number fires with odd groupings",
    "ni-number" in rules_firing("SE 842716 C") and "ni-number" in rules_firing("SE8427 16C"),
)

check(
    "sort code fires when anchored",
    "sort-code" in rules_firing("Sort code: 40-27-15 at the branch"),
)
check(
    "sort code fires in a rust literal",
    "sort-code" in rules_firing('    let sort_code = "40-27-15";'),
)

check(
    "iban fires",
    "iban" in rules_firing("IBAN GB29NWBK60161331926819 for transfers"),
)
check(
    "iban fires in a rust literal",
    "iban" in rules_firing('    let iban = "GB29NWBK60161331926819";'),
)

# ── The NI prefix rule is real, not decorative ──────────────────────────────
# A previous crew's test asserted a catch that was IMPOSSIBLE: it used the
# prefix QQ, and Q is correctly excluded from the character class because HMRC
# never issues it. The regex was right and the test value was wrong, so the
# spaced-NI path was genuinely unproven while looking tested. Both directions
# are pinned here so that bug cannot come back silently.

check(
    "a valid-prefix NI number fires",
    "ni-number" in rules_firing("SE 84 71 26 C"),
)
check(
    "a never-issued prefix (Q) does NOT fire",
    "ni-number" not in rules_firing("QQ 84 71 26 C"),
    "Q is never issued as an NI prefix letter; if this fires the char class has been widened",
)
check(
    "a never-issued prefix (BG) does NOT fire",
    "ni-number" not in rules_firing("BG 84 71 26 C"),
)

# ── Anchoring: the thing that makes the rules usable ────────────────────────

check(
    "ten bare digits with no keyword do NOT fire",
    "utr" not in rules_firing("The build id is 4820571963 and the run succeeded."),
    "unanchored this rule would fire on timestamps and ids across the repo",
)
check(
    "a keyword beyond the context window does NOT fire",
    "utr" not in rules_firing("UTR" + " " * 60 + "4820571963"),
)
check(
    "a date-shaped NN-NN-NN with no keyword does NOT fire",
    "sort-code" not in rules_firing("Released 24-03-15 after review."),
)
check(
    "a sort-code keyword on a header line does not reach data lines",
    "sort-code" not in rules_firing("40-27-15,1234.00,GBP"),
    "this is the lloyds.csv fixture shape: header and data are different lines",
)

# ── Placeholders must not fire ──────────────────────────────────────────────
# Acceptance: the guard must not fire on synthetic data already in the repo.

check(
    "the conventional 1234567890 placeholder does not fire",
    "utr" not in rules_firing("The UTR is 1234567890 in the fixture."),
)
check(
    "a repeated-digit run does not fire",
    "utr" not in rules_firing("UTR 0000000000 placeholder"),
)
check(
    "the 00-00-00 sort-code placeholder does not fire",
    "sort-code" not in rules_firing("Sort code: 00-00-00"),
)

# ── The matched value is never printed ──────────────────────────────────────
# This is the guarantee that keeps a public CI log from repeating the leak.

_secret = "4820571963"
_violations = check_pii.find_violations(f"UTR {_secret}")
check(
    "find_violations reports line, column and rule only",
    all(len(v) == 3 and isinstance(v[2], str) for v in _violations),
)
check(
    "find_violations never returns the matched text",
    not any(_secret in str(part) for v in _violations for part in v),
    "nothing downstream can print what it never received",
)

# ── Scan scope: the untracked-file defect ───────────────────────────────────
# A brand-new never-added file is the realistic leak path — write a doc, paste
# a value, push. Listing only `git ls-files` missed it entirely and reported
# green from a scan that never opened the file.


def run_guard(cwd: pathlib.Path) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [sys.executable, str(ROOT / "scripts" / "check_pii.py")],
        cwd=cwd, capture_output=True, text=True,
    )


def test_scan_scope() -> None:
    """Three directions, in a throwaway repo so the real tree is never touched."""
    with tempfile.TemporaryDirectory() as tmp:
        repo = pathlib.Path(tmp) / "repo"
        (repo / "scripts").mkdir(parents=True)
        (repo / "docs").mkdir()
        subprocess.run(["git", "init", "-q"], cwd=repo, check=True)
        (repo / ".gitignore").write_text("ignored/\n", encoding="utf-8")
        (repo / "scripts" / "check_pii.py").write_text(
            (ROOT / "scripts" / "check_pii.py").read_text(encoding="utf-8"), encoding="utf-8"
        )
        subprocess.run(["git", "add", "-A"], cwd=repo, check=True)
        subprocess.run(
            ["git", "-c", "user.email=t@t", "-c", "user.name=t", "commit", "-qm", "init"],
            cwd=repo, check=True,
        )

        clean = run_guard(repo)
        check("throwaway repo starts green", clean.returncode == 0, clean.stdout + clean.stderr)

        # 1. UNTRACKED new file — the defect. This passed silently before the
        #    ls-files union fix.
        leak = repo / "docs" / "new_note.md"
        leak.write_text("My UTR is 4820571963 this year.\n", encoding="utf-8")
        untracked = run_guard(repo)
        check(
            "an UNTRACKED new file is scanned",
            untracked.returncode == 1 and "new_note.md" in untracked.stderr,
            "this is the realistic leak path and it passed silently before the fix",
        )
        check(
            "the failure does not print the matched value",
            "4820571963" not in untracked.stderr and "4820571963" not in untracked.stdout,
        )

        # 2. Remove it — back to green.
        leak.unlink()
        check("removing the value returns to green", run_guard(repo).returncode == 0)

        # 3. A gitignored path must stay invisible, or build output makes the
        #    guard noisy and it gets bypassed.
        (repo / "ignored").mkdir()
        (repo / "ignored" / "build.md").write_text("UTR 4820571963\n", encoding="utf-8")
        check("a gitignored path is NOT scanned", run_guard(repo).returncode == 0)


test_scan_scope()

# ── Structural exemptions stay small ────────────────────────────────────────
# An exemption list that can grow quietly is a slower way of deleting the
# check, so its exact contents are pinned.

check(
    "SELF_EXEMPT holds only the checker and its tests",
    check_pii.SELF_EXEMPT == {"scripts/check_pii.py", "scripts/test_check_pii.py"},
)
check(
    "SKIPPED_FILES holds only the lockfile",
    check_pii.SKIPPED_FILES == {"package-lock.json"},
)
check(
    "the dropped rules stay dropped",
    {r.ident for r in check_pii.RULES} == {"utr", "ni-number", "iban", "sort-code"},
    "bare-8-digit and GBP-in-prose were measured at 28 and 65 false positives; "
    "re-adding one needs a fresh measurement, not a guess",
)
check(
    "the anchored rules still carry their anchors",
    all(
        next(r for r in check_pii.RULES if r.ident == ident).anchor is not None
        for ident in ("utr", "sort-code")
    ),
    "dropping an anchor is what turns these into 28-false-positive rules",
)

# ── Report ──────────────────────────────────────────────────────────────────

print(f"{PASSED} passed, {len(FAILED)} failed")
for failure in FAILED:
    print(f"  FAIL: {failure}")
sys.exit(1 if FAILED else 0)
