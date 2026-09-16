# The PII guard

This repository is **public**, and its planning docs, fixtures and notes are
written about one person's real tax affairs. The guard exists because the
failure mode is *forgetting*, not disagreeing: a value gets pasted into a
document to work something out, the document gets committed, and nothing
notices. That has already happened once here — a salary figure was committed
in prose and was caught only because a human happened to read a chat message
mentioning it.

It runs in two places, from the same script:

- **Locally, on `git push`** — `.githooks/pre-push`
- **In CI, on every PR** — the `pii-guard` job in `.github/workflows/ci.yml`

CI is the enforcement; the hook is the fast copy that saves you a round-trip.
An unwired or bypassed hook therefore does not get anything through.

## Install the hook

```sh
sh scripts/install_git_hooks.sh
```

That runs one command — `git config core.hooksPath .githooks` — and you can
run it yourself instead. Verify with `git config core.hooksPath`.

It is a script rather than an npm `prepare` hook on purpose. A `prepare` hook
only fires for someone who runs `npm install` inside `frontend/`, and a
contributor working on the Rust backend or on the docs may never do that —
yet those are exactly the people editing the planning documents where a real
value gets pasted. The scan needs only `python3` and the source tree, so it
does not care whether `node_modules` exists.

## Run it by hand

```sh
python3 scripts/check_pii.py            # the whole working tree
python3 scripts/check_pii.py a.md b.rs  # just these files
python3 scripts/test_check_pii.py       # prove the rules still fire
```

## What it looks for

Four shapes, each measured against this repository rather than reasoned about
in the abstract:

| Rule | Shape | Anchored? |
|------|-------|-----------|
| `utr` | ten digits | yes — a UTR keyword within 40 characters |
| `ni-number` | HMRC's issuing rules (letter classes + never-issued pairs) | no — specific enough alone |
| `iban` | country code, check digits, BBAN | no — specific enough alone |
| `sort-code` | `NN-NN-NN` | yes — a sort-code keyword within 40 characters |

**It matches shapes, never values.** No real number appears in the checker, and
none may be added to it. A denylist of the actual values would publish exactly
what the rule protects — a denylist wearing a regular expression as a costume
is the same mistake with extra steps. This is also why a test suite can exist
at all: every value in `scripts/test_check_pii.py` is invented, and the rules
can be proven to fire because they match form rather than content.

**The anchors are what make it usable.** Ten bare digits are also timestamps
and ids; `NN-NN-NN` is also a date and a version range. Requiring the keyword
nearby is the difference between a rule with zero false positives and one with
dozens.

## What a green run does and does NOT prove

**A green run means no text in this repository is shaped like those four
identifiers. It does not mean the repository is free of personal
information.** Those are different claims and only the first is tested.

All of the following pass clean, and you should not read a green tick as
saying otherwise:

- **A bare number in a sentence.** "My salary is 87,400" has no shape that
  distinguishes it from any other number in a document about money. This is
  the single most likely thing to leak, it is exactly what the original
  incident was, and this check cannot see it.
- **A proper noun** — an employer, an accountant, a bank, an address. No shape
  matches a name.
- **An account number on its own.** Bare 8-digit runs were measured and
  dropped (see below), so one sitting in prose without context passes.
- **Anything in a file the scan does not read** — binary, oversized, or
  exempt. The run prints how many files it skipped rather than reporting only
  success, because a check that cannot distinguish "this is clean" from "I
  never looked at this" is worse than no check.

So it is a backstop, not a proof. **Reading the diff is not something a green
tick discharges.** The guard exists to catch the four mechanical cases so that
attention is free for the ones it cannot catch.

## Rules that were measured and dropped

A guard that cries wolf gets disabled, which is strictly worse than no guard.
Two shapes from the original design were measured against the real tree and
dropped:

- **Bare 8-digit runs (account numbers): ~28 matches, effectively all false
  positives** — test fixtures, API research docs, pricing code. Unlike the
  UTR, no keyword reliably sits near an account number in this corpus, so it
  cannot be anchored.
- **£-prefixed 5–6 figure amounts in prose: ~65 matches**, in a repository
  whose documentation is *about money*. Unusable as a blocking rule.

They are recorded so the next person to propose them can see it was tried and
what it cost. Re-adding one needs a fresh measurement, not a guess.

## The measured false-positive rate

**Zero, across 464 scanned files.** With the allowlist emptied, the guard
fires on exactly one line in the repository: the real UTR in
`docs/plans/23_capital_gains_post_v0.md`. That single result is both the
false-positive measurement and the proof the rule would have caught the actual
leak.

Two matches were resolved structurally rather than with allowlist entries,
because the reason was structural in both cases:

- `frontend/package-lock.json` — a base64 SHA-512 integrity hash happened to
  contain an IBAN-shaped run. Lockfiles are generated and prose-free, so
  nobody can leak a value into one; it is in `SKIPPED_FILES`.
- `scripts/check_pii.py` and `scripts/test_check_pii.py` — the file that
  defines the shapes and the file that proves they are caught necessarily
  contain them. Both are in `SELF_EXEMPT`, and a test pins that list so it
  cannot grow quietly.

## Allowlisting, and why it is a last resort

An entry in `scripts/pii_allowlist.txt` is a **permanent hole** scoped to one
path and one rule. It has to be *earned* by measurement: delete it, re-run the
check, and confirm the match it was excusing is real and unavoidable. An entry
that excuses nothing is worse than no entry, because it will silently wave
through a genuine value added to that path later. One sort-code entry was
written during development, proved to excuse nothing — the keyword anchor
already suppressed the CSV fixtures on its own — and was removed for exactly
that reason.

Before adding one, check whether the right fix is structural instead:

- an obviously invented value → widen `PLACEHOLDER_PATTERNS`
- a generated file with no prose → add it to `SKIPPED_FILES`
- the checker's own source or tests → already in `SELF_EXEMPT`

Those are better because they state *why* in a way that keeps applying to
files nobody has written yet.

There is currently **one** entry, and it is temporary — see the comment in the
file. It covers the real UTR still on `master`, which a sibling PR scrubs; it
must be deleted when that lands.

## The matched value is never printed

A failure reports `path:line:column` and the rule name. It does **not** print
what it matched, and must not be changed to. CI logs for a public repository
are themselves public, so a guard announcing the value it found would perform
the exact exposure it exists to prevent — and in a more durable, more indexed
place than the original leak. The line number is enough to find it locally.

## Bypassing

```sh
git push --no-verify
```

Emergency use only, and it buys you nothing but time: CI runs the same check
on the resulting PR. The `pii-guard` job is deliberately unconditional — no
path filter, no `needs:` — because a path-filtered job renders green when it
is *skipped*, which for a guard would mean the one PR adding a leaking file
could show a green tick for a scan that never ran.
