# FluxMirror — developer Makefile
#
# Targets:
#   make sync-helpers   Copy scripts/_dual_write.py into both hook packages.
#                       Run after editing the canonical helper.
#   make verify-helpers Fail if any package copy diverged from canonical.
#                       Used in CI to prevent silent drift.
#   make help           List available targets.

CANONICAL := scripts/_dual_write.py
TARGETS   := plugins/fluxmirror/hooks/_dual_write.py gemini-extension/hooks/_dual_write.py

.PHONY: help sync-helpers verify-helpers

help:
	@echo "FluxMirror Makefile"
	@echo ""
	@echo "  make sync-helpers   Copy $(CANONICAL) into both packages"
	@echo "  make verify-helpers Fail if any package copy diverged"

sync-helpers:
	@for t in $(TARGETS); do \
	  cp $(CANONICAL) $$t && echo "  synced -> $$t"; \
	done

verify-helpers:
	@expected=$$(shasum -a 256 $(CANONICAL) | awk '{print $$1}'); \
	fail=0; \
	for t in $(TARGETS); do \
	  actual=$$(shasum -a 256 $$t | awk '{print $$1}'); \
	  if [ "$$expected" != "$$actual" ]; then \
	    echo "DRIFT: $$t differs from canonical $(CANONICAL)"; \
	    fail=1; \
	  else \
	    echo "OK:    $$t matches canonical"; \
	  fi; \
	done; \
	exit $$fail
