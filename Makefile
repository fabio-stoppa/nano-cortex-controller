.DEFAULT_GOAL := help

PORT ?= $(NANO_PORT)
BAUD ?= $(if $(NANO_BAUD),$(NANO_BAUD),921600)
HOST_TARGET ?= x86_64-unknown-linux-gnu

.PHONY: help build flash run monitor preview preview-portrait test test-protocol test-midi test-ui test-all

help: ## Show available commands
	@awk 'BEGIN { FS = ":.*## "; print "Usage: make <target> [PORT=/dev/ttyACM0] [BAUD=921600]\n" } /^[a-z-]+:.*## / { printf "  %-18s %s\n", $$1, $$2 }' $(MAKEFILE_LIST)
	@printf '\nPORT defaults to NANO_PORT or automatic detection; BAUD defaults to NANO_BAUD or 921600.\nHOST_TARGET defaults to x86_64-unknown-linux-gnu for host tests.\n'

build: ## Build release firmware (including BLE dependency patches)
	./flash.sh build

flash: ## Flash the existing release build without rebuilding
	NANO_BAUD="$(BAUD)" ./flash.sh flash release "$(PORT)"

run: ## Build, flash, and monitor
	NANO_BAUD="$(BAUD)" ./flash.sh run release "$(PORT)"

monitor: ## Open the serial monitor (close it before flashing)
	./flash.sh monitor "$(PORT)"

preview: ## Open the live landscape desktop UI preview
	./firmware/preview.sh

preview-portrait: ## Open the live portrait desktop UI preview
	./firmware/preview.sh portrait

test: test-protocol test-midi ## Run host protocol and MIDI/scene/connection tests (no UI renderer)

test-protocol: ## Run host protocol tests
	cargo +stable test --manifest-path crates/nano-wire/Cargo.toml --target "$(HOST_TARGET)"

test-midi: ## Run host MIDI, scene, settings, and connection tests
	cargo +stable test --manifest-path firmware/tests/midi/Cargo.toml --target "$(HOST_TARGET)"

test-ui: ## Explicitly run the UI renderer regression tests
	cargo +stable test --manifest-path firmware/tests/ui-renderer/Cargo.toml --target "$(HOST_TARGET)"

test-all: test test-ui ## Run all host tests, including the UI renderer
