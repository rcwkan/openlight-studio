# Chapter 8: Testing, Verification, Multi-Platform Packaging & CI/CD

This chapter provides comprehensive instructions for running automated tests, validating TypeScript/Rust builds, packaging production installers across macOS, Linux, and Windows, and configuring CI/CD pipelines.

---

## 1. Automated Testing & Verification

OpenLight Studio uses a dual-layer testing strategy: Rust cargo unit/integration tests for backend math and database operations, and TypeScript/Vite compilation for frontend type safety.

### 1.1 The Master Test Script
Run all tests and build validations with a single command:
```bash
npm test
# Or directly:
./scripts/test.sh
```

This script:
1. Runs `cargo test --manifest-path src-tauri/Cargo.toml`.
2. Runs `npm run build` (`tsc && vite build`) to verify that all React components and TypeScript types compile without errors.

### 1.2 Running Rust Backend Tests
To test specific Rust modules or test cases:

```bash
# Run all tests in the workspace
cargo test --manifest-path src-tauri/Cargo.toml

# Run tests in develop.rs (tonal math, color matrix, spline tests)
cargo test --manifest-path src-tauri/Cargo.toml develop::tests

# Run tests in catalog.rs (SQLite WAL, XMP sidecars, migrations)
cargo test --manifest-path src-tauri/Cargo.toml catalog::tests

# Run tests with stdout output enabled
cargo test --manifest-path src-tauri/Cargo.toml -- --nocapture
```

### 1.3 Running Frontend Type Checks & Production Bundle
```bash
# Typecheck only (no emit)
npx tsc --noEmit

# Full production build (generates dist/ folder)
npm run build
```

---

## 2. Multi-Platform Packaging & Distribution

OpenLight Studio packages native production distributions for **macOS**, **Linux**, and **Windows**.

### 2.1 macOS Production Packaging (`scripts/build.sh`)
On macOS, [`scripts/build.sh`](../scripts/build.sh) compiles the optimized binary, bundles the `.app`, and creates a signed/compressed `.dmg` using macOS `hdiutil`:

```bash
./scripts/build.sh
```

**Packaging Steps Executed**:
1. Builds frontend assets with Vite into `dist/`.
2. Compiles optimized Rust binary (`profile.release` with LTO).
3. Packages the macOS Application Bundle via Tauri: `src-tauri/target/release/bundle/macos/OpenLight Studio.app`.
4. Creates a compressed DMG installer (`UDZO` format): `src-tauri/target/release/bundle/dmg/OpenLight Studio_0.1.0_<arch>.dmg`.
5. Copies finalized artifacts into `dist-packages/macos/`.

### 2.2 Linux Production Packaging
There are two ways to package for Linux:

#### Option A: Containerized Docker Build (`scripts/build-linux.sh`)
Recommended on any platform (including macOS) to build clean Linux `.deb` packages without installing local GTK development headers:
```bash
./scripts/build-linux.sh
```
This builds an isolated builder image via [`scripts/Dockerfile.linux`](../scripts/Dockerfile.linux) based on Debian, compiles the binary, and outputs the `.deb` package to `dist-packages/linux/deb/OpenLight Studio_0.1.0_<arch>.deb`.

#### Option B: Native Linux System Build
On a native Linux workstation with prerequisites installed:
```bash
npx tauri build
```
Artifacts are generated under:
* `src-tauri/target/release/bundle/deb/*.deb`
* `src-tauri/target/release/bundle/appimage/*.AppImage`

### 2.3 Windows Packaging
On a Windows 10/11 system with Visual Studio C++ Build Tools:
```bash
npm run tauri build
```
Artifacts generated:
* `src-tauri/target/release/bundle/msi/*.msi`
* `src-tauri/target/release/bundle/nsis/*.exe`

---

## 3. Continuous Integration & CD Pipeline

OpenLight Studio features a multi-platform GitHub Actions workflow defined in [`.github/workflows/build-all-platforms.yml`](../.github/workflows/build-all-platforms.yml).

### 3.1 Build Matrix Architecture
The pipeline runs concurrently across 4 build runners on every push to `main` and release tag:

```
+--------------------------------------------------------------------------------------------------+
| GITHUB ACTIONS WORKFLOW: build-all-platforms.yml                                                |
+--------------------------------------------------------------------------------------------------+
          |                        |                        |                        |
+---------v---------+    +---------v---------+    +---------v---------+    +---------v---------+
| macOS Apple Silicon|    | macOS Intel       |    | Linux Ubuntu      |    | Windows x64       |
| macos-latest      |    | macos-13          |    | ubuntu-22.04      |    | windows-latest    |
| aarch64-darwin    |    | x86_64-darwin     |    | x86_64-linux-gnu  |    | x86_64-pc-windows |
| Outputs:          |    | Outputs:          |    | Outputs:          |    | Outputs:          |
| - .dmg, .app      |    | - .dmg, .app      |    | - .deb, .AppImage |    | - .msi, .exe      |
+-------------------+    +-------------------+    +-------------------+    +-------------------+
```

### 3.2 Pipeline Steps Summary
1. **Checkout**: Checks out source code using `actions/checkout@v4`.
2. **Node Setup**: Configures Node.js 22 LTS with `npm` caching.
3. **Rust Toolchain**: Configures Rust stable for the target triple with `dtolnay/rust-toolchain`.
4. **Rust Cache**: Caches target dependencies via `swatinem/rust-cache@v2`.
5. **System Dependencies**: Automatically provisions `libwebkit2gtk-4.1-dev`, `libsoup-3.0-dev`, `libgtk-3-dev`, and `librsvg2-dev` on Linux runners.
6. **Tauri Action**: Runs `tauri-apps/tauri-action@v0`, building frontend and native bundles.
7. **Artifact Upload**: Publishes zipped build artifacts for all platforms.

