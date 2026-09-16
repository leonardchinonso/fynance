#!/bin/sh
# Points git at .githooks/ so the pre-push PII scan runs automatically.
#
# One `git config` call does the whole job, rather than taking a hook-manager
# dependency: git runs a hooks-path script directly with `sh` on both Windows
# (Git for Windows ships a POSIX shell) and Linux, so there is nothing else
# to install and nothing to branch on per platform.
#
# WHY THIS IS A SHELL SCRIPT RATHER THAN AN npm `prepare` HOOK.
#
# The reference implementation this method is copied from wires the installer
# to `prepare`, so it fires the moment `npm install` does. That works there
# because the repository is a single npm package. Here it would only fire for
# someone who runs `npm install` inside `frontend/` -- and a contributor
# working on the Rust backend, or on the docs, may never do that. Those are
# exactly the people editing the planning documents where a real value gets
# pasted, so hanging the guard off a frontend dependency install would miss
# the likeliest case.
#
# The scan itself has the same property by design: it needs only python3 and
# the source tree, so it does not care whether node_modules exists.
#
# Deliberately a no-op rather than a failure when there is no .git to
# configure (a tarball, a container build stage), for the same reason the
# hook treats could-not-run as distinct from failed.

set -e

if ! git rev-parse --git-dir >/dev/null 2>&1; then
  echo "install-git-hooks: not a git checkout, skipping (nothing to configure)"
  exit 0
fi

if git config core.hooksPath .githooks; then
  echo "install-git-hooks: core.hooksPath -> .githooks (pre-push runs the PII scan)"
  echo "install-git-hooks: verify with 'git config core.hooksPath'"
else
  echo "install-git-hooks: could not set core.hooksPath; the pre-push scan will not run locally."
  echo "install-git-hooks: this is not fatal -- CI runs the same check on every PR."
  exit 0
fi
