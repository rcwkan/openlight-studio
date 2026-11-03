<div align="center">

# OpenLight Studio

**Professional, non-destructive RAW photo processor and digital asset manager (DAM) with a built-in Model Context Protocol (MCP) server that AI agents can drive.**

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Release](https://img.shields.io/badge/Release-v0.1.0--beta-success.svg)](https://github.com/rcwkan/openlight-studio/releases)
[![Website](https://img.shields.io/badge/Website-openlight.makecade.com-25c6d8.svg)](https://openlight.makecade.com/)
[![Platforms](https://img.shields.io/badge/Platforms-macOS%20%7C%20Linux%20%7C%20Windows-lightgrey.svg)](https://github.com/rcwkan/openlight-studio/releases)

[**Download App**](#-download--installation) · [**User Guide**](#-user-guide--key-features) · [**AI & Agents**](#-ai--agent-integration) · [**Developer Guide**](#-developer--contributor-guide) · [**Website**](https://openlight.makecade.com/)

</div>

---

## 📸 What is OpenLight Studio?

OpenLight Studio is a modern, local-first RAW photo editor and library organizer designed for photographers who want complete control over their images:

* 🔒 **Zero Lock-In & Local-First**: Your RAW files stay right where they are. Edits are stored in non-destructive `.olxmp` sidecars and a local SQLite catalog. No mandatory subscriptions, no vendor lock-in.
* 🌐 **Connect All Your Photos in One Place**: Browse local folders, NAS/network shares (SMB, NFS, WebDAV), self-hosted **Immich** servers, Google Photos picks, and cloud storage (Amazon S3, Backblaze B2, Cloudflare R2, Nextcloud, Dropbox, OneDrive).
* 🎨 **32-Bit Linear Floating-Point Color Engine**: Process RAWs in linear Rec.2020 color space with precision 8-band color mixing, 3D Point Color, tone curves, Upright perspective correction, and analog film profiles with adjustable strength.
* 🧠 **In-Place AI Retouching & Neural Denoise**: Paint away objects with generative fill (Stable Diffusion 1.5, LaMa) and clean noise with NAFNet/SCUNet. Non-destructive adjustment layers that never create bloated duplicate master files.
* 🤖 **AI You Can Talk To**: A built-in Model Context Protocol (MCP) server allows external AI agents (Claude, Antigravity, local Ollama, LM Studio) to inspect histograms, trace precision masks, and execute photo editing workflows naturally.

---

## ⬇️ Download & Installation

Pre-built beta release packages are available for direct download from the [GitHub Releases Page](https://github.com/rcwkan/openlight-studio/releases):

| Operating System | Architecture | Package Format | Download Link |
| :--- | :---: | :---: | :--- |
| **macOS** (11.0+) | Apple Silicon (M1/M2/M3/M4) | `.dmg` Installer | [**Download for macOS (Apple Silicon)**](https://github.com/rcwkan/openlight-studio/releases/download/v0.1.0-beta/OpenLight-Studio-v0.1.0-beta-macOS-arm64.dmg) |
| **macOS** (11.0+) | Intel (x86_64) | `.app.tar.gz` Archive | [**Download for macOS (Intel)**](https://github.com/rcwkan/openlight-studio/releases/download/v0.1.0/OpenLight.Studio_aarch64.app.tar.gz) |
| **Linux** (Debian/Ubuntu) | ARM64 (aarch64) | `.deb` Package | [**Download for Linux (.deb ARM64)**](https://github.com/rcwkan/openlight-studio/releases/download/v0.1.0-beta/OpenLight-Studio-v0.1.0-beta-Linux-arm64.deb) |
| **Linux** (Debian/Ubuntu) | x86_64 (amd64) | `.deb` Package | [**Download for Linux (.deb x86_64)**](https://github.com/rcwkan/openlight-studio/releases/download/v0.1.0/OpenLight.Studio_0.1.0_amd64.deb) |
| **Linux** (Universal) | x86_64 | `.AppImage` | [**Download for Linux (.AppImage)**](https://github.com/rcwkan/openlight-studio/releases/download/v0.1.0/OpenLight.Studio_0.1.0_amd64.AppImage) |
| **Linux** (Fedora/RHEL) | x86_64 | `.rpm` Package | [**Download for Linux (.rpm)**](https://github.com/rcwkan/openlight-studio/releases/download/v0.1.0/OpenLight.Studio-0.1.0-1.x86_64.rpm) |
| **Windows** (10/11) | x64 | `.msi` / `.exe` | [**Download for Windows (Installer)**](https://github.com/rcwkan/openlight-studio/releases/download/v0.1.0/OpenLight.Studio_0.1.0_x64-setup.exe) |

> [!TIP]
> **macOS Security Tip**: Because OpenLight Studio is independently built open-source software, macOS Gatekeeper may show a notice on first launch. Right-click (or Control-click) `OpenLight Studio.app` and select **Open**, or run `xattr -cr "/Applications/OpenLight Studio.app"` in Terminal.

---

## 📖 User Guide & Key Features

### 1. Library & Photo Viewer
- **Zero-Import Browsing**: Add folders directly via **＋ › Folder…**. Files remain in their original directories; new images dropped into folders are picked up automatically.
- **Grid & Full Photo Viewer**:
  - Press <kbd>G</kbd> for the multi-column thumbnail grid with ratings, flags, and color labels.
  - Press <kbd>V</kbd> or <kbd>Enter</kbd> (or double-click) to open the high-resolution **Photo Viewer** with smooth pan, 100% pixel zoom, slideshow mode, and filmstrip navigation.
- **Fast Culling**: Rate photos with <kbd>0</kbd>–<kbd>5</kbd>, flag picks (<kbd>P</kbd>), unflag (<kbd>U</kbd>), or mark rejects (<kbd>X</kbd>).

### 2. Multi-Source DAM: Drives, Clouds & Immich
- **Network Drives (NAS)**: Mount SMB (`smb://nas.local/photos`), AFP, NFS, or WebDAV shares. OpenLight displays connection health and edits photos in place across your network.
- **Self-Hosted Immich Integration**: Connect with your Immich server URL and API key. Browse your library and albums in place without duplicating storage. **Save to Immich** uploads edits and stacks them cleanly over the original. Star ratings and flags sync bidirectionally.
- **Cloud Storage**: Connect Nextcloud, ownCloud, Synology, pCloud, Koofr, Amazon S3, Cloudflare R2, Backblaze B2, Dropbox, or OneDrive. Downloads occur on-demand into a local cache; exports can upload directly to cloud folders.
- **Google Photos**: Import picks securely via the Google Photos Picker API without sharing your broader cloud account.
- **Duplicate & Backup** (*Settings › Storage › Duplicate*): Copy images between local folders, network shares, Immich, and cloud storage with SHA-256 verification, preserving metadata and edits.

### 3. Non-Destructive RAW Develop Workspace
- **Adjust Tab**: Exposure, contrast, highlights, shadows, whites, blacks, texture, clarity, dehaze, vibrance, saturation, and white balance.
- **Live 256-Bin Histogram**: Real-time RGB and luminance histogram with clipping warnings (<kbd>J</kbd> for shadow and highlight clipping overlays).
- **Tone Curves & 8-Band Color Mixer**: Parametric and point tone curves (RGB, R, G, B) alongside 8-hue Hue/Saturation/Luminance control.
- **3D Point Color**: Click the eyedropper on any color in your image to isolate and modify that exact 3D color gamut with controllable falloff.
- **Color Grading**: 3-way split toning (shadows, midtones, highlights) with global luminance and balance sliders.

### 4. AI Retouching & Neural Denoise (The AI Tab)
OpenLight Studio delivers deep-learning generative retouching and denoising directly inside the Develop workspace:
- **Generative AI Object Removal**:
  - Paint over unwanted distractions or people with the smart brush. The mask automatically snaps to edges.
  - Fine-tune brush size and outline dilation (`Grow edge (px)`).
  - Optionally specify what should fill the gap (e.g. *"green grass, mountain rocks, clear sky"*).
  - **Neural Backends**:
    - **Stable Diffusion 1.5 Inpainting**: Generative diffusion fill that synthesizes plausible background content from text descriptions or context.
    - **Moebius (RemovalModel)**: Lightweight 0.2B diffusion inpainting model with learned embedding conditioning for seamless object removal and realistic texture synthesis without needing text descriptions.
    - **LaMa**: High-speed, resolution-robust texture continuation.
  - **Live Progress & Cancellation**: Real-time progress bar displaying execution phase, completed steps, elapsed time, estimated completion time (ETA), and one-click cancel.
- **Neural AI Denoise**:
  - Tame high-ISO sensor noise and grain without softening fine hair, textures, or crisp subject contours.
  - Powered by local **NAFNet** (fast, gentle smartphone/camera denoise) and **SCUNet** (clean & sharp blind denoising) with an adjustable **Strength** slider (0–100).
- **In-Place Adjustment Layers & Zero-Duplicate Architecture**:
  - Traditional editors (like Adobe Lightroom) generate massive duplicate `.dng` or `.tiff` files on disk every time you denoise or remove an object, doubling or tripling catalog storage.
  - OpenLight Studio treats AI operations as **in-place non-destructive adjustment layers**: parameters reside in `.olxmp` sidecar metadata, and rendered patches are stored in a dedicated local application cache. Your master RAW photos are never duplicated or overwritten.
  - **Granular History Management**: Remove individual adjustment steps at any time via the red **✕** button on each history row, or undo/redo at full fidelity.
  - **Storage & Relocation Management** (*Settings › Storage*): Interactive library storage breakdown, cache metrics, one-click cleanup, and full storage folder relocation (move catalog database, thumbnails, AI models, and caches to any internal or external drive with confirmation and seamless zero-restart engine switching).
- **Creative Profile Strength**:
  - All 18 creative and film profiles (Natural, Neutral, Vivid, Portrait, Landscape, B&W, Film, Moody, Street, Food, Architecture, Night, Nature, Teal & Orange, Editorial, Dark Moody, 35mm Film, Fine-Art B&W) include a real-time **Strength** slider (0% to 200%, default 100%).
  - Smoothly scales tone curves, HSL color mixer channels, 3-way color grading, and presence adjustments. Double-click the slider to reset to 100%.

### 5. Advanced Optics & Perspective
- **Lens Corrections**: Distortion, lateral chromatic aberration (auto-measured along contrast edges), defringe, and vignetting.
- **Upright Perspective Solver**: Automatically solve camera pitch, roll, and yaw using Hough transform line detection (**Auto**, **Level**, **Vertical**, **Full**, or **Guided** with 2–4 lines).
- **Constrain Crop**: Automatically shrink crop boundaries to prevent transparent edges after perspective corrections.
- **Film Grain & Camera Calibration**: Fine-grained analog film grain and sensor calibration adjustments.

### 6. Pro Adjustments & Photoshop Tools
The **Pro** tab provides advanced adjustments applied non-destructively on top of RAW develop:
- **Levels**: Per-channel RGB black, gamma, and white points with auto 0.1% clipping.
- **Selective Color**: Adjust CMYK ink balances across 9 color ranges (reds, yellows, greens, cyans, blues, magentas, whites, neutrals, blacks).
- **Channel Mixer & Gradient Maps**: Custom monochrome channel blends and dual-tone gradient maps.
- **Filters**: Gaussian blur, image invert, posterize, threshold, and 90° orientation rotations.

### 7. Automated Functions & Background Batch Jobs
Open the **Auto** bar on the right side of the Library to run background batch operations:
- **Auto Catalog & Culling**: Measures edge-width subject sharpness, highlights/shadows clipping, and noise; automatically detects bursts, selects the sharpest frame, and tags rejects.
- **Auto Horizon Levelling**: Detects tilted horizons and levels images as dedicated history steps.
- **Auto Look Profiles with Strength Slider**: One-click professional color profiling (Natural, Vivid, Portrait, Landscape, Street, Food, Architecture, Night, Nature, Film, Moody, B&W), or picked per photo by type with the AI, with 0–200% strength scaling.
- **Find Blurred Subjects & Duplicates**: Flag missed-focus or motion-blurred shots; identify duplicate and visually similar photos by perceptual hash.
- **Wavelet Denoise & Super Resolution**: Enhance images with multi-scale wavelet denoising and 1.5–4× enlargement into clean 32-bit float master TIFFs.
- **HDR Merge & HDR Toning**: Merge exposure brackets (aligned, deghosted) into a 32-bit float master; tone-map any photo with local adaptation and detail control in Pro.

---

## 🤖 AI & Agent Integration

OpenLight Studio introduces the **Model Context Protocol (MCP)** directly into photo editing. External AI agents can inspect photos, read histogram telemetry, and adjust develop sliders autonomously.

### Built-in MCP Server
The app serves streamable HTTP MCP on loopback:
```text
Endpoint: http://127.0.0.1:41920/mcp
Auth:     Bearer <token> (found in Settings › Agent & MCP)
```

Add OpenLight Studio to Claude Desktop or Claude Code:
```bash
claude mcp add --transport http openlight http://127.0.0.1:41920/mcp --header "Authorization: Bearer <your-token>"
```

### Pluggable AI Router & Local Neural Models
In the Develop view, open the **✦ AI** bar to execute natural language prompts:
- **Interactive Object Masking (✦ Select)**: Click an object (e.g. "castle", "sky", "person") — your selected AI provider traces the boundary and creates an isolated mask.
- **Smart Retouching (✦ Remove)**: Ask the AI to "remove the tourists in the background" — outlines are healed using context-aware texture matching.
- **Dedicated Neural Tooling (*The AI Tab*)**: Direct in-app controls for Generative Object Removal (Stable Diffusion 1.5, LaMa) and AI Denoise (NAFNet, SCUNet) with live progress tracking and cancel.
- **Local Neural Model Manager (*Settings › AI models*)**: Download and configure ONNX models directly from Hugging Face with automatic hardware recommendations (DirectML, Apple Silicon, CPU) and SHA-256 integrity verification.
- **Autonomous Agent MCP Tools**: Autonomous AI agents can query histograms, apply develops (`develop_auto`), perform non-destructive generative fill (`ai_reconstruct_and_retouch`), or execute inpainting object removal (`retouch_remove_objects`) via the standard MCP server.
- **Supported LLM Providers**:
  - **CLI Agents**: Claude CLI (`claude`), Antigravity CLI (`antigravity` / `agy`), custom CLI templates.
  - **Local Models**: Ollama (`qwen2.5-vl`, `llama3.2-vision`), LM Studio, vLLM, llama.cpp.
  - **Cloud APIs**: Anthropic Claude, Google Gemini, OpenAI, OpenRouter.

---

## 💻 Developer & Contributor Guide

### Tech Stack
- **Desktop Shell**: [Tauri v2](https://tauri.app) with custom asset protocol and restricted IPC capabilities.
- **Backend Core**: Rust (stable) with multi-threaded [rayon](https://crates.io/crates/rayon) parallelism and [rusqlite](https://crates.io/crates/rusqlite).
- **RAW Decoder**: [rawler](https://crates.io/crates/rawler) for high-speed camera RAW demosaicing.
- **Color Pipeline**: 32-bit linear Rec.2020 float pipeline with LittleCMS / moxcms profile transforms.
- **Frontend**: [React 19](https://react.dev), [TypeScript](https://www.typescriptlang.org), [Tailwind CSS v4](https://tailwindcss.com), and [Vite](https://vite.dev).

### Prerequisites
- **Node.js**: v20 or newer (`node -v`)
- **Rust**: Stable toolchain (`rustup update stable`)

### Running in Development

```bash
# Clone the repository
git clone https://github.com/rcwkan/openlight-studio.git
cd openlight-studio

# Start development mode (Vite frontend + native Tauri window)
./scripts/dev.sh
```

### Building for Production

```bash
# Build macOS application bundle (.app) and DMG installer
./scripts/build.sh

# Run comprehensive test suite (Rust unit tests + TypeScript typecheck)
./scripts/test.sh

# Run headless MCP stdio server
./scripts/mcp.sh
```

### Headless CLI Usage

OpenLight Studio can run headlessly for batch imports or as a stdio MCP server for agent scripts:

```bash
# Launch GUI
openlight-studio

# Headless recursive import (scans directory, generates previews in place)
openlight-studio --import /path/to/photos

# Headless MCP stdio bridge
openlight-studio --mcp-stdio
```

### Project Structure

```
├── src/                     # React 19 Frontend
│   ├── App.tsx              # App shell, navigation, global hotkeys
│   ├── Library.tsx          # Digital asset management & grid
│   ├── PhotoViewer.tsx      # Full-screen photo viewer, zoom, filmstrip
│   ├── Develop.tsx          # RAW develop workspace & histogram
│   ├── AiDenoise.tsx        # AI object removal, neural denoise & progress
│   ├── Advanced.tsx         # Optics, lens correction & Upright
│   ├── Pro.tsx              # Levels, selective color, Photoshop tools
│   ├── Auto.tsx             # Auto functions, 18 profiles & strength slider
│   ├── AutoView.tsx         # Batch auto culling & processing jobs
│   ├── Storage.tsx          # Cloud storage, network drives, clean up
│   └── Agent.tsx            # AI provider router & MCP token settings
├── src-tauri/               # Rust Core Backend
│   ├── src/develop.rs       # Non-destructive edit graph & pixel pipeline
│   ├── src/aimodel.rs       # ONNX Runtime, NAFNet, SCUNet & SD inpainting
│   ├── src/catalog.rs       # SQLite schema, EXIF, .olxmp sidecars
│   ├── src/vision.rs        # Subject detection, blur & noise analysis
│   ├── src/auto.rs          # Auto culling, rating & batch functions
│   ├── src/mcp.rs           # Embedded HTTP & stdio MCP server
│   ├── src/cloud.rs         # S3, WebDAV, Dropbox & OneDrive storage
│   ├── src/remote.rs        # Immich integration & Google Photos picker
│   └── src/ai.rs            # AI agent router & command execution
├── html/                    # Marketing landing page & static site
├── cloudflare-worker/       # Cloudflare edge proxy & router
├── guide/                   # In-depth architectural & developer guides
└── scripts/                 # Build, test, run, and deployment scripts
```

For in-depth architectural specifications and component guides, consult [`guide/README.md`](guide/README.md) and the [Functional & System Specification](openlight_studio_functional_and_system_specification.md).

---

## 📄 License & Security

- **Application Core License**: OpenLight Studio is released under the open-source [MIT License](LICENSE).
- **Security Policy**: See [`SECURITY.md`](SECURITY.md) for vulnerability disclosure guidelines.

### Third-Party AI Model Licenses & Disclaimers

OpenLight Studio allows optional, on-demand downloading of third-party neural network weights for local inference. Each model operates under its own license and terms of use:

| Model | Purpose | Author / Lab | License | Terms & Disclaimer |
| :--- | :--- | :--- | :--- | :--- |
| **Stable Diffusion 1.5 Inpainting** | Generative Fill | RunwayML / Stability AI | CreativeML OpenRAIL-M | Subject to OpenRAIL-M use-based restrictions. |
| **Moebius (RemovalModel)** | Inpainting & Object Removal | Duan & Xu et al. (HUST) / SimonW | Apache-2.0 | Permissive open-source license. |
| **LaMa** | Inpainting & Object Removal | Suvorov et al. (Samsung Research) | Apache-2.0 | Permissive open-source license. |
| **NAFNet** | Fast Neural Denoise | Megvii Technology | MIT License | Permissive open-source license. |
| **SCUNet** | Real-World Camera Denoise | Zhang et al. | Apache-2.0 | Permissive open-source license. |
