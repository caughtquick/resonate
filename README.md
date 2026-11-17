# Resonate

Resonate is a native Linux Bluetooth headphone daemon and CLI providing reverse-engineered, vendor-specific controls for supported **Apple / Beats** (AirPods, AirPods Pro, AirPods Max, Beats) and **Bowers & Wilkins** (Px7, Px8, Pi) wireless headphones.

It brings first-class desktop integration to Linux:
- **Resonate Studio GUI** (`resonate-studio`): Full-featured desktop application built with Slint providing real-time battery HUD, tactile ANC switching, interactive B&W parametric EQ curves, hardware personalization, and daemon supervision.
- **Active Noise Cancellation (ANC)** mode toggling (Off, Noise Cancellation, Transparency, Adaptive, Wind Reduction)
- **Detailed battery telemetry** (left bud, right bud, charging case, and charging indicators)
- **Smart auto-pause / auto-resume** via MPRIS when earbuds are removed or re-inserted
- **Hardware-specific settings** (Conversation Awareness, single-bud ANC, mic routing, chime volume, standby timers, custom action buttons, and hardware equalizers)
- **Status bar streaming** (`resonate watch --format waybar`) for Waybar, Polybar, or custom shell scripts
- **Desktop notifications** via FreeDesktop notifications for low battery alerts and connection status

---

## Table of Contents

- [Supported Hardware](#supported-hardware)
- [Architecture & How It Works](#architecture--how-it-works)
- [Technical Documentation](#technical-documentation)
- [Prerequisites & Permissions](#prerequisites--permissions)
- [Installation & Setup](#installation--setup)
- [CLI Command Reference](#cli-command-reference)
  - [Status & Introspection](#status--introspection)
  - [Multipoint Bluetooth Management](#resonate-multipoint)
  - [Audio & Listening Mode Controls](#audio--listening-mode-controls)
  - [Hardware Toggles & Settings](#hardware-toggles--settings)
  - [Shell Completions](#shell-completions)
- [Resonate Studio (Desktop GUI)](#resonate-studio-desktop-gui)
- [Status Bar Integration (Waybar & Polybar)](#status-bar-integration-waybar--polybar)
- [D-Bus API Reference](#d-bus-api-reference)
- [Configuration Reference](#configuration-reference)
- [Troubleshooting & FAQ](#troubleshooting--faq)
- [Building & Development](#building--development)
- [License & Attribution](#license--attribution)

---

## Supported Hardware

| Manufacturer | Models | Features Supported |
| :--- | :--- | :--- |
| **Apple** | AirPods Pro (1st, 2nd gen, USB-C, 3rd gen) | ANC / Transparency / Adaptive, Conversation Awareness, Left/Right/Case battery, Single-bud ANC, Ear detection, Mic selection, Chime volume |
| **Apple** | AirPods 4 (Standard & ANC) | ANC / Transparency / Adaptive (ANC model), Case battery, Ear detection, Chime volume |
| **Apple** | AirPods (1st, 2nd, 3rd gen) | Left/Right/Case battery, Ear detection, Mic selection, Spatial Audio flag |
| **Apple** | AirPods Max (Lightning & USB-C) | ANC / Transparency, Battery percentage, Head detection |
| **Beats** | Beats Fit Pro, Beats Studio Buds+, Beats Studio Pro | ANC / Transparency, Left/Right/Case battery, Ear detection, Mic selection |
| **Beats** | Beats Solo 4, Beats Solo Buds | Battery telemetry, Device status |
| **Bowers & Wilkins** | Px7 S2, Px7 S2e, Px8, PI7, PI5 | ANC (Off, Auto, Low, High), Bass / Treble tone controls, Wear Sensor, Auto-standby timer, Action button remapping |

---

## Architecture & How It Works

Resonate operates in two tiers:

```mermaid
flowchart TD
    subgraph Hardware["Bluetooth Devices"]
        AP["Apple AirPods / Beats\n(L2CAP PSM 0x1001)"]
        BW["Bowers & Wilkins\n(BLE GATT Characteristics)"]
    end

    subgraph Daemon["Resonate Daemon (resonate daemon)"]
        BZ["BlueZ D-Bus Client\n(Device Inventory & Sinks)"]
        MGR["DeviceManager\n(Session Lifecycle & State Synchronization)"]
        AP_S["AppleL2capSession\n(AAP Frame Protocol)"]
        BW_S["BluezGattClient\n(MessagePack RPC)"]
        AP_CO["AutoPauseCoordinator\n(MPRIS Media Player Integration)"]
        NOTIF["NotificationService\n(org.freedesktop.Notifications)"]
        DBUS["D-Bus Service\n(org.headbridge.Manager /org/headbridge)"]
    end

    subgraph Clients["Control Clients"]
        CLI["CLI Commands\n(resonate anc, status, ...)"]
        BAR["Status Bars\n(Waybar / Polybar via watch)"]
        GUI["Third-Party D-Bus Clients\n(Desktop Panels / Scripts)"]
    end

    AP <--> AP_S
    BW <--> BW_S
    BZ <--> MGR
    AP_S <--> MGR
    BW_S <--> MGR
    MGR <--> AP_CO
    MGR <--> NOTIF
    MGR <--> DBUS
    DBUS <--> CLI
    DBUS <--> BAR
    DBUS <--> GUI
```

### Runtime Flow

1. **Daemon Startup**: `resonate daemon` claims the session D-Bus names `org.headbridge` and `org.resonate`, publishing the `org.headbridge.Manager` interface at `/org/headbridge`.
2. **Monitoring Loop**: Every 2.5 seconds, the daemon:
   - Queries `pactl info` for the active default PulseAudio/PipeWire sink.
   - Queries BlueZ `org.bluez.Device1` objects to discover paired and connected Bluetooth audio devices.
   - Detects when a supported headphone connects or disconnects.
3. **Vendor Session Establishment**:
   - **Apple**: Connects an asynchronous non-blocking L2CAP socket directly to the device on PSM `0x1001`, sends protocol handshake packets, negotiates feature flags (`0xD7`), subscribes to real-time telemetry, and auto-enables single-bud ANC.
   - **Bowers & Wilkins**: Locates the vendor GATT characteristics under BlueZ, registers for notifications, and sends MessagePack RPC queries.
4. **State Signals & Broadcasts**: When telemetry arrives (e.g. battery drain, bud removed from ear, mode button clicked on the headphone), the internal `ResonateState` updates and emits a `StateChanged` D-Bus signal with complete JSON.
5. **Smart Auto-Pause**: When in-ear detection detects bud removal, `AutoPauseCoordinator` queries MPRIS (`org.mpris.MediaPlayer2.*`), pauses actively playing players, and records their bus names. Re-inserting the earbuds within 10 minutes automatically resumes playback.

---

## Technical Documentation

For developers, protocol engineers, and advanced integrators, deep-dive specifications are available in the [`docs/`](docs/) directory:

- 📖 **[Reverse-Engineered Protocol Specifications](docs/PROTOCOLS.md)**: Packet layout, opcode catalog, handshake sequences, AAP L2CAP seqpackets, and Bowers & Wilkins MessagePack RPC commands over BLE GATT.
- 🔌 **[D-Bus API Reference & Client Integration](docs/DBUS_API.md)**: Complete method signatures, properties, signals, introspection XML, and client examples for `busctl`, Python (`pydbus`), and Rust (`zbus`).
- 🏗️ **[Architecture & Internals](docs/ARCHITECTURE.md)**: Detailed breakdown of the Tokio concurrency model, RAII task cleanup (`AbortOnDrop`), MPRIS media player coordination, and persistence caching.

---

## Prerequisites & Permissions

- **Linux Kernel & BlueZ**: Linux 5.x+ with BlueZ 5.50+ installed and running (`systemctl status bluetooth`).
- **Bluetooth Permissions**: Accessing raw Bluetooth sockets (`L2CAP`) requires standard user permissions. On most distributions (Arch, Fedora, Ubuntu, Debian), ensuring your user is part of the `bluetooth` group or having active logind seat sessions is sufficient:
  ```bash
  sudo usermod -aG bluetooth $USER
  ```
- **Audio Stack**: PulseAudio or PipeWire with `pactl` (usually provided by `pulseaudio-utils` or `pipewire-pulse`).
- **Media Player Control**: D-Bus MPRIS-compliant media players (Spotify, Firefox, Chrome, VLC, Rhythmbox, etc.). `playerctl` is optional and used as a fallback.
- **Desktop Notifications**: Any FreeDesktop-compliant notification daemon (Dunst, Mako, SwayNotificationCenter, GNOME, KDE).

---

## Installation & Setup

### Building from Source

Ensure you have Rust and Cargo installed (1.75+ recommended):

```bash
git clone https://github.com/caughtquick/resonate.git
cd resonate
cargo build --release
cp target/release/resonate ~/.cargo/bin/
```

### Systemd User Daemon Setup

Resonate includes built-in commands to install, run, and manage a user-level systemd service:

```bash
# 1. Install and start user systemd unit
resonate daemon install

# 2. Check service status
resonate daemon status

# 3. View live logs
journalctl --user -u resonate.service -f
```

The service unit is created at:
- `~/.config/systemd/user/resonate.service`

Lifecycle commands:
```bash
resonate daemon start      # Start daemon
resonate daemon stop       # Stop daemon
resonate daemon restart    # Restart daemon
resonate daemon uninstall  # Remove systemd user unit
```

---

## CLI Command Reference

### Status & Introspection

#### `resonate` or `resonate status`
Displays the full status of the active headphone session, including connection state, model name, battery breakdown, ANC mode, audio sink, and feature toggles:
```bash
$ resonate
AirPods Pro (2nd gen, USB-C) [Connected]
Vendor:        Apple
Audio Sink:    bluez_output.6C_12_70_3F_8E_AE.1
Battery:       Left: 82% | Right: 85% | Case: 90% (Discharging)
Noise Control: Noise Cancellation
In-Ear Detect: Enabled (In-Ear)
Auto-Pause:    Enabled
Conv. Aware:   Disabled
Mic Mode:      Auto
One-Bud ANC:   Enabled
```

Add `--json` to output machine-readable JSON:
```bash
$ resonate --json status
{
  "connected": true,
  "connectionState": "connected",
  "deviceName": "AirPods Pro (2nd gen, USB-C)",
  "vendor": "Apple",
  "batteryPercent": 85,
  "batteryLeft": 82,
  "batteryRight": 85,
  "batteryCase": 90,
  "isCharging": false,
  "noiseMode": "noise_cancellation",
  "audioSink": "bluez_output.6C_12_70_3F_8E_AE.1",
  "earDetection": true,
  "autoPause": true,
  "inEar": true,
  "inEarPrimary": "in_ear",
  "inEarSecondary": "in_ear",
  "conversationAwareness": false,
  "micMode": "auto",
  "oneBudAnc": true,
  "chimeVolume": 80,
  "standbyMinutes": 0,
  "equalizerPreset": ""
}
```

#### `resonate battery`
Quick overview of battery levels across buds and charging case:
```bash
$ resonate battery
Battery: 85%
  Left Bud:   82%
  Right Bud:  85%
  Case:       90%
  Status:     Discharging
```

#### `resonate devices`
Lists all paired Bluetooth audio devices discovered via BlueZ, their support status, and connection state:
```bash
$ resonate devices
Paired Devices:
 * Abhijit's AirPods Pro [6C:12:70:3F:8E:AE] (Apple: AirPods Pro (2nd gen, USB-C)) - Connected
   Px7 S2e                [00:11:22:33:44:55] (Bowers & Wilkins: Px7 S2e) - Paired
```

#### `resonate multipoint [list|connect <target>|disconnect <target>]`
Inspects and manages hardware multipoint paired devices on Bowers & Wilkins headphones over BLE GATT:

- **List Paired Devices**: Run without arguments or with `list`:
  ```bash
  $ resonate multipoint
  === Multipoint Paired Devices ===
   - [0] MacBook Pro (EC:66:D1:BE:1D:80) [Connected]
   - [1] iPhone 15 (30:E3:A4:86:67:F2) [Paired]
  ```

- **Connect / Switch Active Audio Link**: Trigger device connection by slot index, name substring, or MAC address:
  ```bash
  resonate multipoint connect 1
  resonate multipoint connect "MacBook"
  resonate multipoint connect "30:E3:A4:86:67:F2"
  ```

- **Disconnect Audio Link**: Disconnect a secondary or active paired link from the headphones:
  ```bash
  resonate multipoint disconnect 0
  ```

- **JSON Output**: Add `--json` for scripting and automations:
  ```bash
  $ resonate --json multipoint
  [
    {
      "id": 0,
      "name": "MacBook Pro",
      "address": "EC:66:D1:BE:1D:80",
      "connected": true
    },
    {
      "id": 1,
      "name": "iPhone 15",
      "address": "30:E3:A4:86:67:F2",
      "connected": false
    }
  ]
  ```

#### `resonate connect [target]` & `resonate disconnect`
Connects to or disconnects from a supported headphone by address or name substring:
```bash
resonate connect "AirPods"
resonate connect "6C:12:70:3F:8E:AE"
resonate disconnect
```

---

### Audio & Listening Mode Controls

#### `resonate anc [mode]`
Gets or sets the active noise cancellation / listening mode.
Supported modes: `off`, `anc` (or `nc`), `ambient` (or `transparency`), `adaptive`, `wind`.

```bash
# Query current mode
resonate anc

# Switch modes
resonate anc anc
resonate anc transparency
resonate anc adaptive
resonate anc off
```

#### `resonate eq [args...]`
Gets or sets tone controls or equalizers on Bowers & Wilkins headphones:

- **Query Current Settings**: Run without arguments to inspect active equalizer configuration:
  ```bash
  $ resonate eq
  Current equalizer: Bass: +2 dB, Treble: -1 dB
  ```

- **Tone Controls** (Px7 S2, Px7 S2e, Px8): Adjust bass and treble gains in decibels (-6 to +6 dB, with 0.1 dB precision):
  ```bash
  resonate eq 3 -1              # Bass: +3 dB, Treble: -1 dB
  resonate eq bass 2 treble -1  # Explicit keywords
  resonate eq +1.5 -0.5         # Tenth-decibel precision
  resonate eq flat              # Reset tone controls to 0 dB
  ```

- **5-Band Parametric Equalizer** (Px7 S3): Pass 5 gain values in decibels:
  ```bash
  resonate eq 0 1 2 1 0         # Bands 1-5 gains in dB
  ```

---

### Hardware Toggles & Settings

#### `resonate auto-pause [on|off]`
Toggles Resonate's smart auto-pause and auto-resume behavior when earbuds are taken out or replaced.

#### `resonate wear-sensor [action]`
Inspects or configures hardware in-ear and over-ear wear detection (Apple, B&W):

- **Inspect Current Status**: Run without arguments to inspect hardware toggle state, real-time placement, and wear sensitivity:
  ```bash
  $ resonate wear-sensor
  === Wear Sensor Status ===
  Wear Detection: Enabled
  Currently Worn: Yes
  Sensitivity:    Normal / Medium
  ```

- **Toggle Detection**: Turn hardware wear sensing on or off:
  ```bash
  resonate wear-sensor on
  resonate wear-sensor off
  ```

- **Configure Sensitivity** (B&W): Tune wear sensor threshold (`low`, `medium` / `normal`, `high`):
  ```bash
  resonate wear-sensor low
  resonate wear-sensor medium
  resonate wear-sensor high
  ```

#### `resonate conv-aware [on|off]`
Enables or disables Conversation Awareness on AirPods Pro 2 / AirPods 4 ANC.

#### `resonate mic [auto|left|right]`
Configures active microphone selection for dual-bud Apple accessories.

#### `resonate one-bud-anc [on|off]`
Allows active noise cancellation to remain active even when only one earbud is worn.

#### `resonate chime [0-100]`
Sets the volume of the charging case speaker and bud alert tones (AirPods Pro 2).

#### `resonate standby [minutes]` (B&W)
Sets the auto-power off / sleep timer.

#### `resonate button [anc|voice]` (B&W)
Remaps the physical action button on Bowers & Wilkins headphones.

---

### Shell Completions

Generate shell completion scripts for Bash, Zsh, or Fish:

```bash
# Bash
resonate completion bash > ~/.local/share/bash-completion/completions/resonate

# Zsh
resonate completion zsh > ~/.zsh/completion/_resonate

# Fish
resonate completion fish > ~/.config/fish/completions/resonate.fish
```

---

## Resonate Studio (Desktop GUI)

Resonate includes **Resonate Studio** (`resonate-studio`), a dedicated, hardware-accelerated desktop application built with [Slint](https://slint.dev/) designed for complete headphone management on modern Linux desktops (Wayland, Niri, GNOME, KDE, Sway).

```bash
# Launch Resonate Studio
resonate-studio
```

### Studio Features

1. **Active Device Dashboard**:
   - Hero hardware card with model recognition and live connection state.
   - 3-point battery HUD (Left earbud, Right earbud, Case) with charging animations (`⚡`) and placement status (`In Ear`, `In Case`, `Out of Ear`).
   - Tactile segmented Listening Mode selector (**Off**, **Noise Cancellation**, **Transparency**, **Adaptive Audio**, **Wind Reduction**).
   - Quick toggles for Smart Auto-Pause, Conversation Awareness, and Single-Bud ANC.

2. **Acoustic & EQ Studio (Bowers & Wilkins)**:
   - Interactive Bass and Treble sliders (`-6.0 dB` to `+6.0 dB`).
   - Real-time parametric frequency response curve visualizer.
   - Factory presets (*Flat / Reset*, *Bass Boost*, *Vocal*, *Treble Boost*).

3. **Hardware Personalization**:
   - Microphone earbud routing selector (*Automatic*, *Always Left*, *Always Right*).
   - Alert chime volume slider (`0%` to `100%`) for AirPods Pro 2 case speaker and buds.
   - Auto-standby sleep timer (*Never*, *10m*, *15m*, *30m*).
   - Physical quick-action button remapping (*Listening Modes* vs *Voice Assistant*).

4. **Devices & Multipoint Matrix**:
   - Paired Bluetooth headphones inventory with 1-click connect/disconnect.
   - Multi-device audio handoff matrix for Bowers & Wilkins multipoint devices.

5. **Daemon Supervisor & Real-Time Diagnostics**:
   - Background systemd user service status (`Active (Running)`, `Inactive`, `Failed`).
   - 1-click `Start Daemon`, `Stop Daemon`, and `Restart Daemon` controls.
   - Real-time scrolling telemetry event stream monitoring `StateChanged` D-Bus signals.

---

## Status Bar Integration (Waybar & Polybar)

Resonate provides continuous event streaming via `resonate watch` designed for status bar widgets, desktop panels, and custom scripts:

```bash
# Waybar format (JSON stream with class, alt, tooltip, percentage)
resonate watch --format waybar

# Compact format (Plain text one-liner for Polybar, i3blocks, tmux)
resonate watch --format compact

# Raw format (Full JSON telemetry payload per event)
resonate watch --format json
```

### 1. Waybar Output Format (`--format waybar`)

```json
{
  "text": " 85% [ANC]",
  "alt": "connected",
  "tooltip": "AirPods Pro (2nd gen, USB-C)\nBattery: Left: 82% | Right: 85% | Case: 90%\nMode: Noise Cancellation\nSink: bluez_output.6C_12_70_3F_8E_AE.1",
  "class": "connected",
  "percentage": 85
}
```

#### Waybar Configuration
In `~/.config/waybar/config`:
```json
"custom/resonate": {
    "format": " {icon} {}",
    "format-icons": {
        "connected": "󰋋",
        "disconnected": "󰟎"
    },
    "return-type": "json",
    "exec": "resonate watch --format waybar",
    "on-click": "resonate anc anc || resonate anc transparency",
    "on-click-right": "resonate anc off",
    "on-click-middle": "resonate disconnect"
}
```

In `~/.config/waybar/style.css`:
```css
#custom-resonate {
    padding: 0 10px;
    color: #89b4fa;
}

#custom-resonate.connected {
    color: #a6e3a1;
}

#custom-resonate.disconnected {
    color: #6c7086;
}
```

### 2. Polybar Configuration (`--format compact`)

In `~/.config/polybar/config.ini`:
```ini
[module/resonate]
type = custom/script
exec = resonate watch --format compact
tail = true
format = <label>
label = %output%
format-foreground = #89b4fa
click-left = resonate anc anc || resonate anc transparency
click-right = resonate anc off
click-middle = resonate disconnect
```

When connected: `🎧 85% • ANC`
When disconnected: `🎧 Disconnected`

### 3. Tmux Status Bar Integration

In `~/.tmux.conf`:
```tmux
set -g status-right '#(resonate --json status | jq -r "if .connected then \"🎧 \(.batteryPercent)% [\(.noiseMode)]\" else \"\" end") %H:%M'
```

---

## D-Bus API Reference

Resonate exposes its interface over session D-Bus:

- **Bus Names**: `org.headbridge`, `org.resonate`
- **Object Path**: `/org/headbridge`
- **Interface**: `org.headbridge.Manager`

### Methods

| Method | Signature | Description |
| :--- | :--- | :--- |
| `GetStatus` | `() -> (s)` | Returns complete state formatted as JSON string |
| `ListDevices` | `() -> (s)` | Returns paired devices list as JSON string |
| `GetMultipoint` | `() -> (b, s)` | Queries multipoint paired devices list from active B&W headphone |
| `ConnectMultipoint` | `(s target) -> (b, s)` | Connects paired multipoint device by slot index, MAC, or name |
| `DisconnectMultipoint` | `(s target) -> (b, s)` | Disconnects paired multipoint device by slot index, MAC, or name |
| `Connect` | `(s target) -> (b, s)` | Connects to device by name or MAC address |
| `Disconnect` | `() -> (b)` | Disconnects the active session |
| `SetAnc` | `(s mode) -> (b, s)` | Sets ANC mode (`"off"`, `"anc"`, `"ambient"`, `"adaptive"`, `"wind"`) |
| `SetEqualizer` | `(s args) -> (b, s)` | Configures EQ preset or tone controls string |
| `SetWearSensor` | `(b enable) -> (b)` | Enables or disables hardware wear detection |
| `SetWearSensorString` | `(s action) -> (b, s)` | Sets wear sensor toggle or sensitivity (`"on"`, `"off"`, `"low"`, `"medium"`, `"high"`) |
| `SetAutoPause` | `(b enable) -> (b)` | Enables or disables software smart auto-pause |
| `GetAutoPause` | `() -> (b)` | Queries smart auto-pause setting |
| `SetConversationAwareness` | `(b enable) -> (b)` | Toggles Conversation Awareness |
| `SetMicrophoneMode` | `(s mode) -> (b)` | Configures mic routing (`"auto"`, `"left"`, `"right"`) |
| `SetOneBudAnc` | `(b enable) -> (b)` | Toggles single-bud ANC permission |
| `SetChimeVolume` | `(y vol) -> (b)` | Sets chime volume (0-100) |
| `SetStandby` | `(s val) -> (b)` | Sets auto-power off timer |
| `SetActionButton` | `(s action) -> (b, s)` | Remaps B&W action button (`"anc"`, `"voice"`) |

### Properties

| Property | Type | Description |
| :--- | :--- | :--- |
| `Connected` | `bool` | True if a supported session is communicating |
| `ConnectionState` | `string` | Connection status (`"connected"`, `"connecting"`, `"idle"`) |
| `DeviceName` | `string` | Friendly product name |
| `Vendor` | `string` | `"Apple"`, `"Bowers & Wilkins"`, or `"Unknown"` |
| `BatteryPercent` | `int32` | Overall or primary battery percentage (-1 if unknown) |
| `BatteryLeft` | `int32` | Left earbud battery percentage |
| `BatteryRight` | `int32` | Right earbud battery percentage |
| `BatteryCase` | `int32` | Charging case battery percentage |
| `IsCharging` | `bool` | True if any component is actively charging |
| `NoiseMode` | `string` | Active listening mode (`"noise_cancellation"`, `"transparency"`, etc.) |
| `EarDetection` | `bool` | Hardware wear sensor toggle |
| `AutoPause` | `bool` | MPRIS smart auto-pause toggle |
| `InEar` | `bool` | True if earbuds are inserted in ears |
| `InEarPrimary` | `string` | Primary bud wear state (`"in_ear"`, `"out_of_ear"`, `"in_case"`) |
| `InEarSecondary` | `string` | Secondary bud wear state (`"in_ear"`, `"out_of_ear"`, `"in_case"`) |
| `ConversationAwareness` | `bool` | True if Conversation Awareness is active |
| `MicrophoneMode` | `string` | Active microphone routing (`"auto"`, `"left"`, `"right"`) |
| `OneBudAnc` | `bool` | True if single-bud ANC is permitted |
| `ChimeVolume` | `int32` | Chime volume level (0-100) |
| `StandbyMinutes` | `int32` | Sleep timeout in minutes (0 = disabled) |
| `EqualizerPreset` | `string` | Current equalizer preset or tone controls string |
| `AudioSink` | `string` | Current PipeWire / PulseAudio sink name |
| `WearSensitivity` | `int32` | B&W wear sensor sensitivity (0 = Low, 1 = Normal/Medium, 2 = High, -1 if unavailable) |

> [!TIP]
> For complete D-Bus introspection XML, type signatures, and client integration scripts in Python and Rust, consult **[`docs/DBUS_API.md`](docs/DBUS_API.md)**.

### Signals

- `StateChanged(s json)`: Broadcast whenever telemetry, battery levels, or settings change.

### Inspecting via `busctl`

```bash
# Query status property
busctl --user get-property org.headbridge /org/headbridge org.headbridge.Manager NoiseMode

# Call method
busctl --user call org.headbridge /org/headbridge org.headbridge.Manager SetAnc s "anc"

# Monitor real-time state changes
busctl --user monitor org.headbridge
```

---

## Configuration Reference

Resonate stores user preferences and cached battery states in:
- `$XDG_CONFIG_HOME/resonate/settings.json` (defaults to `~/.config/resonate/settings.json`)

### File Schema Example

```json
{
  "notifications_enabled": true,
  "auto_pause_enabled": true,
  "cached_case_batteries": {
    "6C:12:70:3F:8E:AE": 90
  },
  "preferred_noise_modes": {
    "6C:12:70:3F:8E:AE": "noise_cancellation"
  }
}
```

- `notifications_enabled`: Enables desktop notifications for connection and battery alerts (default: `true`).
- `auto_pause_enabled`: Enables smart auto-pause / auto-resume when earbuds are removed or inserted (default: `true`).
- `cached_case_batteries`: Maps device MAC address to the last reported case battery level so it remains available in status bars even when the case lid is closed and the case radio enters sleep.
- `preferred_noise_modes`: Remembers the user's preferred listening mode to restore across reconnects.

---

## Troubleshooting & FAQ

### 1. "Cannot connect to Resonate daemon over D-Bus"
Make sure the daemon is running in the background:
```bash
resonate daemon status
# If inactive:
resonate daemon start
```

### 2. "AirPods connect to Linux audio, but Resonate doesn't show battery or ANC"
Ensure the Apple L2CAP control channel is established. Some distributions or kernel versions restrict raw L2CAP seqpacket socket creation without appropriate capabilities. Add your user to the `bluetooth` group:
```bash
sudo usermod -aG bluetooth $USER
```
Then log out and log back in.

### 3. "Noise cancellation doesn't turn on when wearing only one earbud"
By default, Apple firmware disables ANC when only one earbud is worn unless "Noise Cancellation with One AirPod" is enabled. Resonate automatically enables this setting upon session connection (`OneBudAnc = 1`), but you can also verify with:
```bash
resonate one-bud-anc on
```

### 4. "Case battery shows null / not reported"
AirPods charging cases only power on their Bluetooth transmitter when the case lid is open or when earbuds are actively docking/undocking. Resonate caches the last observed case battery percentage and continues serving it from `settings.json`.

---

## Building & Development

### Running Tests

```bash
cargo test
```

### Generating Documentation

```bash
cargo doc --no-deps --open
```

### Code Formatting & Linting

```bash
cargo fmt --check
cargo clippy --all-targets --all-features
```

---

## License & Attribution

Resonate is distributed under the [MIT License](LICENSE).

### Attribution

Resonate is derived from and inspired by [HeadBridge](https://github.com/herenickname/HeadBridge) (Copyright &copy; 2026 HeadBridge contributors), adapting its Bluetooth headphone protocol research and reverse engineering (especially for Bowers & Wilkins BLE RPC protocol, along with maintaining D-Bus interface compatibility under `org.headbridge`) for native Linux environments.

