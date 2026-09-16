#!/usr/bin/env python3
"""Fails when a file in this repository contains something shaped like a real
UK financial identifier: a UTR, a National Insurance number, a sort code or an
IBAN.

This repository is PUBLIC and its docs are about one person's actual tax
affairs, so the working notes, plans and fixtures in it are written next to
real numbers. The failure mode is not disagreement, it is forgetting: a value
gets pasted into a planning doc to work something out, the doc gets committed,
and nothing notices. That has already happened here once — a salary figure was
committed in prose and caught only because a human happened to read a message
mentioning it. A gate is the only version of the rule that survives the tenth
pull request.

── How it matches: SHAPES, NEVER VALUES ────────────────────────────────────

Every pattern below is a *grammatical shape* — ten digits near the word "UTR",
two letters and six digits and a suffix letter. None of them is, or may become,
a list of the real numbers that must stay out of this repository. Writing those
in "so they can be grepped for" publishes precisely what the rule exists to
keep out, and a denylist wearing a regular expression as a costume is the same
mistake with extra steps. If you find yourself pasting one of the real values
into this file, stop: the answer is a shape, or nothing.

This is also why the test suite can exist at all. Every value in
`scripts/test_check_pii.py` is invented, and it can prove the rules fire
because the rules match form rather than content. A value-denylist guard could
not have a test suite without publishing the thing it protects.

── The matched text is never printed ───────────────────────────────────────

A failure reports `path:line:column` and the rule name. It does NOT print the
value it matched, and must not be changed to. CI logs for a public repository
are themselves public, and a guard that announces "found the UTR 1234567890 at
line 12" has performed the exact exposure it exists to prevent — more
efficiently than the leak would have, because now it is in a log that is
indexed and kept. The line number is enough to find it locally.

── What a green run does and does NOT prove ────────────────────────────────

**A green run means no text in this repository is SHAPED like the four
identifiers below. It does not mean the repository is free of personal
information.** Those are different claims and only the first is tested here.
Specifically, all of these pass clean:

  - **A bare number in a sentence.** "My salary is 87,400" has no shape that
    distinguishes it from any other number in a document about money. This is
    the single most likely thing to leak and this check cannot see it. It is
    also exactly what the incident that prompted this check was.
  - **A proper noun.** An employer, an accountant, a bank branch, an address.
    No shape matches a name.
  - **A number the check deliberately does not look for.** Bare 8-digit
    account numbers and £-prefixed figures were measured against this repo and
    dropped — see "Rules that were measured and dropped" below. An account
    number sitting on its own in a doc therefore passes.
  - **Anything in a file the scan does not read** — binary, oversized, or
    allowlisted. The run summary prints those counts rather than reporting
    only success, because a check that cannot tell "this is clean" from "I
    never looked at this" is worse than no check.

So this is a backstop, not a proof. **Reading the diff is not something a green
tick discharges.** This check exists to catch the four mechanical cases so that
attention is free for the ones it cannot.

── Rules that were measured and dropped ────────────────────────────────────

Two shapes from the original design were measured against the real tracked
tree and dropped, because a guard that cries wolf gets disabled, which is
strictly worse than no guard:

  - **Bare 8-digit runs (account numbers): ~28 matches, effectively all false
    positives** — test fixtures, API research docs, pricing code. There is no
    keyword that reliably sits near an account number in this corpus the way
    "UTR" does, so it cannot be anchored the way the UTR rule is.
  - **£-prefixed 5-6 figure amounts in prose: ~65 matches** in a repository
    whose documentation is *about money*. Unusable as a blocking rule.

Both are recorded here rather than silently omitted, so the next person to
propose them can see they were tried and what it cost.

── Recording a deliberate exception ────────────────────────────────────────

An allowlist entry is a hole in the guard and has to be EARNED by measurement,
not assumed. The rule for adding one: remove it, run the check, and confirm the
match it was excusing is real and unavoidable. An entry that excuses nothing is
worse than no entry at all, because it will silently wave through a genuine
value added to that path later. One was written during development, proved to
excuse nothing, and removed for exactly that reason.

Entries live in `scripts/pii_allowlist.txt`, one `path:rule` pair per line with
a mandatory comment saying why.

Usage:
    python3 scripts/check_pii.py            # every file in the working tree
    python3 scripts/check_pii.py a.md b.rs  # just these
"""

from __future__ import annotations

import pathlib
import re
import subprocess
import sys

def _repo_root() -> pathlib.Path:
    """The repository the scan is being run AGAINST, not the one this script
    happens to live in.

    Deriving the root from `__file__` makes the check silently scan its own
    checkout no matter where it is invoked, which is wrong in the two places
    it matters: a git worktree sharing the script, and any invocation from
    another tree. It "works" under the pre-push hook and in CI only because
    both happen to run from the repo root — a check that is correct by luck
    reports green having read a different repository than the one being
    pushed, which is the silent-success failure this guard exists to avoid.

    `git rev-parse --show-toplevel` asks git, which is the only thing that
    actually knows. Falling back to the script's own parent keeps the check
    runnable outside a git checkout (a tarball, a container build stage)
    rather than failing to start.
    """
    result = subprocess.run(
        ["git", "rev-parse", "--show-toplevel"], capture_output=True, text=True
    )
    if result.returncode == 0 and result.stdout.strip():
        return pathlib.Path(result.stdout.strip())
    return pathlib.Path(__file__).resolve().parent.parent


ROOT = _repo_root()
ALLOWLIST_PATH = ROOT / "scripts" / "pii_allowlist.txt"

# How far from a digit-run the anchoring keyword may sit. Wide enough for
# "UTR (Unique Taxpayer Reference): 1234567890" and a markdown table cell,
# narrow enough that a keyword in a heading does not reach the numbers in the
# paragraph below it.
CONTEXT_WINDOW = 40


class Rule:
    """One shape.

    `ident` is what a failure names, so it is short and greppable. `why` is
    what the author reads at 2am, so it says what to do rather than restating
    the rule. `anchor`, when present, is a keyword that must appear within
    CONTEXT_WINDOW characters of the match for it to count — this is what makes
    the difference between a usable rule and 28 false positives.
    """

    def __init__(self, ident: str, pattern: str, why: str, anchor: str | None = None):
        self.ident = ident
        self.regex = re.compile(pattern)
        self.why = why
        self.anchor = re.compile(anchor, re.IGNORECASE) if anchor else None


RULES = [
    Rule(
        "utr",
        # Ten digits, not part of a longer run. Anchored, because ten bare
        # digits are also timestamps, ids and phone numbers.
        r"(?<!\d)\d{10}(?!\d)",
        "looks like a Unique Taxpayer Reference next to the word UTR — replace "
        "it with a placeholder such as 1234567890, or move the real value out "
        "of the repository entirely",
        anchor=r"\butr\b|\bunique\s+taxpayer\b|\btaxpayer\s+reference\b",
    ),
    Rule(
        "ni-number",
        # AA 00 00 00 A. HMRC never issues D, F, I, Q, U or V as either of the
        # first two letters, nor O as the second, and never issues the pairs
        # BG, GB, KN, NK, NT, TN or ZZ. Both halves of that are encoded: the
        # character classes for the per-letter rule, the negative lookahead
        # for the pairs.
        #
        # This is still a SHAPE and not a denylist — these are HMRC's
        # published issuing rules, true of every NI number that has ever
        # existed, and they name nobody. Encoding them rather than writing
        # [A-Z]{2} is what stops this firing on every pair of capitals
        # followed by digits (a git sha fragment, an enum variant, a constant).
        #
        # The suffix is A-D; a trailing space before it is optional, as is
        # every internal separator, because the value gets written both ways.
        r"(?<![A-Z0-9])(?!BG|GB|KN|NK|NT|TN|ZZ)"
        r"[A-CEGHJ-PR-TW-Z][A-CEGHJ-NPR-TW-Z]\s?\d{2}\s?\d{2}\s?\d{2}\s?[A-D](?![A-Z0-9])",
        "looks like a National Insurance number — replace it with a clearly "
        "invented placeholder, or move the real value out of the repository",
    ),
    Rule(
        "iban",
        # Two-letter country code, two check digits, then the BBAN. Long
        # enough to be specific on its own, so no anchor is needed.
        r"(?<![A-Z0-9])[A-Z]{2}\d{2}[A-Z0-9]{11,30}(?![A-Z0-9])",
        "looks like an IBAN — replace it with a placeholder, or move the real "
        "value out of the repository",
    ),
    Rule(
        "sort-code",
        # NN-NN-NN. Anchored: this shape is also a date fragment, a version
        # range and a table rule in markdown.
        r"(?<!\d)\d{2}-\d{2}-\d{2}(?!\d)",
        "looks like a bank sort code next to a sort-code keyword — replace it "
        "with a placeholder such as 00-00-00",
        anchor=r"\bsort[\s_-]?code\b|\bsortcode\b",
    ),
]

# Values that are conventionally placeholders in this repository. These are
# NOT real values being denylisted — they are the invented ones the fixtures
# already use, listed so the guard does not fire on the very placeholders it
# tells people to use. A sequential or repeated-digit run is nobody's real
# identifier.
PLACEHOLDER_PATTERNS = [
    re.compile(r"^(\d)\1*$"),  # 0000000000, 1111111111
    re.compile(r"^1234567890$"),
    re.compile(r"^0123456789$"),
    re.compile(r"^9876543210$"),
    re.compile(r"^00-00-00$"),
    re.compile(r"^12-34-56$"),
]

BINARY_EXTENSIONS = {
    "png", "jpg", "jpeg", "gif", "ico", "webp", "pdf", "zip", "gz",
    "woff", "woff2", "ttf", "eot", "mp4", "mp3", "xlsx", "xls", "lock",
}

# Generated, enormous and prose-free. A lockfile is full of base64 integrity
# hashes, and a 60-character base64 blob will eventually contain two letters,
# two digits and eleven alphanumerics in a row purely by chance — which is
# exactly what the IBAN shape is. Measured on this repository: one such
# collision in `frontend/package-lock.json`, from a SHA-512 hash.
#
# Skipping the file is the right fix rather than an allowlist entry, because
# the reason is structural (nobody hand-writes a lockfile, so nobody can leak
# a value into one) rather than a judgement about a particular line. Named
# rather than matched by extension so that adding one is a visible change.
SKIPPED_FILES = {"package-lock.json"}

# Two files necessarily contain the shapes: the one that defines them, and the
# one that proves they are caught. A test suite that cannot trip the rule
# proves nothing, so `test_check_pii.py` must contain matching values — every
# one of them invented, which is precisely what shape-based matching buys you.
#
# Nothing else belongs in here, and a test asserts as much: a self-exemption
# list that can grow quietly is just a slower way of deleting the check.
SELF_EXEMPT = {"scripts/check_pii.py", "scripts/test_check_pii.py"}

# Anything larger than this is not prose anyone wrote by hand.
MAX_BYTES = 512 * 1024


def is_placeholder(text: str) -> bool:
    """Is this one of the invented values the repo's fixtures already use?"""
    stripped = text.replace(" ", "")
    return any(p.match(stripped) for p in PLACEHOLDER_PATTERNS)


def load_allowlist() -> dict[str, set[str]]:
    """Read `path:rule` pairs. Comments and blank lines ignored.

    A missing allowlist file is an empty allowlist, not an error — the guard
    is stricter without it, and failing to start because a hole-punching file
    is absent would be backwards.
    """
    allowed: dict[str, set[str]] = {}
    if not ALLOWLIST_PATH.exists():
        return allowed
    for raw in ALLOWLIST_PATH.read_text(encoding="utf-8").splitlines():
        line = raw.split("#", 1)[0].strip()
        if not line:
            continue
        path, _, rule = line.rpartition(":")
        if not path:
            continue
        allowed.setdefault(path.strip(), set()).add(rule.strip())
    return allowed


def find_violations(text: str) -> list[tuple[int, int, str]]:
    """Every match in one file's text, as `(line, column, rule_ident)`.

    The matched text is deliberately NOT returned. Nothing downstream can
    print what it never received, which is a stronger guarantee than
    remembering not to print it.
    """
    violations = []
    for number, line in enumerate(text.splitlines(), start=1):
        for rule in RULES:
            for match in rule.regex.finditer(line):
                if is_placeholder(match.group(0)):
                    continue
                if rule.anchor and not _anchored(line, match, rule):
                    continue
                violations.append((number, match.start() + 1, rule.ident))
    return sorted(violations)


def _anchored(line: str, match: re.Match[str], rule: Rule) -> bool:
    """Does the rule's keyword sit within CONTEXT_WINDOW chars of the match?

    Measured on this repository, the window is what makes the sort-code rule
    usable: `backend/tests/fixtures/lloyds.csv` carries a "Sort Code" header
    on one line and NN-NN-NN shapes on the data lines below, and because the
    keyword and the shape never co-occur on one line they never fire. That is
    the anchor doing the work, not an allowlist entry.
    """
    assert rule.anchor is not None
    start = max(0, match.start() - CONTEXT_WINDOW)
    end = min(len(line), match.end() + CONTEXT_WINDOW)
    return bool(rule.anchor.search(line[start:end]))


def scannable_paths() -> list[str]:
    """Every file in the working tree the repository would keep — tracked
    **and** untracked-but-not-ignored.

    **The untracked half is not optional.** This check exists to fail before
    CI does, and listing only tracked files made it reliably wrong in the one
    case that matters most: a file just written and not yet added. Write a new
    planning doc, paste a real value into it, run the check, get a green
    result from a scan that never opened the file. That is the realistic leak
    path, and it passed silently until this was fixed.

    A check that cannot tell "this is clean" from "I never looked at this" is
    the silent-success failure that makes a guard worse than none.

    `--others --exclude-standard` adds untracked files while honouring
    .gitignore, which is what keeps `frontend/dist` and `target/` out — so
    build output cannot make the guard noisy.
    """
    def listing(extra: list[str]) -> list[str]:
        result = subprocess.run(
            ["git", "ls-files", "-z", *extra],
            cwd=ROOT, capture_output=True, text=True,
        )
        if result.returncode != 0:
            raise SystemExit(f"git ls-files failed: {result.stderr.strip()}")
        return [p for p in result.stdout.split("\0") if p]

    # A path can appear in both listings in some index states; dedupe, or the
    # same violation is reported twice.
    seen = dict.fromkeys(listing([]) + listing(["--others", "--exclude-standard"]))
    return list(seen)


def is_scannable(path: str) -> bool:
    if path in SELF_EXEMPT:
        return False
    name = path.split("/")[-1]
    if name in SKIPPED_FILES:
        return False
    ext = name.rsplit(".", 1)[-1].lower() if "." in name else ""
    return ext not in BINARY_EXTENSIONS


def main(argv: list[str]) -> int:
    explicit = argv[1:]
    listed = explicit if explicit else scannable_paths()
    paths = [p for p in listed if is_scannable(p)]
    allowlist = load_allowlist()

    failures: list[str] = []
    scanned = 0
    allowed_count = 0

    for path in paths:
        full = ROOT / path
        try:
            if not full.is_file() or full.stat().st_size > MAX_BYTES:
                continue
            contents = full.read_text(encoding="utf-8", errors="replace")
        except OSError:
            continue  # deleted or unreadable between listing and reading

        scanned += 1
        permitted = allowlist.get(path, set())
        for line, column, ident in find_violations(contents):
            if ident in permitted or "*" in permitted:
                allowed_count += 1
                continue
            rule = next(r for r in RULES if r.ident == ident)
            # path:line:column and the rule name ONLY. The matched value is
            # never printed — see the module docstring.
            failures.append(f"{path}:{line}:{column}  [{ident}]\n    ↳ {rule.why}")

    skipped = len(listed) - scanned
    coverage = f"Scanned {scanned} of {len(listed)} files"
    if skipped > 0:
        coverage += f" ({skipped} skipped: binary, oversized, or missing)"
    allowed_note = (
        f" {allowed_count} match{'' if allowed_count == 1 else 'es'} allowlisted."
        if allowed_count
        else ""
    )

    if not failures:
        print(f"{coverage}: nothing is shaped like a UK financial identifier.{allowed_note}")
        print(
            "Note: a green run means no text matches the four shapes checked. It does NOT "
            "mean the repository is free of personal information — a bare number or a name "
            "in a sentence passes every rule here. See scripts/check_pii.py."
        )
        return 0

    print("\n\n".join(failures), file=sys.stderr)
    print(
        f"\n{len(failures)} value{'' if len(failures) == 1 else 's'} shaped like a UK "
        "financial identifier.\n\n"
        "This repository is PUBLIC. Replace the value with a clearly invented placeholder,\n"
        "or move it out of the repository entirely. The matched text is deliberately not\n"
        "printed above — CI logs are public too, so naming it here would repeat the exposure.\n"
        "Open the file at the line shown to see what matched.\n\n"
        "If a match is genuinely a placeholder the rules should recognise, prefer widening\n"
        "PLACEHOLDER_PATTERNS over an allowlist entry. An allowlist entry is a permanent\n"
        "hole scoped to one path and one rule, and has to be earned: remove it, re-run, and\n"
        "confirm it was excusing a real and unavoidable match.\n\n"
        "Emergency bypass for the hook only: git push --no-verify (CI still runs this).",
        file=sys.stderr,
    )
    return 1


if __name__ == "__main__":
    sys.exit(main(sys.argv))
