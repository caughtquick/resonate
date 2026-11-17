PREFIX ?= /usr/local
BINDIR ?= $(PREFIX)/bin
DATADIR ?= $(PREFIX)/share
APPLICATIONS_DIR ?= $(DATADIR)/applications
ICONS_DIR ?= $(DATADIR)/icons/hicolor/scalable/apps

.PHONY: all build release install install-user uninstall uninstall-user clean test

all: build

build:
	cargo build --release

release: build

install-user:
	@$(MAKE) install PREFIX="$(HOME)/.local"

uninstall-user:
	@$(MAKE) uninstall PREFIX="$(HOME)/.local"

install: build
	install -d "$(DESTDIR)$(BINDIR)"
	install -m 755 target/release/resonate "$(DESTDIR)$(BINDIR)/resonate"
	install -m 755 target/release/resonate-studio "$(DESTDIR)$(BINDIR)/resonate-studio"
	# Clean up any legacy -gui artifacts
	rm -f "$(DESTDIR)$(BINDIR)/resonate-gui"
	rm -f "$(DESTDIR)$(ICONS_DIR)/resonate-gui.svg"
	rm -f "$(DESTDIR)$(APPLICATIONS_DIR)/resonate-gui.desktop"
	install -d "$(DESTDIR)$(ICONS_DIR)"
	install -m 644 assets/icons/headphone.svg "$(DESTDIR)$(ICONS_DIR)/resonate-studio.svg"
	install -d "$(DESTDIR)$(APPLICATIONS_DIR)"
	sed -e 's|^Exec=.*|Exec=$(BINDIR)/resonate-studio|' \
	    -e 's|^Icon=.*|Icon=resonate-studio|' \
	    assets/resonate-studio.desktop > "$(DESTDIR)$(APPLICATIONS_DIR)/resonate-studio.desktop"
	chmod 644 "$(DESTDIR)$(APPLICATIONS_DIR)/resonate-studio.desktop"
	@if [ -z "$(DESTDIR)" ] && command -v update-desktop-database >/dev/null 2>&1; then \
		update-desktop-database "$(APPLICATIONS_DIR)" || true; \
	fi
	@echo "Resonate Studio successfully installed to $(PREFIX)"

uninstall:
	rm -f "$(DESTDIR)$(BINDIR)/resonate"
	rm -f "$(DESTDIR)$(BINDIR)/resonate-studio"
	rm -f "$(DESTDIR)$(BINDIR)/resonate-gui"
	rm -f "$(DESTDIR)$(ICONS_DIR)/resonate-studio.svg"
	rm -f "$(DESTDIR)$(ICONS_DIR)/resonate-gui.svg"
	rm -f "$(DESTDIR)$(APPLICATIONS_DIR)/resonate-studio.desktop"
	rm -f "$(DESTDIR)$(APPLICATIONS_DIR)/resonate-gui.desktop"
	@if [ -z "$(DESTDIR)" ] && command -v update-desktop-database >/dev/null 2>&1; then \
		update-desktop-database "$(APPLICATIONS_DIR)" || true; \
	fi
	@echo "Resonate uninstalled from $(PREFIX)"

test:
	cargo test

clean:
	cargo clean
