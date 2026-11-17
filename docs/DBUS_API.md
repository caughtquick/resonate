# Resonate D-Bus API Reference

Resonate exposes its primary IPC control boundary over the session D-Bus. This allows CLI commands, status bars (Waybar, Polybar), desktop environment panels, and third-party scripts to introspect state, monitor real-time changes, and send control commands.

---

## 1. Bus Names & Addressing

| Field | Value | Notes |
| :--- | :--- | :--- |
| **Bus Type** | Session Bus | Connect with `--user` in `busctl` or `DBUS_SESSION_BUS_ADDRESS` |
| **Well-Known Names** | `org.headbridge`<br/>`org.resonate` | Both names are claimed by the daemon for backward compatibility |
| **Object Path** | `/org/headbridge` | Primary management object |
| **Interface** | `org.headbridge.Manager` | Main control and telemetry interface |

---

## 2. Methods

### 2.1 Overview Table

| Method | In Signatures | Out Signatures | Description |
| :--- | :--- | :--- | :--- |
| `GetStatus` | *None* | `(s)` | Returns full state snapshot as formatted JSON string |
| `ListDevices` | *None* | `(s)` | Returns paired Bluetooth audio devices as JSON string |
| `GetMultipoint` | *None* | `(b, s)` | Queries multipoint paired devices list from active B&W headphone |
| `ConnectMultipoint` | `(s target)` | `(b, s)` | Connects paired multipoint device by slot index, MAC, or name |
| `DisconnectMultipoint` | `(s target)` | `(b, s)` | Disconnects paired multipoint device by slot index, MAC, or name |
| `Connect` | `(s target)` | `(b, s)` | Connects to a device by MAC address or name substring |
| `Disconnect` | *None* | `(b)` | Disconnects the active vendor session |
| `SetAnc` | `(s mode)` | `(b, s)` | Sets active noise control (`"off"`, `"anc"`, `"ambient"`, `"adaptive"`) |
| `SetEqualizer` | `(s args)` | `(b, s)` | Configures EQ preset or tone controls string |
| `SetWearSensor` | `(b enable)` | `(b)` | Enables or disables hardware in-ear wear sensors |
| `SetWearSensorString` | `(s action)` | `(b, s)` | Sets wear sensor toggle or sensitivity (`"on"`, `"off"`, `"low"`, `"medium"`, `"high"`) |
| `SetAutoPause` | `(b enable)` | `(b)` | Enables or disables smart auto-pause / auto-resume via MPRIS |
| `GetAutoPause` | *None* | `(b)` | Queries smart auto-pause configuration state |
| `SetConversationAwareness` | `(b enable)` | `(b)` | Toggles Conversation Awareness (AirPods Pro 2) |
| `SetMicrophoneMode` | `(s mode)` | `(b)` | Configures mic routing (`"auto"`, `"left"`, `"right"`) |
| `SetOneBudAnc` | `(b enable)` | `(b)` | Toggles single-bud ANC permission |
| `SetChimeVolume` | `(y vol)` | `(b)` | Sets case/bud alert chime volume (`0..=100`) |
| `SetStandby` | `(s val)` | `(b)` | Configures auto-power off / sleep timer in minutes |
| `SetActionButton` | `(s action)` | `(b, s)` | Remaps physical action button (`"anc"`, `"voice"`) |

---

### 2.2 Method Details

#### `GetStatus() -> (s status_json)`
Returns the current active state serialized as a JSON string.
```bash
busctl --user call org.headbridge /org/headbridge org.headbridge.Manager GetStatus
```

#### `GetMultipoint() -> (b success, s result_json)`
Returns a boolean success flag and a JSON array of paired multipoint devices reported by Bowers & Wilkins headphones:
```json
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

#### `ConnectMultipoint(s target) -> (b success, s message)`
Connects to or switches the active link to a paired multipoint device on Bowers & Wilkins headphones. Target can be a 0-indexed slot index (`"0"`, `"1"`), Bluetooth address (`"EC:66:D1:BE:1D:80"`), or name substring (`"MacBook"`).

#### `DisconnectMultipoint(s target) -> (b success, s message)`
Disconnects an active paired multipoint connection from Bowers & Wilkins headphones by slot index, address, or name substring.

#### `Connect(s target) -> (b success, s message)`
Target can be an exact Bluetooth address (`"6C:12:70:3F:8E:AE"`) or a case-insensitive name substring (`"AirPods"`).

#### `SetAnc(s mode) -> (b success, s message)`
Accepted modes:
- `"off"`: Ambient transparency and ANC disabled.
- `"anc"`, `"nc"`, `"on"`: Active noise cancellation enabled.
- `"ambient"`, `"transparency"`, `"passthrough"`: Transparency / ambient sound enabled.
- `"adaptive"`: Dynamic noise reduction adapting to ambient acoustics (AirPods Pro 2).

#### `SetWearSensorString(s action) -> (b success, s message)`
Configures wear detection toggle or sensitivity level (`"on"`, `"off"`, `"low"`, `"medium"`, `"high"`).

#### `SetEqualizer(s args) -> (b success, s message)`
- If `args` is empty: Returns the active equalizer preset or tone controls string.
- If `args` contains tone controls: e.g. `"bass 2 treble -1"`, `"3 -1"`, or `"flat"`.
- If `args` contains 5 gain numbers: e.g. `"0 1 2 1 0"` (Px7 S3 5-band EQ).

---

## 3. Properties

All properties are read-only and emit change notifications when updated.

| Property | Type | Description |
| :--- | :--- | :--- |
| `Connected` | `b` (bool) | True if a supported headphone session is active |
| `ConnectionState` | `s` (string) | Connection lifecycle state (`"connected"`, `"connecting"`, `"idle"`) |
| `DeviceName` | `s` (string) | Human-readable product or alias name |
| `Vendor` | `s` (string) | Manufacturer name (`"Apple"`, `"Bowers & Wilkins"`, or `"Unknown"`) |
| `BatteryPercent` | `i` (int32) | Primary or overall battery percentage (-1 if unknown) |
| `BatteryLeft` | `i` (int32) | Left earbud battery percentage (-1 if unknown) |
| `BatteryRight` | `i` (int32) | Right earbud battery percentage (-1 if unknown) |
| `BatteryCase` | `i` (int32) | Charging case battery percentage (-1 if unknown) |
| `IsCharging` | `b` (bool) | True if any component is actively drawing charging current |
| `NoiseMode` | `s` (string) | Current ANC mode (`"off"`, `"noise_cancellation"`, `"transparency"`, `"adaptive"`) |
| `EarDetection` | `b` (bool) | Hardware wear detection enabled state |
| `AutoPause` | `b` (bool) | MPRIS smart auto-pause enabled state |
| `InEar` | `b` (bool) | True if earbuds are inserted in ears |
| `InEarPrimary` | `s` (string) | Primary bud state (`"in_ear"`, `"out_of_ear"`, `"in_case"`) |
| `InEarSecondary` | `s` (string) | Secondary bud state (`"in_ear"`, `"out_of_ear"`, `"in_case"`) |
| `ConversationAwareness` | `b` (bool) | Conversation Awareness active state |
| `MicrophoneMode` | `s` (string) | Active microphone routing (`"auto"`, `"left"`, `"right"`) |
| `OneBudAnc` | `b` (bool) | Single-bud ANC enabled state |
| `ChimeVolume` | `i` (int32) | Alert tone volume percentage (`0..=100`) |
| `StandbyMinutes` | `i` (int32) | Sleep timeout in minutes (`0` = disabled) |
| `EqualizerPreset` | `s` (string) | Current EQ preset or tone controls string |
| `AudioSink` | `s` (string) | Current default PipeWire / PulseAudio sink name |
| `WearSensitivity` | `i` (int32) | B&W wear sensor sensitivity (0 = Low, 1 = Normal/Medium, 2 = High, -1 if unavailable) |

---

## 4. Signals

### `StateChanged(s json)`
Broadcast whenever telemetry, connection state, battery percentages, or settings change.

#### Payload Format Example:
```json
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

---

## 5. Client Integration Examples

### 5.1 CLI via `busctl`
```bash
# Read active Noise Cancellation mode
busctl --user get-property org.headbridge /org/headbridge org.headbridge.Manager NoiseMode

# Set ANC mode to Transparency
busctl --user call org.headbridge /org/headbridge org.headbridge.Manager SetAnc s "transparency"

# Stream real-time state changes
busctl --user monitor org.headbridge
```

### 5.2 Python Client (`pydbus`)
```python
#!/usr/bin/env python3
import json
from gi.repository import GLib
from pydbus import SessionBus

bus = SessionBus()
manager = bus.get("org.headbridge", "/org/headbridge")

print(f"Device: {manager.DeviceName} ({manager.Vendor})")
print(f"Battery: {manager.BatteryPercent}% (Case: {manager.BatteryCase}%)")
print(f"ANC Mode: {manager.NoiseMode}")

# Toggle ANC mode
manager.SetAnc("anc")

# Subscribe to real-time StateChanged signals
def on_state_changed(json_str):
    state = json.loads(json_str)
    print(f"Update: Battery {state.get('batteryPercent')}% | Mode: {state.get('noiseMode')}")

manager.StateChanged.connect(on_state_changed)
GLib.MainLoop().run()
```

### 5.3 Rust Client (`zbus`)
```rust
use zbus::Connection;
use zbus::proxy;

#[proxy(
    default_service = "org.headbridge",
    default_path = "/org/headbridge",
    interface = "org.headbridge.Manager"
)]
trait ResonateManager {
    async fn get_status(&self) -> zbus::Result<String>;
    async fn set_anc(&self, mode: &str) -> zbus::Result<(bool, String)>;
    #[zbus(property)]
    async fn battery_percent(&self) -> zbus::Result<i32>;
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let connection = Connection::session().await?;
    let proxy = ResonateManagerProxy::new(&connection).await?;
    
    println!("Battery: {}%", proxy.battery_percent().await?);
    proxy.set_anc("transparency").await?;
    Ok(())
}
```
