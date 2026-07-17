.PHONY: help build serve kill build-release clean install-playwright test test-ui validate-bots bench-tiers ayce default

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
	@echo "  validate-bots       parse + validate embedded data/bots/*.yaml"
	@echo "  bench-tiers         chips/100 ordering bench: weak < standard < strong"
	@echo "  test                validate-bots + dev build + playwright tests"
	@echo "  test-ui             dev build + playwright interactive UI"

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

# Parse + validate the embedded bot-lineup YAML (EPIC-49). Fails the build if a
# profile can't deserialize or any bundle drifts from its code pool.
validate-bots:
	cargo test --lib bot_bundle

# EPIC-49 Phase 3 acceptance bench: seeded-decider (entropy-dealt) matchups
# asserting the chips/100 ordering weak < standard < strong with statistical
# margin. Release mode; ~15s. Not in the default fast suite — the deal RNG is
# entropy (pkcore has no seeded deck), so this is a bench, not a unit test.
bench-tiers:
	cargo test --release --lib difficulty_ordering -- --ignored --nocapture --test-threads=2

test: validate-bots build
	npx playwright test

test-ui: build
	npx playwright test --ui

# All You Can Eat - clean, build, and test
ayce: clean build test
