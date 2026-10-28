# OpenLight Studio

Non-destructive RAW processor and photo catalog with a built-in MCP server and a pluggable AI/agent router — a first working slice of [the functional & system specification](openlight_studio_functional_and_system_specification.md).

Stack: Tauri v2 · Rust · React 19 · TypeScript · Tailwind CSS · SQLite (WAL) · rawler.

## Run

Requires Node 20+ and Rust (stable).

```bash
npm install
npm run tauri dev
```

Release bundle: `npm run tauri build`. Rust checks: `cd src-tauri && cargo test --lib`.

The catalog lives in `~/Library/Application Support/org.openlight.studio/` (`catalog.olcat`, `thumbs/`, `settings.json`, and `remote/` for originals downloaded from Immich or Google Photos); set `OPENLIGHT_DATA_DIR` to use another catalog folder.

| Command | Purpose |
|---|---|
| `openlight-studio` | GUI |
| `openlight-studio --import <folder>` | Headless import (recursive; files stay in place) |
| `openlight-studio --mcp-stdio` | MCP over stdio — forwards to the running app, or edits the catalog headless when it is closed |

## Immich and Google Photos

The Library sidebar has **Import from Immich…** and **Import from Google Photos…**. Originals download to `remote/<source>/<album>/` in the catalog folder and are imported like a folder. Re-importing fetches only what is new. Formats OpenLight can't open (HEIC, video) are skipped.

To send edits back, choose **Upload to Immich** or **Upload to Google Photos** as the destination in the Export dialog. OpenLight renders to a temporary folder, uploads the files and deletes them.

- **Immich:** enter the server URL (e.g. `http://192.168.1.10:2283`) and an API key from *Account Settings → API Keys* with `album.read`, `asset.read` and `asset.download`, plus `asset.upload` to upload. The key is stored in the OS keychain. Import an album or your favorites. Immich's search covers your own library, so albums other people share with you import only your photos in them. Uploads go to your timeline, dated with the photo's capture time (read as this computer's time zone).
- **Google Photos:** Google's Library API no longer lists your photos, so OpenLight uses the Picker API: you choose photos in Google Photos in your browser, then OpenLight downloads them (Google removes location metadata). Uploads go through the Library API, which still accepts new photos. One-time setup in Google Cloud Console:
  1. Enable the **Photos Picker API**, and the **Photos Library API** to upload.
  2. Configure the OAuth consent screen and add yourself as a test user.
  3. Create an OAuth client ID of type **Desktop app**.
  4. Paste the client ID and secret into OpenLight and choose **Sign in with Google**.

  While the consent screen is in *Testing*, Google expires the sign-in after 7 days; sign in again or switch it to *In production*. Exports carry no EXIF yet, so Google Photos files uploads under the upload date.

## Connect an agent

The app serves MCP (Streamable HTTP, loopback only, bearer token) at `http://127.0.0.1:41920/mcp`. The **AI & MCP** tab shows the token and copy-ready configs, e.g.:

```bash
claude mcp add --transport http openlight http://127.0.0.1:41920/mcp --header "Authorization: Bearer <token>"
```

Tools: `catalog_query`, `culling_batch_update`, `develop_apply_parameters`, `mask_create_and_edit`, `batch_export`, `photo_inspect`. Resources: `openlight://catalog/summary`, `selection/active`, `photo/{id}/state|telemetry|preview`. Each mutating call is one undoable history step and is recorded in the photo's `ai_provenance_log`; the open Develop view updates live.

The Develop view's **✦ AI** bar runs a prompt through the selected provider: CLI agents (Claude CLI, Antigravity, custom argv templates) edit through MCP; HTTP providers (Ollama, LM Studio/vLLM/llama.cpp/OpenAI/OpenRouter, Anthropic, Gemini) return an `AiTuningResponse` that is validated, clamped and applied, with an optional verification pass. API keys are stored in the OS keychain (or read from `ANTHROPIC_API_KEY`, `GEMINI_API_KEY`, `OPENAI_API_KEY`, `OPENROUTER_API_KEY`).

## Layout

```
src/                 React UI: Library, Develop, AI & MCP (App.tsx, controls.tsx, api.ts)
src-tauri/src/
  develop.rs         edit graph (spec §6), pipeline, telemetry, export encoders
  catalog.rs         SQLite schema, import/EXIF/thumbnails, history, virtual copies, .olxmp sidecars
  mcp.rs             MCP server: JSON-RPC, tools/resources, HTTP + stdio transports
  ai.rs              provider router (spec §5)
  remote.rs          Immich + Google Photos: import (download originals, then folder import) and export upload
  lib.rs             shared engine state + Tauri commands
```

The pipeline works in linear Rec.2020 float. One Rust implementation renders the viewport, MCP previews/telemetry and full-resolution exports, so agents see exactly what the user sees.

## Spec coverage

**Implemented**

- **DAM (§3.1):** classic catalog import, import from Immich (albums, favorites) and Google Photos (Picker), EXIF, thumbnails; ratings, flags, colour labels, keywords; filters (folder, text, rating, flag, label, edited); virtual copies; `.olxmp` sidecars (read on import, written on edit).
- **Develop (§3.2–3.3):** white balance, exposure, contrast, highlights/shadows/whites/blacks, texture, clarity, dehaze, vibrance (skin-protected), saturation; B&W profile; parametric + point curves (RGB/R/G/B); 8-band colour mixer; Point Color (8 swatches, eyedropper, 3-D range, falloff); 3-way + global colour grading; sharpening, colour noise reduction; vignette; crop and straighten (auto-constrained); history with undo/redo; before/after; copy/paste/sync settings; live RGB histogram with clipping.
- **Masks (§3.4):** linear/radial gradient, luminance range and colour range components with add/subtract/intersect, invert, feather, edge shift, local adjustments and an overlay.
- **Export (§3.5, §3.7):** JPEG, AVIF, 16-bit PNG/TIFF in sRGB, Display P3 or Adobe RGB with embedded ICC; Rec. 2020 PQ HDR for 16-bit files; upload to Immich or Google Photos.
- **MCP (§4) and router (§5):** everything listed above.

**Not yet**

- **AI models:** semantic masks (subject/sky/people/landscape/objects), depth, generative remove/expand, AI denoise, super resolution, lens blur, face/eye culling, CLIP search (`catalog_query` falls back to keywords), and the `ai_reconstruct_and_retouch` tool. These need ONNX models that are not bundled.
- **Colour and optics:** DCP/ICC camera profiles and LUTs, lens corrections, upright, calibration, embedded-ICC input colour management.
- **Library features:** smart collections, survey/compare/reference views, brush masks.
- **HDR and output:** HDR display, merges, tethering, soft proofing, C2PA signing, video, and the Map/Book/Print/Slideshow/Web modules.
- **Sync and web:** cloud sync, EXIF in exports (capture date for Google Photos uploads), the web/PWA build, Adobe `crs:` XMP interop.
- **Performance SLAs (§7):** the pipeline runs on the CPU (rayon) over a 1600 px proxy rather than `wgpu`, and the grid is not virtualised for very large catalogs.
