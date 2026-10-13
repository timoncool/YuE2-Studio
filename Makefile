.PHONY: help backend frontend open

# Default target
help: ## Display this help message
	@awk 'BEGIN {FS = ":.*##"; printf "\nUsage:\n  make \033[36m<target>\033[0m\n"} /^[a-zA-Z_-]+:.*?##/ { printf "  \033[36m%-15s\033[0m %s\n", $$1, $$2 } /^##@/ { printf "\n\033[1m%s\033[0m\n", substr($$0, 5) } ' $(MAKEFILE_LIST)

##@ Application

backend: ## Start the Rust music-server backend (127.0.0.1:8791)
	cargo run -p music-server

frontend: ## Start the Next/Vite frontend UI (127.0.0.1:3791)
	npm --prefix app run dev

##@ Utilities

open: ## Open the UI in your Windows browser from WSL
	@if command -v wslview >/dev/null 2>&1; then \
		wslview http://127.0.0.1:3791; \
	else \
		powershell.exe -c "Start-Process http://127.0.0.1:3791"; \
	fi