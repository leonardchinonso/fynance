.PHONY: build dev-backend dev-frontend test lint fmt clean

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
