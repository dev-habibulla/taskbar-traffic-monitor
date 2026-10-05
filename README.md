# Taskbar Monitor

Windows 11-এর taskbar-এর পাশে (notification area-র বামে) ছোট দুই লাইনের সিস্টেম মনিটর।  
TrafficMonitor-এর মতো ফিল, কিন্তু পুরোটা Rust + native Windows API — Electron / Tauri / WebView কিছুই নেই।

```
↑ 835.7 KB/s   CPU 59%
↓  41.6 KB/s   MEM 48%
```

রিলিজ বিল্ড সাধারণত ~৩৫০ KB-এর কাছাকাছি থাকে। ব্যাকগ্রাউন্ড/কার্ড/বর্ডার নেই — শুধু টেক্সট, ক্লিক-থ্রু, তাই নিচের taskbar ব্যবহার করা যায়।

## Download

বিল্ড করতে না চাইলে সরাসরি exe নাও (Windows x64, ~348 KB):

- **Latest:** [taskbar-monitor.exe](https://github.com/dev-habibulla/taskbar-traffic-monitor/releases/latest/download/taskbar-monitor.exe)
- সব ভার্সন: [Releases](https://github.com/dev-habibulla/taskbar-traffic-monitor/releases)

ডাউনলোড করে ডাবল-ক্লিক করলেই চলবে (tray আইকন + taskbar overlay)।

## কী কী দেখায়

- Upload / Download speed (ইউনিট অটো: `B/s` → `KB/s` → `MB/s` …)
- CPU % এবং Memory %
- চাইলে Total Traffic (এই সেশনের মোট বাইট) — ডিফল্টে বন্ধ
- প্রতিটা মেট্রিক আলাদা করে চালু/বন্ধ করা যায়; বন্ধ করলে খালি জায়গা থাকে না
- System tray আইকন + রাইট-ক্লিক মেনু
- Windows-এর সাথে স্টার্ট (HKCU `Run` — admin লাগে না)
- Light / Dark থিম ফলো করে
- DPI + মাল্টি-মনিটর / taskbar মুভ / auto-hide হ্যান্ডল করে

---

## লোকাল রান ও বিল্ড

নিচের ধাপগুলো পরেরবার নিজে চালাতে সুবিধা হবে বলে ধাপে ধাপে লিখে রাখলাম।

### ১) যা লাগবে আগে

| জিনিস | কেন |
| ----- | --- |
| **Windows 10/11** | অ্যাপটা Win32 / Direct2D ভিত্তিক — অন্য OS-এ চলবে না |
| **Rust (stable)** | `rustup` দিয়ে ইনস্টল: https://rustup.rs |
| **MSVC Build Tools** | `cargo build` লিংকের সময় Visual C++ toolchain লাগে (Visual Studio Installer → “Desktop development with C++”, অথবা Build Tools) |

চেক করতে টার্মিনালে:

```powershell
# Rust ইনস্টল আছে কিনা
rustc --version
cargo --version

# MSVC টুলচেইন দেখতে (Windows-এ সাধারণত এটাই ডিফল্ট)
rustup show
```

### ২) প্রজেক্ট ফোল্ডারে যাও

```powershell
# রিপো ক্লোন করে থাকলে, সেই ফোল্ডারে ঢুকো
cd path\to\taskbar-monitor
```

### ৩) ডেভেলপমেন্ট মোডে রান (লোকাল টেস্ট)

ডিবাগ বিল্ড — দ্রুত কম্পাইল, কনসোলে লগ থাকে। প্রতিদিনের টেস্টের জন্য এটাই সুবিধাজনক।

```powershell
# প্রথমবার ডিপেন্ডেন্সি ডাউনলোড + কম্পাইল একটু সময় নিতে পারে
cargo run

# শুধু বিল্ড করতে চাইলে (এক্সিকিউটেবল রান হবে না):
cargo build

# ডিবাগ exe এখানে থাকে:
#   .\target\debug\taskbar-monitor.exe
```

আলাদা করে exe চালাতে:

```powershell
.\target\debug\taskbar-monitor.exe
```

### ৪) রিলিজ বিল্ড (ছোট, ফাস্ট বাইনারি)

সাইজ/স্পিড অপটিমাইজড প্রোফাইল (`Cargo.toml`-এ `opt-level = "z"`, LTO, strip)।  
বন্ধুকে দিয়ে টেস্ট বা নিজে রোজ ব্যবহার — এটাই ব্যবহার করো।

```powershell
# রিলিজ বিল্ড
cargo build --release

# বিল্ড শেষে চালাও
.\target\release\taskbar-monitor.exe
```

শুধু এক কমান্ডে বিল্ড + রান:

```powershell
cargo run --release
```

### ৫) টেস্ট চালানো

```powershell
cargo test
```

কিছু টেস্ট লাইভ সিস্টেম (taskbar, নেটওয়ার্ক, রেজিস্ট্রি স্যান্ডবক্স) ছোঁয় — তাই Windows মেশিনে চালানোই ভালো।

### ৬) ক্লিন বিল্ড (সমস্যা হলে)

কখনো অদ্ভুত কম্পাইল এরর হলে:

```powershell
# target ফোল্ডার মুছে আবার বিল্ড
cargo clean
cargo build --release
```

> নোট: `target/` ফোল্ডারটা বিশাল হতে পারে (কয়েকশো MB)। এটা গিটে যায় না (`.gitignore`-এ আছে) — লোকালেই থাকে।

---

## সেটিংস কোথায় থাকে

`%APPDATA%\TaskbarMonitor\config.json`

Tray → রাইট ক্লিক থেকে: Show / Hide, Settings, Pause, Start with Windows, Exit।  
Settings উইন্ডো বন্ধ করলে মনিটরিং বন্ধ হয় না — ব্যাকগ্রাউন্ডে চলতে থাকে।

ডিফল্ট দ্রুত রেফারেন্স:

| Setting            | Default |
| ------------------ | ------- |
| Upload / Download  | on      |
| CPU / Memory       | on      |
| Total Traffic      | off     |
| Update interval    | 1000 ms |
| Theme              | System  |
| Start with Windows | on      |
| Start minimized    | on      |

---

## কীভাবে taskbar-এ বসে

Windows 11-এ deskband নেই, তাই এটা আসলে taskbar-এর উপরে একটা **transparent, click-through, topmost overlay** — notification area (`TrayNotifyWnd`)-এর ঠিক বামে।

- Layered window + per-pixel alpha → শুধু অক্ষর দেখা যায়
- Click-through → মাউস নিচের taskbar-এ চলে যায়
- Alt+Tab-এ আসে না, ফোকাস চুরি করে না
- টেক্সট: DirectWrite · তীর: Direct2D

Explorer রিস্টার্ট / ডিসপ্লে চেঞ্জ / DPI চেঞ্জ হলে নিজে থেকে আবার জায়গা ধরে নেয়। একসাথে একটাই ইনস্ট্যান্স চলে (named mutex)।

---

## ফোল্ডার ম্যাপ (দ্রুত ওরিয়েন্ট)

```
src/
├── main.rs              স্টার্টআপ + সিঙ্গেল-ইনস্ট্যান্স
├── app.rs               ট্রে / রেন্ডার / মেসেজ লুপ
├── format.rs            স্পিড / পার্সেন্ট ফরম্যাটিং
├── monitor/             CPU, RAM, নেটওয়ার্ক স্যাম্পল
├── taskbar/             প্লেসমেন্ট + Direct2D/DirectWrite ওভারলে
├── tray/                ট্রে আইকন + মেনু
├── settings/            কনফিগ মডেল + %APPDATA% সেভ
├── startup/             Windows Run-key অটোস্টার্ট
└── ui/                  সেটিংস উইন্ডো
```

দুই থ্রেড: UI থ্রেড (উইন্ডো/ট্রে/মেসেজ লুপ) + একটা স্যাম্পলার থ্রেড। Busy-poll নেই।

---

## সীমাবদ্ধতা

- প্রাইমারি taskbar টার্গেট — সেকেন্ডারি মনিটরের taskbar এখনো ড্র করে না
- Tray মেনু `tray-icon` ক্রেট দিয়ে; ওভারলেটা নিজেদের Win32/Direct2D কোড

---

## লাইসেন্স

MIT
