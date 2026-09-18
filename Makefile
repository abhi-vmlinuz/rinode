PREFIX ?= /usr/local
BINDIR ?= $(PREFIX)/bin
DATADIR ?= $(PREFIX)/share
MANDIR ?= $(DATADIR)/man/man1
BASHCOMPDIR ?= /usr/share/bash-completion/completions
FISHCOMPDIR ?= /usr/share/fish/vendor_completions.d
ZSHCOMPDIR ?= /usr/share/zsh/site-functions
SYSCONFDIR ?= /etc/rinode
CARGO ?= $(shell which cargo 2>/dev/null || if [ -n "$$SUDO_USER" ] && [ -x "/home/$$SUDO_USER/.cargo/bin/cargo" ]; then echo "/home/$$SUDO_USER/.cargo/bin/cargo"; elif [ -x "$$HOME/.cargo/bin/cargo" ]; then echo "$$HOME/.cargo/bin/cargo"; else echo cargo; fi)

.PHONY: all build release install uninstall purge test clean whitepaper

all: build

build:
	$(CARGO) build --release

target/release/rinode:
	$(CARGO) build --release

install: target/release/rinode
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

uninstall:
	rm -f $(DESTDIR)$(BINDIR)/rinode
	rm -f $(DESTDIR)$(MANDIR)/rinode.1
	rm -f $(DESTDIR)$(BASHCOMPDIR)/rinode
	rm -f $(DESTDIR)/etc/bash_completion.d/rinode
	rm -f $(DESTDIR)$(FISHCOMPDIR)/rinode.fish
	rm -f $(DESTDIR)$(ZSHCOMPDIR)/_rinode
	rm -rf $(DESTDIR)$(SYSCONFDIR)
	rm -f /root/.local/bin/rinode
	rm -f /root/.local/share/man/man1/rinode.1
	rm -f /root/.config/fish/completions/rinode.fish
	rm -f /root/.local/share/bash-completion/completions/rinode
	rm -rf /root/.config/rinode
	rm -rf /root/.local/share/rinode
	rm -rf /root/.local/share/recent-inode
	@if [ -n "$$SUDO_USER" ]; then \
		USER_HOME=$$(getent passwd "$$SUDO_USER" | cut -d: -f6); \
		if [ -n "$$USER_HOME" ]; then \
			rm -rf "$$USER_HOME/.config/rinode"; \
			rm -rf "$$USER_HOME/.local/share/rinode"; \
			rm -rf "$$USER_HOME/.local/share/recent-inode"; \
			rm -f "$$USER_HOME/.local/bin/rinode"; \
			rm -f "$$USER_HOME/.local/share/man/man1/rinode.1"; \
			rm -f "$$USER_HOME/.config/fish/completions/rinode.fish"; \
			rm -f "$$USER_HOME/.local/share/bash-completion/completions/rinode"; \
		fi; \
	fi
	rm -rf $(HOME)/.config/rinode
	rm -rf $(HOME)/.local/share/rinode
	rm -rf $(HOME)/.local/share/recent-inode
	rm -f $(HOME)/.local/bin/rinode
	rm -f $(HOME)/.local/share/man/man1/rinode.1
	rm -f $(HOME)/.config/fish/completions/rinode.fish
	rm -f $(HOME)/.local/share/bash-completion/completions/rinode
	@if [ -n "$$XDG_CONFIG_HOME" ]; then rm -rf "$$XDG_CONFIG_HOME/rinode"; fi
	@if [ -n "$$XDG_DATA_HOME" ]; then rm -rf "$$XDG_DATA_HOME/rinode" "$$XDG_DATA_HOME/recent-inode"; fi
	@echo "rinode, configurations, databases, and storage uninstalled."

purge: uninstall

test: build
	bash tests/integration_test.sh

clean:
	$(CARGO) clean

whitepaper:
	pdflatex -interaction=nonstopmode -output-directory=docs docs/whitepaper.tex
	pdflatex -interaction=nonstopmode -output-directory=docs docs/whitepaper.tex
