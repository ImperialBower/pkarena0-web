.PHONY: help build serve build-release clean install-playwright test test-ui

help:
	@echo "pkarena0-web — available targets:"
	@echo ""
	@echo "  build               wasm-pack dev build → www/pkg/"
	@echo "  build-release       wasm-pack release build (optimised)"
	@echo "  serve               dev build + python3 http.server on :8080"
	@echo "  clean               cargo clean + remove www/pkg/"
	@echo "  install-playwright  npm install + playwright install chromium"
	@echo "  test                dev build + playwright headless tests"
	@echo "  test-ui             dev build + playwright interactive UI"

build:
	wasm-pack build --target web --out-dir www/pkg

serve: build
	@echo "Serving at http://localhost:8080"
	cd www && python3 -m http.server 8080

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
