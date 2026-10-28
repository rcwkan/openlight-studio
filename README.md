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

The catalog lives in `~/Library/Application Support/org.openlight.studio/` (`catalog.olcat`, `thumbs/`, `settings.json`, and `remote/` for Immich originals opened for editing and Google Photos picks); set `OPENLIGHT_DATA_DIR` to use another catalog folder.

| Command | Purpose |
|---|---|
| `openlight-studio` | GUI |
| `openlight-studio --import <folder>` | Headless import (recursive; files stay in place) |
| `openlight-studio --mcp-stdio` | MCP over stdio — forwards to the running app, or edits the catalog headless when it is closed |

## Folders, Immich and Google Photos

The Library sidebar shows your sources directly; there is no import step. Edits are always non-destructive: originals are never overwritten.

- **Folders:** **＋** adds a folder. Its photos stay where they are, and opening the folder picks up files added since. Edits live in the catalog and an `.olxmp` sidecar next to each file.
- **Immich:** connect with the server URL (e.g. `http://192.168.1.10:2283`) and an API key from *Account Settings → API Keys* with `album.read`, `asset.read`, `asset.view`, `asset.download`, `asset.upload`, `asset.update` and `stack.create`. The key is stored in the OS keychain. Your favorites and albums appear in the sidebar and are browsed in place with the server's thumbnails, 250 photos at a time: the next page loads as you scroll near the end, so even very large albums open at once. Opening or exporting a photo downloads it once into `remote/Immich/originals/`; HEIC photos open from Immich's full-size JPEG rendition (or its preview when the server doesn't generate full-size images). **Save to Immich** uploads the edit and stacks it on top of the original, so Immich shows the edit and keeps the original. Star ratings and flags sync both ways: stars ↔ Immich rating, pick ↔ favorite, reject ↔ Immich's rejected rating. Changes here are sent right away (a failure shows in the status bar), and opening an album takes Immich's current values. Virtual copies keep their own ratings locally. Immich's search covers your own library, so albums other people share with you show only your photos in them.
- **Google Photos:** Google no longer lets apps list your library or change existing photos, so OpenLight uses the Picker API. **Pick photos…** opens Google Photos in your browser; your picks are downloaded (Google removes location metadata) and listed under *Picked photos*. Saving an edit uploads it as a new photo through the Library API. One-time setup in Google Cloud Console:
  1. Enable the **Photos Picker API**, and the **Photos Library API** to save edits.
  2. Configure the OAuth consent screen and add yourself as a test user.
  3. Create an OAuth client ID of type **Desktop app**.
  4. Paste the client ID and secret into OpenLight and choose **Sign in with Google**.

  While the consent screen is in *Testing*, Google expires the sign-in after 7 days; sign in again or switch it to *In production*.

To save edits, use **Save to Immich…** on an Immich photo, or pick **Upload to Immich** / **Upload to Google Photos** as the destination in the Export dialog. OpenLight renders to a temporary folder, uploads and deletes the files. Exports carry the capture date as EXIF, so uploads land on the day the photo was taken (16-bit TIFF exports have no EXIF).

## Connect an agent

The app serves MCP (Streamable HTTP, loopback only, bearer token) at `http://127.0.0.1:41920/mcp`. The **AI & MCP** tab shows the token and copy-ready configs, e.g.:

```bash
claude mcp add --transport http openlight http://127.0.0.1:41920/mcp --header "Authorization: Bearer <token>"
```

Tools: `catalog_query`, `culling_batch_update`, `develop_apply_parameters`, `mask_create_and_edit`, `retouch_remove_objects`, `batch_export`, `photo_inspect`. Resources: `openlight://catalog/summary`, `selection/active`, `photo/{id}/state|telemetry|preview`. Each mutating call is one undoable history step and is recorded in the photo's `ai_provenance_log`; the open Develop view updates live.

The Develop view's **✦ AI** bar runs a prompt through the selected provider: CLI agents (Claude CLI, Antigravity, custom argv templates) edit through MCP; HTTP providers (Ollama, LM Studio/vLLM/llama.cpp/OpenAI/OpenRouter, Anthropic, Gemini) return an `AiTuningResponse` that is validated, clamped and applied, with an optional verification pass. Prompts can select or remove things: "select the castle" makes the model trace an outline that becomes a mask (snapped to the photo's edges and shown with its overlay), "remove the people" heals each outline from a nearby matching area (listed under **Remove**). The models see the full uncropped frame, which is what mask and removal coordinates refer to. API keys are stored in the OS keychain (or read from `ANTHROPIC_API_KEY`, `GEMINI_API_KEY`, `OPENAI_API_KEY`, `OPENROUTER_API_KEY`).

## Layout

```
src/                 React UI: Library, Develop, AI & MCP (App.tsx, controls.tsx, api.ts)
src-tauri/src/
  develop.rs         edit graph (spec §6), pipeline, telemetry, export encoders
  catalog.rs         SQLite schema, import/EXIF/thumbnails, history, virtual copies, .olxmp sidecars
  mcp.rs             MCP server: JSON-RPC, tools/resources, HTTP + stdio transports
  ai.rs              provider router (spec §5)
  remote.rs          Immich browsed in place (originals on demand, saves stacked), Google Photos picker, uploads
  lib.rs             shared engine state + Tauri commands
```

The pipeline works in linear Rec.2020 float. One Rust implementation renders the viewport, MCP previews/telemetry and full-resolution exports, so agents see exactly what the user sees.

## Spec coverage

**Implemented**

- **DAM (§3.1):** folders as live sources (files stay in place), Immich albums and favorites browsed in place, Google Photos picks, EXIF, thumbnails; ratings, flags, colour labels, keywords; filters (folder, text, rating, flag, label, edited); virtual copies; `.olxmp` sidecars (read on import, written on edit).
- **Develop (§3.2–3.3):** white balance, exposure, contrast, highlights/shadows/whites/blacks, texture, clarity, dehaze, vibrance (skin-protected), saturation; B&W profile; parametric + point curves (RGB/R/G/B); 8-band colour mixer; Point Color (8 swatches, eyedropper, 3-D range, falloff); 3-way + global colour grading; sharpening, colour noise reduction; vignette; crop and straighten (auto-constrained); history with undo/redo; before/after; copy/paste/sync settings; live RGB histogram with clipping.
- **Masks (§3.4):** linear/radial gradient, luminance range, colour range and AI-outline (polygon, edge-snapped by a colour model and guided filter) components with add/subtract/intersect, invert, feather, edge shift, local adjustments and an overlay.
- **Remove (§3.4.2):** object removal from AI outlines, healed from an automatically chosen nearby area and colour-matched to the surroundings; touching outlines heal as one.
- **Export (§3.5, §3.7):** JPEG, AVIF, 16-bit PNG/TIFF in sRGB, Display P3 or Adobe RGB with embedded ICC and the EXIF capture date (not TIFF); Rec. 2020 PQ HDR for 16-bit files; upload to Immich or Google Photos.
- **MCP (§4) and router (§5):** everything listed above.

**Not yet**

- **AI models:** model-based semantic masks (subject/sky/people/landscape; AI providers trace outlines instead), depth, generative remove/expand (removal heals from nearby texture and cannot invent hidden structure), AI denoise, super resolution, lens blur, face/eye culling, CLIP search (`catalog_query` falls back to keywords), and the `ai_reconstruct_and_retouch` tool. These need ONNX models that are not bundled.
- **Colour and optics:** DCP/ICC camera profiles and LUTs, lens corrections, upright, calibration, embedded-ICC input colour management.
- **Library features:** smart collections, survey/compare/reference views, brush masks.
- **HDR and output:** HDR display, merges, tethering, soft proofing, C2PA signing, video, and the Map/Book/Print/Slideshow/Web modules.
- **Sync and web:** cloud sync, camera EXIF in exports (make, model, exposure) and EXIF in TIFF, the web/PWA build, Adobe `crs:` XMP interop.
- **Performance SLAs (§7):** the pipeline runs on the CPU (rayon) over a 1600 px proxy rather than `wgpu`, and the grid is not virtualised for very large catalogs.
