# Resonate

Resonate is a native Linux Bluetooth headphone daemon + CLI for supported Apple (AirPods/Beats), Sony MDR/WH/WF, and Bowers & Wilkins headphones.

It gives you one local control surface for:

- connection and status
- battery telemetry
- ANC/listening mode controls
- device-specific settings (depending on vendor)
- smart auto-pause/resume on in-ear changes

## How it works

Resonate runs in two layers:

1. **Daemon layer** (`resonate daemon`)
   - keeps a shared runtime state (`ResonateState`)
   - monitors paired Bluetooth devices from BlueZ
   - opens a vendor-specific control session when a supported device is connected
   - emits state updates over D-Bus signals
2. **CLI layer** (`resonate <command>`)
   - calls daemon methods over D-Bus
   - prints human-readable or JSON output
   - can also stream live updates (`resonate watch`)

### Runtime flow

1. Daemon starts and registers D-Bus names:
   - `org.headbridge`
   - `org.resonate`
2. It exposes `org.headbridge.Manager` at `/org/headbridge`.
3. A monitoring loop (every ~2.5s) checks:
   - current default audio sink
   - paired/connected devices from BlueZ
   - whether vendor control sessions need connect/reconnect/disconnect
4. Vendor protocol frames/properties update in-memory state.
5. Daemon broadcasts `StateChanged` with JSON status.
6. CLI commands read/call this interface (status, connect, anc, battery, etc.).

## Vendor transport/protocol strategy

- **Apple**: L2CAP session (`transport/l2cap.rs`) + Apple Accessory Protocol parsing (`protocols/aap.rs`)
- **Sony**: RFCOMM session (`transport/rfcomm.rs`) + Sony packet protocol (`protocols/sony.rs`)
- **Bowers & Wilkins**: BlueZ GATT client (`transport/bluez.rs`) + B&W protocol mapping (`protocols/bw.rs`)

If available, Resonate also integrates with:

- **MPRIS D-Bus players** (for smart auto-pause/resume)
- `playerctl` fallback when MPRIS discovery is limited
- `pactl` for default sink tracking
- `org.freedesktop.Notifications` for desktop battery/connect notifications

## Key modules

- `src/main.rs` — command entrypoint and daemon bootstrap
- `src/cli.rs` — CLI command definitions
- `src/manager.rs` — core orchestration and vendor session lifecycle
- `src/dbus_service.rs` — D-Bus interface and method/property bridge
- `src/inventory.rs` — BlueZ inventory and vendor identification
- `src/audio.rs` — smart auto-pause and media-player interactions
- `src/settings.rs` — persisted user settings (`~/.config/resonate/settings.json`)

## Common commands

```bash
# default command: status
resonate

# run daemon in foreground
resonate daemon

# list supported paired devices
resonate devices

# connect by address or device name fragment
resonate connect "AA:BB:CC:DD:EE:FF"
resonate connect "AirPods"

# stream updates for status bars/scripts
resonate watch --format waybar

# output JSON for scripting
resonate --json status
```

## Daemon service management

Resonate includes helper actions for a user-level systemd service:

```bash
resonate daemon install
resonate daemon status
resonate daemon restart
resonate daemon uninstall
```

The generated unit is written under:

- `$XDG_CONFIG_HOME/systemd/user/resonate.service` (or `~/.config/systemd/user/resonate.service`)

## Build

```bash
cargo build --release
```

