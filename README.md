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

## লোকাল ইনস্টল / রান / বিল্ড

পরেরবার আবার সেটআপ করতে লাগলে এই ধাপগুলো ফলো করো।  
`target/` এবং Cargo ক্যাশ গিটে যায় না — জায়গা বাঁচাতে লোকালে মুছে রাখা যায়; দরকার হলে নিচের কমান্ড দিয়ে আবার তৈরি হবে।

### ১) যা লাগবে আগে (একবার ইনস্টল)

| জিনিস | কেন |
| ----- | --- |
| **Windows 10/11** | অ্যাপটা Win32 / Direct2D ভিত্তিক — অন্য OS-এ চলবে না |
| **Rust (stable)** | `rustup` দিয়ে ইনস্টল: https://rustup.rs |
| **MSVC Build Tools** | Visual Studio Installer → “Desktop development with C++” (অথবা Build Tools) |

```powershell
# Rust ইনস্টল চেক
rustc --version
cargo --version
rustup show
```

Rust না থাকলে:

```powershell
# https://rustup.rs থেকে rustup-init.exe চালাও, অথবা:
winget install Rustlang.Rustup
```

### ২) সোর্স নাও + প্রজেক্টে ঢুকো

```powershell
git clone https://github.com/dev-habibulla/taskbar-traffic-monitor.git
cd taskbar-traffic-monitor
```

আগেই ক্লোন করা থাকলে শুধু:

```powershell
cd path\to\taskbar-monitor
git pull
```

### ৩) আবার বিল্ড / ইনস্টল (ক্যাশ মুছে থাকলেও)

প্রথমবার (বা `target/` মুছে দিলে) ডিপেন্ডেন্সি ডাউনলোড + কম্পাইল সময় নিতে পারে।

```powershell
# ডেভ টেস্ট — দ্রুত কম্পাইল, কনসোল লগ থাকে
cargo run

# শুধু ডিবাগ বিল্ড
cargo build
# exe: .\target\debug\taskbar-monitor.exe

# রোজ ব্যবহার / ছোট বাইনারি (~348 KB)
cargo build --release
.\target\release\taskbar-monitor.exe

# এক কমান্ডে রিলিজ বিল্ড + রান
cargo run --release
```

### ৪) টেস্ট

```powershell
cargo test
```

### ৫) ডিস্ক খালি রাখতে (অপ্রয়োজনীয় বিল্ড ফাইল)

বিল্ড শেষে / অ্যাপ ইনস্টল হয়ে গেলে `target/` রাখার দরকার নেই — কয়েকশো MB থেকে কয়েক GB পর্যন্ত ফুলে যেতে পারে।

```powershell
# অ্যাপ বন্ধ করে তারপর:
cargo clean
# অথবা ফোল্ডার সরাসরি মুছো:  Remove-Item -Recurse -Force .\target
```

পরে আবার চাইলে শুধু:

```powershell
cargo build --release
```

> নোট: `target/` গিটে কমিট হয় না (`.gitignore`)। সোর্স (`src/`, `Cargo.toml`, `Cargo.lock`, `README`) রাখলেই যথেষ্ট।

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
