PREFIX ?= /usr/local
BINDIR ?= $(PREFIX)/bin
DATADIR ?= $(PREFIX)/share
MANDIR ?= $(DATADIR)/man/man1
BASHCOMPDIR ?= /usr/share/bash-completion/completions
FISHCOMPDIR ?= /usr/share/fish/vendor_completions.d
ZSHCOMPDIR ?= /usr/share/zsh/site-functions
SYSCONFDIR ?= /etc/rinode
CARGO ?= cargo

.PHONY: all build release install install-user uninstall test clean whitepaper

all: build

build:
	$(CARGO) build --release

install: build
	install -d $(DESTDIR)$(BINDIR)
	install -m 755 target/release/rinode $(DESTDIR)$(BINDIR)/rinode
	install -d $(DESTDIR)$(MANDIR)
	install -m 644 man/rinode.1 $(DESTDIR)$(MANDIR)/rinode.1
	# Bash completions (modern vendor path + legacy fallback)
	install -d $(DESTDIR)$(BASHCOMPDIR)
	install -m 644 completions/rinode.bash $(DESTDIR)$(BASHCOMPDIR)/rinode
	install -d $(DESTDIR)/etc/bash_completion.d
	install -m 644 completions/rinode.bash $(DESTDIR)/etc/bash_completion.d/rinode
	# Fish completions
	install -d $(DESTDIR)$(FISHCOMPDIR)
	install -m 644 completions/rinode.fish $(DESTDIR)$(FISHCOMPDIR)/rinode.fish
	# Zsh completions
	install -d $(DESTDIR)$(ZSHCOMPDIR)
	install -m 644 completions/_rinode $(DESTDIR)$(ZSHCOMPDIR)/_rinode
	# System configuration
	install -d $(DESTDIR)$(SYSCONFDIR)
	test -f $(DESTDIR)$(SYSCONFDIR)/config.toml || install -m 644 rinode.toml $(DESTDIR)$(SYSCONFDIR)/config.toml
	@echo ""
	@echo "rinode systemwide installation complete."
	@echo "------------------------------------------------------------"
	@echo "To enable the 'r' shortcut and shell integration, add to your shell config:"
	@echo ""
	@echo "  Fish (~/.config/fish/config.fish):"
	@echo "    rinode init fish | source"
	@echo ""
	@echo "  Bash (~/.bashrc):"
	@echo "    eval \"\$$(rinode init bash)\""
	@echo ""
	@echo "  Zsh (~/.zshrc):"
	@echo "    eval \"\$$(rinode init zsh)\""
	@echo ""
	@echo "To also alias 'rm' to 'rinode rm', add --alias-rm to the command:"
	@echo "  rinode init fish --alias-rm | source"
	@echo "------------------------------------------------------------"

install-user: build
	install -d $(HOME)/.local/bin
	install -m 755 target/release/rinode $(HOME)/.local/bin/rinode
	install -d $(HOME)/.local/share/man/man1
	install -m 644 man/rinode.1 $(HOME)/.local/share/man/man1/rinode.1
	install -d $(HOME)/.config/fish/completions
	install -m 644 completions/rinode.fish $(HOME)/.config/fish/completions/rinode.fish
	install -d $(HOME)/.local/share/bash-completion/completions
	install -m 644 completions/rinode.bash $(HOME)/.local/share/bash-completion/completions/rinode
	install -d $(HOME)/.config/rinode
	test -f $(HOME)/.config/rinode/config.toml || install -m 644 rinode.toml $(HOME)/.config/rinode/config.toml
	@echo ""
	@echo "rinode installed locally to $(HOME)/.local/bin/rinode."
	@echo "------------------------------------------------------------"
	@echo "1. Ensure $(HOME)/.local/bin is in your PATH."
	@echo "2. Add shell integration to your shell configuration:"
	@echo ""
	@echo "  Fish (~/.config/fish/config.fish):"
	@echo "    rinode init fish | source"
	@echo ""
	@echo "  Bash (~/.bashrc):"
	@echo "    eval \"\$$(rinode init bash)\""
	@echo ""
	@echo "  Zsh (~/.zshrc):"
	@echo "    eval \"\$$(rinode init zsh)\""
	@echo ""
	@echo "To also alias 'rm' to 'rinode rm', add --alias-rm to the command:"
	@echo "  rinode init fish --alias-rm | source"
	@echo "------------------------------------------------------------"

uninstall:
	rm -f $(DESTDIR)$(BINDIR)/rinode
	rm -f $(DESTDIR)$(MANDIR)/rinode.1
	rm -f $(DESTDIR)$(BASHCOMPDIR)/rinode
	rm -f $(DESTDIR)/etc/bash_completion.d/rinode
	rm -f $(DESTDIR)$(FISHCOMPDIR)/rinode.fish
	rm -f $(DESTDIR)$(ZSHCOMPDIR)/_rinode
	@echo "rinode uninstalled."

test: build
	bash tests/integration_test.sh

clean:
	$(CARGO) clean

whitepaper:
	pdflatex -interaction=nonstopmode -output-directory=docs docs/whitepaper.tex
	pdflatex -interaction=nonstopmode -output-directory=docs docs/whitepaper.tex
