PREFIX ?= /usr/local
BINDIR ?= $(PREFIX)/bin
DATADIR ?= $(PREFIX)/share
APPLICATIONS_DIR ?= $(DATADIR)/applications
ICONS_DIR ?= $(DATADIR)/icons/hicolor/scalable/apps

# Binary build targets
TARGET_CLI := target/release/resonate
TARGET_STUDIO := target/release/resonate-studio

# Source tracking
# CLI depends on CLI/daemon sources and shared libraries, but not GUI/Slint sources or build.rs
CLI_SOURCES := $(shell find src -type f ! -path 'src/gui*' ! -name 'gui_main.rs' 2>/dev/null) Cargo.toml Cargo.lock

# Studio depends on GUI sources, Slint UI, build script, embedded icons, and shared libraries, but not CLI main/args
STUDIO_SOURCES := $(shell find src ui build.rs assets/icons -type f ! -name 'main.rs' ! -name 'cli.rs' 2>/dev/null) Cargo.toml Cargo.lock

# Installed file targets
INSTALLED_CLI := $(DESTDIR)$(BINDIR)/resonate
INSTALLED_STUDIO := $(DESTDIR)$(BINDIR)/resonate-studio
INSTALLED_ICON := $(DESTDIR)$(ICONS_DIR)/resonate-studio.svg
INSTALLED_DESKTOP := $(DESTDIR)$(APPLICATIONS_DIR)/resonate-studio.desktop

.PHONY: all build release cli studio install install-user install-cli install-studio uninstall uninstall-user clean test

all: build

build: $(TARGET_CLI) $(TARGET_STUDIO)

release: build

cli: $(TARGET_CLI)

studio: $(TARGET_STUDIO)

$(TARGET_CLI): $(CLI_SOURCES)
	cargo build --release --bin resonate

$(TARGET_STUDIO): $(STUDIO_SOURCES)
	cargo build --release --bin resonate-studio

# Installation rules
$(INSTALLED_CLI): $(TARGET_CLI)
	install -d "$(@D)"
	install -m 755 $< $@
	rm -f "$(@D)/resonate-gui"

$(INSTALLED_STUDIO): $(TARGET_STUDIO)
	install -d "$(@D)"
	install -m 755 $< $@
	rm -f "$(@D)/resonate-gui"

$(INSTALLED_ICON): assets/icons/headphone.svg
	install -d "$(@D)"
	install -m 644 $< $@
	rm -f "$(@D)/resonate-gui.svg"

$(INSTALLED_DESKTOP): assets/resonate-studio.desktop
	install -d "$(@D)"
	sed -e 's|^Exec=.*|Exec=$(BINDIR)/resonate-studio|' \
	    -e 's|^Icon=.*|Icon=resonate-studio|' \
	    $< > $@
	chmod 644 $@
	rm -f "$(@D)/resonate-gui.desktop"
	@if [ -z "$(DESTDIR)" ] && command -v update-desktop-database >/dev/null 2>&1; then \
		update-desktop-database "$(@D)" || true; \
	fi

install: $(INSTALLED_CLI) $(INSTALLED_STUDIO) $(INSTALLED_ICON) $(INSTALLED_DESKTOP)
	@echo "Resonate is up to date in $(PREFIX)"

install-cli: $(INSTALLED_CLI)
	@echo "Resonate CLI is up to date in $(PREFIX)"

install-studio: $(INSTALLED_STUDIO) $(INSTALLED_ICON) $(INSTALLED_DESKTOP)
	@echo "Resonate Studio is up to date in $(PREFIX)"

install-user:
	@$(MAKE) install PREFIX="$(HOME)/.local"

uninstall-user:
	@$(MAKE) uninstall PREFIX="$(HOME)/.local"

uninstall:
	rm -f "$(DESTDIR)$(BINDIR)/resonate"
	rm -f "$(DESTDIR)$(BINDIR)/resonate-studio"
	rm -f "$(DESTDIR)$(BINDIR)/resonate-gui"
	rm -f "$(DESTDIR)$(ICONS_DIR)/resonate-studio.svg"
	rm -f "$(DESTDIR)$(ICONS_DIR)/resonate-gui.svg"
	rm -f "$(DESTDIR)$(APPLICATIONS_DIR)/resonate-studio.desktop"
	rm -f "$(DESTDIR)$(APPLICATIONS_DIR)/resonate-gui.desktop"
	@if [ -z "$(DESTDIR)" ] && command -v update-desktop-database >/dev/null 2>&1; then \
		update-desktop-database "$(DESTDIR)$(APPLICATIONS_DIR)" || true; \
	fi
	@echo "Resonate uninstalled from $(PREFIX)"

test:
	cargo test

clean:
	cargo clean
