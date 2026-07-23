.PHONY: help build serve kill build-release clean install-playwright test test-ui validate-bots validate-okf bench-tiers ayce default

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
	@echo "  validate-okf        check the .okf/ knowledge bundle is OKF-conformant"
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

# Check the .okf/ knowledge bundle for OKF v0.1 conformance (§9). Fails only on
# hard errors (unparseable frontmatter / missing `type`); cross-links into docs/
# are out-of-bundle by design and stay warnings, so --strict is intentionally
# omitted. Needs `uv` (auto-installs the checker's pyyaml dep via PEP-723).
validate-okf:
	uv run scripts/okf_validate.py .okf

# EPIC-49/50 acceptance bench: entropy-dealt matchups asserting chips/100
# ordering weak < standard < strong from the real bundle pools. Release mode;
# the strong tier runs the equity engine (500-sample MC per postflop decision),
# so the strong-vs-standard leg is ~5 min (standard-vs-weak ~65 s). Not in the
# default fast suite — the deal RNG is entropy (pkcore has no seeded deck).
bench-tiers:
	cargo test --release --lib difficulty_ordering -- --ignored --nocapture --test-threads=2

test: validate-bots build
	npx playwright test

test-ui: build
	npx playwright test --ui

# All You Can Eat - clean, build, and test
ayce: clean build test
