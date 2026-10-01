.PHONY: help build windows runtime dist test test-parity fmt fmt-check lint check ci install clean \
        bump bump-patch bump-minor bump-major

.DEFAULT_GOAL := help

# Everything builds inside Docker through ./dev.sh: neither Rust nor Python has to be installed.
LINUX_BIN   := target/release/s1grep
WINDOWS_BIN := target/x86_64-pc-windows-gnu/release/s1grep.exe
INSTALL_DIR ?= $(HOME)/.local/bin

# ONNX Runtime for Windows: s1grep.exe loads this DLL from its own folder. Linux links the same version statically.
ORT_VERSION := 1.28.0
ORT_DLL     := .cache/onnxruntime/$(ORT_VERSION)/onnxruntime.dll
ORT_URL     := https://github.com/microsoft/onnxruntime/releases/download/v$(ORT_VERSION)/onnxruntime-win-x64-$(ORT_VERSION).zip

# rustfmt is added to the image as root; the files are handed back to the user afterwards.
UID_GID     := $(shell id -u):$(shell id -g)
RUST_ROOT   := docker run --rm -v "$(CURDIR)":/p -w /p rust:1-trixie sh -c

GREEN  := \033[0;32m
YELLOW := \033[1;33m
CYAN   := \033[0;36m
RED    := \033[0;31m
NC     := \033[0m

CURRENT_VERSION = $(shell sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\(.*\)"/\1/p' Cargo.toml)
DIST            = dist/$(CURRENT_VERSION)

help:
	@echo "$(GREEN)s1grep — Development Commands$(NC)\n"
	@awk 'BEGIN {FS = ":.*?## "} /^[a-zA-Z_-]+:.*?## / \
	    {printf "  $(YELLOW)%-14s$(NC) %s\n", $$1, $$2}' $(MAKEFILE_LIST)
	@echo "\n$(CYAN)Current version: $(CURRENT_VERSION)$(NC)"

build: ## Build the Linux binary
	./dev.sh cargo build --release -p s1grep

windows: ## Cross-compile the Windows binary
	./dev.sh windows build --release -p s1grep

runtime: $(ORT_DLL) ## Download onnxruntime.dll for the Windows build

$(ORT_DLL):
	@mkdir -p $(dir $(ORT_DLL))
	docker run --rm -u $(UID_GID) -v "$(CURDIR)/$(dir $(ORT_DLL))":/out python:3.13-bookworm python -c "\
		import io, urllib.request, zipfile; \
		archive = zipfile.ZipFile(io.BytesIO(urllib.request.urlopen('$(ORT_URL)').read())); \
		member = next(name for name in archive.namelist() if name.endswith('lib/onnxruntime.dll')); \
		open('/out/onnxruntime.dll', 'wb').write(archive.read(member))"

dist: build windows runtime ## Pack the release archives and SHA256SUMS into dist/<version>
	@rm -rf $(DIST) && mkdir -p $(DIST)
	@for platform in x86_64-linux x86_64-windows; do \
	    folder=$(DIST)/s1grep-$(CURRENT_VERSION)-$$platform; \
	    mkdir -p $$folder/licenses; \
	    cp README.md LICENSE packaging/NOTICE.txt $$folder/; \
	    cp packaging/licenses/* $$folder/licenses/; \
	 done
	@install -m 755 $(LINUX_BIN) $(DIST)/s1grep-$(CURRENT_VERSION)-x86_64-linux/s1grep
	@cp $(WINDOWS_BIN) $(ORT_DLL) $(DIST)/s1grep-$(CURRENT_VERSION)-x86_64-windows/
	@cd $(DIST) && tar -czf s1grep-$(CURRENT_VERSION)-x86_64-linux.tar.gz s1grep-$(CURRENT_VERSION)-x86_64-linux \
	    && python3 -m zipfile -c s1grep-$(CURRENT_VERSION)-x86_64-windows.zip s1grep-$(CURRENT_VERSION)-x86_64-windows \
	    && rm -rf s1grep-$(CURRENT_VERSION)-x86_64-linux s1grep-$(CURRENT_VERSION)-x86_64-windows \
	    && sha256sum s1grep-* > SHA256SUMS
	@echo "$(GREEN)Release files in $(DIST):$(NC)" && ls -lh $(DIST)

test: ## Run the unit tests
	./dev.sh cargo test --release --workspace --lib --bins

test-parity: ## Run the parity tests against the Python reference (needs models/)
	./dev.sh cargo test --release --workspace

fmt: ## Format the code
	$(RUST_ROOT) "rustup component add rustfmt >/dev/null 2>&1 && cargo fmt --all && chown -R $(UID_GID) crates"

fmt-check: ## Check formatting without changes
	$(RUST_ROOT) "rustup component add rustfmt >/dev/null 2>&1 && cargo fmt --all --check"
	@bash -n install.sh && bash -n update.sh

lint: ## Run clippy
	./dev.sh cargo clippy --release --workspace --all-targets

check: fmt-check ## Format check

ci: check test ## Full CI pipeline locally

install: build ## Install the Linux binary into ~/.local/bin (INSTALL_DIR=... to change)
	@mkdir -p $(INSTALL_DIR)
	install -m 755 $(LINUX_BIN) $(INSTALL_DIR)/s1grep
	@echo "$(GREEN)Installed $(INSTALL_DIR)/s1grep $(CURRENT_VERSION)$(NC)"

clean: ## Remove build artifacts
	rm -rf target dist

bump-patch: ## Bump patch version (0.2.0 → 0.2.1) and release
	@CURRENT=$(CURRENT_VERSION) ; \
	 MAJOR=$$(echo $$CURRENT | cut -d. -f1) ; \
	 MINOR=$$(echo $$CURRENT | cut -d. -f2) ; \
	 PATCH=$$(echo $$CURRENT | cut -d. -f3) ; \
	 NEW="$$MAJOR.$$MINOR.$$((PATCH+1))" ; \
	 echo "$(CYAN)Bumping $$CURRENT → $$NEW$(NC)" ; \
	 $(MAKE) bump VERSION=$$NEW

bump-minor: ## Bump minor version (0.2.0 → 0.3.0) and release
	@CURRENT=$(CURRENT_VERSION) ; \
	 MAJOR=$$(echo $$CURRENT | cut -d. -f1) ; \
	 MINOR=$$(echo $$CURRENT | cut -d. -f2) ; \
	 NEW="$$MAJOR.$$((MINOR+1)).0" ; \
	 echo "$(CYAN)Bumping $$CURRENT → $$NEW$(NC)" ; \
	 $(MAKE) bump VERSION=$$NEW

bump-major: ## Bump major version (0.2.0 → 1.0.0) and release
	@CURRENT=$(CURRENT_VERSION) ; \
	 MAJOR=$$(echo $$CURRENT | cut -d. -f1) ; \
	 NEW="$$((MAJOR+1)).0.0" ; \
	 echo "$(CYAN)Bumping $$CURRENT → $$NEW$(NC)" ; \
	 $(MAKE) bump VERSION=$$NEW

bump: ## Release a specific version: make bump VERSION=1.2.0
	@[ "$(VERSION)" != "" ] || { echo "$(RED)Set VERSION: make bump VERSION=1.2.0$(NC)"; exit 1; }
	@grep -q "## \[$(VERSION)\]" CHANGELOG.md || { \
	    echo "$(RED)Add ## [$(VERSION)] section to CHANGELOG.md before releasing$(NC)"; exit 1; }
	@git fetch -q origin main
	@[ -z "$$(git rev-list origin/main..HEAD)" ] || { \
	    echo "$(RED)Local branch is ahead of origin/main; a release must add nothing but its own bump:$(NC)" ; \
	    git log --oneline origin/main..HEAD ; \
	    echo "$(RED)Push or drop those commits, then release.$(NC)" ; exit 1 ; }
	@git diff --quiet -- . ':!Cargo.toml' ':!Cargo.lock' && git diff --cached --quiet -- . ':!Cargo.toml' ':!Cargo.lock' || { \
	    echo "$(RED)There are changes that are not committed; the tag would build without them:$(NC)" ; \
	    git status --short -- . ':!Cargo.toml' ':!Cargo.lock' ; \
	    echo "$(RED)Commit and push them, then release.$(NC)" ; exit 1 ; }
	sed -i '/^\[workspace.package\]/,/^\[/s/^version = .*/version = "$(VERSION)"/' Cargo.toml
	for crate in s1-engine s1-index s1grep; do \
	    sed -i "/^name = \"$$crate\"$$/{n;s/^version = .*/version = \"$(VERSION)\"/}" Cargo.lock ; \
	done
	git add Cargo.toml Cargo.lock
	git diff --cached --quiet || git commit -m "chore: bump s1grep to $(VERSION)"
	git tag $(VERSION)
	git push origin HEAD:main
	git push origin $(VERSION)
	@echo "$(GREEN)✓ s1grep $(VERSION) tagged — CI will build and publish the release$(NC)"
