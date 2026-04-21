.PHONY: help build serve kill build-release clean install-playwright test test-ui test-yaml ayce default

# Default target
default: ayce

help:
	@echo "pkarena0-web — available targets:"
	@echo ""
	@echo "  build               wasm-pack dev build → www/pkg/"
	@echo "  build-release       wasm-pack release build (optimised)"
	@echo "  serve               dev build + python3 http.server on :8080"
	@echo "  kill                kill the http.server on :8080"
	@echo "  clean               cargo clean + remove www/pkg/"
	@echo "  install-playwright  npm install + playwright install chromium"
	@echo "  test                dev build + playwright headless tests"
	@echo "  test-ui             dev build + playwright interactive UI"
	@echo "  test-yaml           browser download test + pkcore YAML validation"

build:
	wasm-pack build --target web --out-dir www/pkg

serve: build
	@echo "Serving at http://localhost:8080"
	cd www && python3 -m http.server 8080

kill:
	@lsof -ti :8080 | xargs kill 2>/dev/null || echo "Nothing running on :8080"

build-release:
	wasm-pack build --release --target web --out-dir www/pkg

clean:
	cargo clean
	rm -rf www/pkg

install-playwright:
	npm install
	npx playwright install chromium

test: build
	npx playwright test

test-ui: build
	npx playwright test --ui

test-yaml: build
	npx playwright test tests/yaml-download.spec.ts
	cargo run --bin validate-yaml -- tests/fixtures/session.yaml

# All You Can Eat - clean, build, and test
ayce: clean build test
