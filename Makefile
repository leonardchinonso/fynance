.PHONY: build dev-backend dev-frontend test lint fmt clean

# NOTE: every target below that touches `backend/` (dev-backend, test, lint,
# fmt) calls cargo directly and does NOT build the frontend first. On a
# fresh clone/worktree frontend/dist does not exist yet (it's gitignored),
# and cargo embeds it at compile time via include_dir!, so these will fail
# with a proc-macro panic until you've run `make build` (or
# `cd frontend && npm run build`) at least once. This is expected, not a
# broken repo -- see backend/RUNNING.md -> Troubleshooting for the exact
# error and fix. Once frontend/dist exists it persists on disk across
# backend rebuilds, so you only need to rebuild it when frontend source
# changes.
#
# If `cargo` is not on PATH in your shell, use the absolute path instead,
# e.g. ~/.cargo/bin/cargo build (see backend/RUNNING.md).

# Full build: frontend bundle first, then cargo picks it up via include_dir!
build:
	cd frontend && npm run build
	cd backend && cargo build --release

dev-backend:
	cd backend && cargo watch -x 'run -- serve --no-open'

dev-frontend:
	cd frontend && npm run dev

test:
	cd backend && cargo test

lint:
	cd backend && cargo clippy --all-targets -- -D warnings

fmt:
	cd backend && cargo fmt

clean:
	cd backend && cargo clean
	rm -rf frontend/dist frontend/node_modules
