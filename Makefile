# Legend — build & install (Linux / WSL).
# Native Windows is a separate port (see docs/production-roadmap.md, W1).
#
#   make            strict, sha-stamped build -> ./legend
#   make dev        fast non-strict build for iteration
#   make install    build + install binary and model under $(PREFIX) (default ~/.local)
#   make test       run the full gate (check.sh)
#   make uninstall  remove the installed binary + model
#
# The model is installed NEXT TO the binary ($(BIN)/models/...), which the embedder
# self-locates via its binary-relative path — so no LEGEND_EMBED_DIR is needed and
# nothing hardcodes a home directory. (LEGEND_EMBED_DIR still overrides if set.)

PREFIX  ?= $(HOME)/.local
BIN      := $(PREFIX)/bin
MODELDIR := $(BIN)/models/bge-small-en-v1.5
CC      ?= cc
SHA      := $(shell git rev-parse --short HEAD 2>/dev/null || echo unknown)
CFLAGS  ?= -std=c99 -Wall -Wextra -Werror -O2 -DLEGEND_BUILD="\"$(SHA)\""
SRC      := legend.c embed.c
SRCMODEL := models/bge-small-en-v1.5

.PHONY: all dev install uninstall test clean

all: legend

legend: $(SRC) embed.h
	$(CC) $(CFLAGS) $(SRC) -o legend -lm

dev:
	$(CC) -std=c99 -O2 -DLEGEND_BUILD="\"$(SHA)-dev\"" $(SRC) -o legend -lm

install: legend
	install -d "$(BIN)" "$(MODELDIR)"
	install -m755 legend "$(BIN)/legend"
	install -m644 "$(SRCMODEL)/minilm.int8.bin" "$(MODELDIR)/minilm.int8.bin"
	install -m644 "$(SRCMODEL)/vocab.txt"        "$(MODELDIR)/vocab.txt"
	@echo ""
	@echo "installed: $(BIN)/legend  (build $(SHA))"
	@echo "     model: $(MODELDIR)"
	@echo "ensure $(BIN) is on PATH, then run 'legend init' inside a project."

uninstall:
	rm -f "$(BIN)/legend"
	rm -rf "$(MODELDIR)"

test:
	./check.sh

clean:
	rm -f legend legend.asan legend_test embed_test
