# Resonate Architecture & Internals

This document details the software architecture, concurrency model, and subsystem interactions of the Resonate daemon and CLI.

---

## 1. System Overview

Resonate is structured as a native Linux daemon and control utility written in Rust. It decouples high-level user interfaces (CLI, status bars, D-Bus clients) from low-level Bluetooth transports and reverse-engineered wire protocols.

```mermaid
flowchart TD
    subgraph Hardware["Bluetooth Headphone Hardware"]
        AP["Apple AirPods / Beats\n(L2CAP PSM 0x1001)"]
        BW["Bowers & Wilkins\n(BLE GATT Characteristics)"]
    end

    subgraph LinuxKernel["Linux Kernel & System Services"]
        BT["Kernel Bluetooth Stack\n(AF_BLUETOOTH / L2CAP)"]
        BZ["BlueZ Daemon\n(org.bluez ObjectManager & GATT)"]
        PA["PipeWire / PulseAudio\n(Audio Sinks & Routing)"]
        MP["MPRIS Media Players\n(Spotify, Firefox, VLC)"]
        NOTIF_D["Notification Daemon\n(Dunst, Mako, GNOME)"]
    end

    subgraph ResonateDaemon["Resonate Daemon (resonate daemon)"]
        MGR["DeviceManager\n(Session Lifecycle & State Sync)"]
        L2["AppleL2capSession\n(Tokio AsyncFd)"]
        GATT["BluezGattClient\n(zbus GATT Client)"]
        AP_CO["AutoPauseCoordinator\n(MPRIS Tracker)"]
        AUD["AudioMonitor\n(pactl sink poller)"]
        SET["SettingsStore\n(~/.config/resonate/settings.json)"]
        DBUS_S["D-Bus Service\n(org.headbridge.Manager)"]
    end

    subgraph UserInterfaces["Client Applications"]
        CLI["Resonate CLI\n(resonate anc, status, eq)"]
        BAR["Desktop Status Bars\n(Waybar, Polybar via watch)"]
        APP["Third-Party D-Bus Clients\n(Scripts, Desktop Panels)"]
    end

    AP <--> BT
    BW <--> BZ
    BT <--> L2
    BZ <--> GATT
    BZ <--> MGR
    PA <--> AUD
    MP <--> AP_CO
    NOTIF_D <-- MGR

    L2 <--> MGR
    GATT <--> MGR
    AUD --> MGR
    AP_CO <--> MGR
    SET <--> MGR
    MGR <--> DBUS_S

    DBUS_S <--> CLI
    DBUS_S <--> BAR
    DBUS_S <--> APP
```

---

## 2. Core Subsystems

### 2.1 DeviceManager (`src/manager.rs`)
The central orchestrator and single source of truth for headphone state:
- Maintains the active session address and runtime `ResonateState` protected by `Arc<RwLock<ResonateState>>`.
- Drives the 2.5-second background monitoring loop (`run_monitoring_loop`), polling default audio sinks and checking BlueZ connection states.
- Coordinates automatic reconnection when a paired headphone reconnects to Linux audio.
- Emits the `StateChanged` D-Bus signal whenever battery levels, listening mode, or wear status change.

### 2.2 Transports (`src/transport/`)

#### Asynchronous L2CAP Socket (`src/transport/l2cap.rs`)
- Uses raw Linux Bluetooth sockets (`AF_BLUETOOTH`, `SOCK_SEQPACKET`, `BTPROTO_L2CAP`) targeting PSM `0x1001`.
- Implements non-blocking I/O using Tokio's `AsyncFd` wrapper around standard file descriptors.
- Ingestion loop continuously reads incoming AAP packets and dispatches them to the protocol parser.

#### BlueZ GATT Client (`src/transport/bluez.rs`)
- Traverses BlueZ's `org.freedesktop.DBus.ObjectManager` tree to discover GATT services and characteristics matching Bowers & Wilkins UUIDs.
- Subscribes to GATT notifications (`start_notify`) and sends RPC write requests (`write_value`).
- Implements an `AbortOnDrop` RAII task guard to ensure spawned background listeners are aborted immediately upon device disconnection.

#### Bluetooth Address (`src/transport/bluetooth_addr.rs`)
- Provides `BdAddr`: a type-safe 6-byte MAC address structure with parsing, uppercase hex formatting, and endianness conversion required by `sockaddr_l2`.

### 2.3 Protocols (`src/protocols/`)

#### Apple Accessory Protocol (`src/protocols/aap.rs` & `src/protocols/apple_models.rs`)
- Encapsulates binary packet framing, handshake replies, and 14-byte feature negotiation packets.
- Implements decoders for:
  - Multi-component battery payloads (Left, Right, Case percentages and charging flags).
  - In-ear detection events (In-Ear, Out-of-Ear, In-Case).
  - Settings writes (ANC modes, single-bud ANC, mic selection, chime volume).
- Parses BlueZ `Modalias` strings (e.g. `bluetooth:v004Cp2014d...`) to match known AirPods and Beats hardware revisions and capabilities.

#### Bowers & Wilkins RPC (`src/protocols/bw.rs`)
- Encodes and decodes binary RPC frames using MessagePack serialization.
- Maps command namespaces (`0x02` System, `0x03` ANC, `0x04` Audio, `0x05` Multipoint, `0x08` Telemetry, `0x0A` Wear Sensor).
- Manages hardware generation profiles (`Standard` for Px7 S3 vs `OriginalPx8` for Px7 S2/Px8).
- Implements tenth-decibel (`0.1 dB`) gain calculations and clamped tone control parsing.

### 2.4 Audio & Smart Auto-Pause (`src/audio.rs`)

#### AutoPauseCoordinator
Implements intelligent auto-pause and auto-resume:
1. **Earbud Removal**: When in-ear detection transitions from both buds in-ear to one or both buds removed, `AutoPauseCoordinator` inspects all running D-Bus media players (`org.mpris.MediaPlayer2.*`).
2. **State Tracking**: Any player actively in `PlaybackStatus == "Playing"` receives a `Pause` command, and its D-Bus bus name is saved in the resume queue.
3. **Re-insertion**: When earbuds are re-inserted within 10 minutes, `AutoPauseCoordinator` sends `Play` to the remembered players.
4. **Docking Reset**: If both earbuds are docked into the charging case or if the 10-minute timeout expires, the resume queue is discarded to prevent unexpected playback later.

#### AudioMonitor
- Executes `pactl info` to extract the current default PulseAudio / PipeWire audio sink.
- Correlates the sink name (e.g. `bluez_output.6C_12_70_3F_8E_AE.1`) with the active Bluetooth device.

### 2.5 D-Bus IPC Interface (`src/dbus_service.rs`)
- Registers `org.headbridge` and `org.resonate` on `/org/headbridge`.
- Bridges incoming synchronous D-Bus calls to asynchronous Tokio tasks spawned on the runtime handle.
- Exposes complete property getters and the `StateChanged` broadcast signal.

---

## 3. Concurrency & Memory Safety

Resonate is built on standard Rust concurrency primitives:

- **Tokio Runtime**: All I/O, socket reading, D-Bus dispatch, and background loops run on a multi-threaded Tokio runtime.
- **Fine-Grained Locking**:
  - `Arc<RwLock<ResonateState>>`: Allows concurrent readers (D-Bus status queries, status bar streaming) without blocking while ensuring atomic state updates when telemetry packets arrive.
  - `Arc<Mutex<Option<AppleL2capSession>>>`: Protects the active socket handle.
- **Resource Cleanup (RAII)**:
  - Task handles are wrapped in `AbortOnDrop` structures. When a session disconnects, dropping the client automatically aborts background listener tasks, preventing background task leaks.

---

## 4. State Persistence & Caching (`src/settings.rs`)

User preferences and hardware telemetry caches are stored in `$XDG_CONFIG_HOME/resonate/settings.json`:

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

### Case Battery Caching
AirPods cases only broadcast their battery state when the case lid is open or when buds are docked/undocked. Once the lid is closed, the case radio powers down. Resonate caches the last reported battery percentage to disk and memory, ensuring status bars and CLI queries continue displaying accurate case battery levels throughout the day.
