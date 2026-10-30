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

The catalog lives in `~/Library/Application Support/org.openlight.studio/` (`catalog.olcat`, `thumbs/`, `settings.json`, and `remote/` for Immich originals opened for editing, Google Photos picks and folders downloaded from cloud storage); set `OPENLIGHT_DATA_DIR` to use another catalog folder.

| Command | Purpose |
|---|---|
| `openlight-studio` | GUI |
| `openlight-studio --import <folder>` | Headless import (recursive; files stay in place) |
| `openlight-studio --mcp-stdio` | MCP over stdio — forwards to the running app, or edits the catalog headless when it is closed |

## Sources: folders, network drives, cloud storage, Immich and Google Photos

The Library sidebar shows your sources directly, grouped under **Library**, **Folders** (folders and network drives) and **Cloud & services**; there is no import step. It lists only what you have added or connected, and **＋** on a group offers the rest. Edits are always non-destructive: originals are never overwritten. Network drives, cloud storage and what happens to new photos are set up in **Settings › Locations**.

- **Folders:** **＋ › Folder…** adds a folder. Its photos stay where they are, and opening the folder picks up files added since. Edits live in the catalog and an `.olxmp` sidecar next to each file.
- **Network drives:** add a share by its address (`smb://nas.local/photos` for Windows, Samba and most NAS; `afp://`; `nfs://`; `https://` for WebDAV) and, if you like, the folder where it appears once connected; OpenLight suggests `/Volumes/<share>` on macOS, the GVfs mount on Linux and the UNC path on Windows. A share connected some other way (a mapped drive) needs only its folder. Drives are listed in the Library with a dot showing whether they are connected; opening one connects it through the system (Finder, GVfs or Explorer, which ask for the password and remember it) and picks up new photos like a folder. Photos are read and edited in place.
- **Cloud storage:** WebDAV (Nextcloud, ownCloud, Synology, pCloud, Koofr, …), S3-compatible storage (Amazon S3, Backblaze B2, Cloudflare R2, Wasabi, MinIO; requests are signed with AWS Signature V4), Dropbox and OneDrive. Choose a folder in an account and it is downloaded into `remote/Cloud/` with its subfolders and added to the catalog; opening it again downloads only what's new. Exports can be uploaded into any folder of an account, never replacing a file there. Passwords, keys and sign-ins are stored with the other credentials, never in `settings.json`; the downloads can be cleared in *Usage & clean up* (the originals stay in the cloud). Dropbox and OneDrive sign in with OAuth (PKCE) through an app you register:
  - **Dropbox:** create an app at dropbox.com/developers/apps (Scoped access) with `files.metadata.read`, `files.content.read` and `files.content.write`, and paste its App key. After you allow it in the browser, Dropbox shows a code to paste into OpenLight.
  - **OneDrive:** register an app in the Microsoft Entra admin center for personal and work accounts, add the platform *Mobile and desktop applications* with the redirect URI `http://localhost` and the delegated permissions `Files.ReadWrite` and `offline_access`, and paste its Application (client) ID.
- **Immich:** connect with the server URL (e.g. `http://192.168.1.10:2283`) and an API key from *Account Settings → API Keys* with `album.read`, `asset.read`, `asset.view`, `asset.download`, `asset.upload`, `asset.update` and `stack.create`. The key is stored in the OS keychain. Your favorites and albums appear in the sidebar and are browsed in place with the server's thumbnails, 250 photos at a time: the next page loads as you scroll near the end, so even very large albums open at once. Opening or exporting a photo downloads it once into `remote/Immich/originals/`; HEIC photos open from Immich's full-size JPEG rendition (or its preview when the server doesn't generate full-size images). **Save to Immich** uploads the edit and stacks it on top of the original, so Immich shows the edit and keeps the original. Star ratings and flags sync both ways: stars ↔ Immich rating, pick ↔ favorite, reject ↔ Immich's rejected rating. Changes here are sent right away (a failure shows in the status bar), and opening an album takes Immich's current values. Virtual copies keep their own ratings locally. Immich's search covers your own library, so albums other people share with you show only your photos in them.
- **Google Photos:** Google no longer lets apps list your library or change existing photos, so OpenLight uses the Picker API. **Pick photos…** opens Google Photos in your browser; your picks are downloaded (Google removes location metadata) and listed under *Picked photos*. Saving an edit uploads it as a new photo through the Library API. One-time setup in Google Cloud Console:
  1. Enable the **Photos Picker API**, and the **Photos Library API** to save edits.
  2. Configure the OAuth consent screen and add yourself as a test user.
  3. Create an OAuth client ID of type **Desktop app**.
  4. Paste the client ID and secret into OpenLight and choose **Sign in with Google**.

  While the consent screen is in *Testing*, Google expires the sign-in after 7 days; sign in again or switch it to *In production*.

To save edits, use **Save to Immich…** on an Immich photo, or pick **Upload to Immich**, **Upload to Google Photos** or a cloud storage account as the destination in the Export dialog. OpenLight renders to a temporary folder, uploads and deletes the files. Exports carry the capture date as EXIF, so uploads land on the day the photo was taken (16-bit TIFF exports have no EXIF).

**Duplicate** (*Settings › Storage › Duplicate*) copies photos from one place to another: from a folder, a network drive, the selected photos, an Immich album or the favorites, Google Photos picks or a cloud folder, to a folder, a network drive, Immich, Google Photos or a cloud folder. Options: subfolders; only picks or a minimum rating; skip files the destination has already (same name and size, and date when dates are kept); edits along as `.olxmp` sidecars; keep the files' dates; verify each copy by its SHA-256; add the copies to the catalog with the originals' ratings, flags, labels and keywords; then cull them. Nothing is overwritten or deleted: a different file with the same name is kept beside it as `name-1.ext`.

**Automation:** with *Auto Catalog & Culling of new photos* on, photos new to the catalog are culled in the background (progress in the status bar) with the settings of Auto › Catalog & Culling. It can be switched on per source: folders and network drives, cloud storage and Google Photos picks, Immich albums, and copies made by Duplicate.

## Connect an agent

The app serves MCP (Streamable HTTP, loopback only, bearer token) at `http://127.0.0.1:41920/mcp`. The **AI & MCP** tab shows the token and copy-ready configs, e.g.:

```bash
claude mcp add --transport http openlight http://127.0.0.1:41920/mcp --header "Authorization: Bearer <token>"
```

Tools: `catalog_query`, `culling_batch_update`, `develop_apply_parameters`, `develop_auto`, `auto_run` (the [auto functions](#auto-functions)), `mask_create_and_edit`, `retouch_remove_objects`, `ai_reconstruct_and_retouch` (denoise and super resolution), `batch_export`, `photo_inspect`. Resources: `openlight://catalog/summary`, `selection/active`, `photo/{id}/state|telemetry|preview`. Each mutating call is one undoable history step and is recorded in the photo's `ai_provenance_log`; the open Develop view updates live.

The Develop view's **✦ AI** bar runs a prompt through the selected provider: CLI agents (Claude CLI, Antigravity, custom argv templates) edit through MCP; HTTP providers (Ollama, LM Studio/vLLM/llama.cpp/OpenAI/OpenRouter, Anthropic, Gemini) return an `AiTuningResponse` that is validated, clamped and applied, with an optional verification pass. Prompts can select or remove things: "select the castle" makes the model trace an outline that becomes a mask (snapped to the photo's edges and shown with its overlay), "remove the people" heals each outline from a nearby matching area (listed under **Remove**). The models see the full uncropped frame, which is what mask and removal coordinates refer to.

**✦ Select** (S) does the same by clicking: each click asks the selected provider to trace the object under the cursor (it sees the photo with the click marked) and creates a separate selection for that object; ⌥-click subtracts from the active selection. Selections are stored with the photo in the **Selections** tab: each AI-selected object (e.g. castle, sky) is listed as its own selection that can be selected to finetune independently in Adjust, unselected, toggled on/off or deleted without asking the AI again. While a selection (any mask) is selected, every Develop panel — Basic, tone curve, colour mixer, Point Color, colour grading, detail — tunes only inside it, on top of the photo's own settings; **Done** or Esc goes back to the whole photo. API keys are stored in the OS keychain (or read from `ANTHROPIC_API_KEY`, `GEMINI_API_KEY`, `OPENAI_API_KEY`, `OPENROUTER_API_KEY`).

The Develop view's **Advanced** tab (next to Adjust and Selections) holds Lightroom's advanced panels, always for the whole photo. They are part of the edit graph, so exports, thumbnails, MCP previews and agents (`lens`, `transform`, `calibration`, `grain` and `detail.luminance_*` in `develop_apply_parameters`) see the same result:

- **Lens corrections:** distortion; lateral chromatic aberration (red/cyan, blue/yellow) with **auto**, which measures the fringes along edges in linear sRGB; defringe (purple and green amount and hue range, only along high-contrast edges); lens vignetting (amount, midpoint).
- **Transform:** **Upright** Auto, Level, Vertical and Full find straight edges (Hough transform) and solve the camera tilt, pan and roll that make them vertical or horizontal; **Guided** solves from 2–4 lines you drag on the photo. Manual vertical, horizontal, rotate, aspect, scale and offset, and **Constrain crop**, which shrinks the crop until no empty corners show. The frame centre keeps its scale, as in Lightroom.
- **Calibration:** shadows tint and red/green/blue primary hue and saturation (a white-preserving matrix).
- **Noise reduction:** luminance (edge-preserving) with detail, plus colour.
- **Grain:** amount, size, roughness; sized relative to the frame, so the preview matches the export.
- **Spot removal** (Q): click a spot to heal it from a matching nearby area (listed with the other removals); **Visualize spots** shows edges in black and white so dust stands out.
- **Soft proofing:** gamut warning for sRGB, Display P3 or Adobe RGB (violet), clipping warning (J; red highlights, blue shadows). The overlays stay on in every tab until turned off from the toolbar.

The **Pro** tab adds Photoshop's most used adjustments that the Lightroom panels lack, applied on top of Adjust. With a selection selected they tune only it, like an adjustment layer with a layer mask (e.g. blur an inverted subject for a soft background). Agents set them with `levels`, `hue_saturation`, `selective_color`, `channel_mixer`, `gradient_map`, `filters` and `image_rotation`:

- **Levels:** black / gamma / white and output levels for RGB and each channel, with **auto** (each channel's black and white point, 0.1% clipped).
- **Hue / Saturation:** master hue, saturation, lightness, and **Colorize**.
- **Selective color:** cyan / magenta / yellow / black for reds, yellows, greens, cyans, blues, magentas, whites, neutrals and blacks, relative or absolute.
- **Channel mixer** (with monochrome) and **Gradient map** (two colours, amount).
- **Filters:** Gaussian blur, invert, posterize, threshold.
- **Image rotation:** 90° turns and horizontal / vertical flips, whole photo only; the crop turns with the photo.

The RGB adjustments work on display sRGB values like a Photoshop sRGB document, so out-of-sRGB colours clip while one is on. Lightroom already covers the rest of Photoshop's everyday set: Brightness/Contrast, Exposure, Curves, Vibrance and Shadows/Highlights (Basic, tone curve), Color Balance (colour grading), Black & White (B&W + colour mixer), Photo Filter (white balance), Replace Color (Point Color), Unsharp Mask and Reduce Noise (Detail, Advanced), Add Noise (grain), healing and Content-Aware Fill (spot removal, AI remove) and Select Subject / Color Range (Selections).

The **Auto** tab gathers every automatic adjustment, always for the whole photo:

- **✦ Auto** and **auto to a profile** (Natural, Neutral, Vivid, Portrait, Landscape, B&W, Film, Moody): white balance and tone measured on the photo, then the profile's look. They replace white balance, Basic, presence and colour/B&W plus the panels a profile uses (Film: tone curve, grading, grain; Moody: vignette), so a profile applied twice changes nothing and other edits stay. Auto white balance follows the near-grey surfaces and holds back where the photo has none or its brightest pixels disagree, so flowers, sunsets and night skies keep their colour. Agents use it as the `develop_auto` MCP tool. The Basic panel's **auto** is the same auto tone.
- **One at a time:** white balance, tone, levels, chromatic aberration.
- **Auto rotate:** Straighten (Upright Level), Upright, Verticals and Full perspective; **✦ Fix orientation** asks the AI to turn a sideways or upside-down photo upright.
- **Auto select & remove:** ✦ Select subject, ✦ Select sky, ✦ Remove people and ✦ AI fine tune send a prompt through the AI bar. ✦ AI fine tune first levels a tilted horizon (Upright Level, its own history step), leaving photos without straight edges, already level or with a straighten or Upright of your own as they are; **Remove** heals away the objects of any AI selection (from ✦ Select) and drops it.

Geometry is applied last (distortion, transform, straighten, image rotation, crop in one resample), so masks, removals and guides stay pinned to the content in the uncorrected frame, which is what AI Select, the spot tool and Guided Upright show.

## Auto functions

The **Auto** buttons on the Library's right bar open six functions in the right panel, next to the grid; each runs on the selected photos, the photos in view or the whole catalog. Each has its basic settings and **Advanced settings** (remembered, with *Reset to defaults*), shows its progress and can be cancelled, keeping what is done. Results list each photo with quick actions and **Show in Library**. Agents run the same functions with the `auto_run` MCP tool, overriding any setting per call. Measurements are cached in the catalog per file (until the file changes), so running another function on the same photos is quick.

- **Catalog & Culling:** measures each photo's sharpness on its main subject (edge width, so a soft but well-exposed photo is told apart from a sharp one), clipped highlights and shadows, noise and colours; groups bursts (taken within a few seconds and alike by perceptual hash) and picks the best of each; rejects blurred and blown-out or black photos; rates the rest 1–5 stars; adds catalog keywords (year, month, time of day, orientation, main colours, black and white, high ISO, long exposure, burst, blurry). *Advanced:* where sharpness is judged (1024 px, 2048 px or 100%), the main subject or the sharpest part, the clipping limit, the burst window and similarity, rejecting the rest of a burst, colour labels (green picks, yellow borderline, red rejects), subject and scene keywords from the AI provider, and leaving photos you rated, flagged or labelled alone.
- **Non-destructive Editing:** white balance and tone measured on each photo, then a look (Natural, Neutral, Vivid, Portrait, Landscape, B&W, Film, Moody) at a chosen strength, one undoable history step per photo. *Advanced:* which parts change, Upright (level, balanced, verticals, full), colour fringe removal, noise reduction from the noise measured on the photo, skipping photos you edited, or editing a new virtual copy.
- **AI Masking:** adds Subject, Sky, Background and People selections to each photo's Selections, ready to tune in Develop. The built-in estimate finds the subject by its colour contrast with the photo's borders and the sky by growing from the top edge, and traces their outlines; the AI provider can trace them instead (People need it). *Advanced:* a starting adjustment (subject brighter, sky deeper, background darker), feather, edge shift and replacing earlier selections of the same name.
- **AI Denoise & Super Resolution:** writes a new master `<name>-Enhanced.tif` next to the original (32-bit float linear light for RAW files, 16-bit otherwise) with the original's edits, rating and keywords; the original stays untouched. The built-in engine denoises in Oklab with an à trous wavelet shrinkage tuned to the noise measured at each brightness, and enlarges 1.5–4× with Lanczos, iterative back-projection and edge-aware sharpening, up to a megapixel limit. An external AI engine can run instead, e.g. `realesrgan-ncnn-vulkan -i {{INPUT}} -o {{OUTPUT}} -s {{SCALE}}` (run without a shell). *Advanced:* fine detail, colour noise, sharpening, the megapixel limit, the file format and folder.
- **Find blurred subjects:** finds photos whose main subject is out of focus, motion-blurred (blur in one direction: camera shake or a moving subject) or missed by the focus while something else is sharp; then lists them, flags them as rejects, labels them red or adds the keyword `blurry`. *Advanced:* the size sharpness is judged at, the subject from the built-in estimate or the AI provider, the motion threshold.
- **Find duplicates:** exact copies (same file contents by SHA-256) across folders and sources, and look-alikes (perceptual hash), optionally only among photos taken within some minutes of each other; keeps the best of each group (picks, stars and edits, then size; or the largest, sharpest, oldest or newest) and acts on the extra copies.

## Layout

```
src/                 React UI: Library, Develop (+ Advanced.tsx, Pro.tsx, Auto.tsx), Auto functions (AutoView.tsx),
                     AI & MCP, Settings › Storage (Settings.tsx, Storage.tsx) (App.tsx, controls.tsx, api.ts)
src-tauri/src/
  develop.rs         edit graph (spec §6), pipeline, telemetry, export encoders
  catalog.rs         SQLite schema, import/EXIF/thumbnails, history, virtual copies, .olxmp sidecars, analysis cache
  auto.rs            the auto functions, their settings and cancellable jobs
  vision.rs          image analysis: edge-width blur, subject, noise, exposure, colours, perceptual hash, outlines
  enhance.rs         wavelet denoise, super resolution, external AI engines
  storage.rs         storage report and clean-up, network drives, Duplicate
  cloud.rs           WebDAV, S3 (SigV4), Dropbox and OneDrive: browse, download, upload
  mcp.rs             MCP server: JSON-RPC, tools/resources, HTTP + stdio transports
  ai.rs              provider router (spec §5)
  remote.rs          Immich browsed in place (originals on demand, saves stacked), Google Photos picker, uploads
  lib.rs             shared engine state + Tauri commands
```

The pipeline works in linear Rec.2020 float. One Rust implementation renders the viewport, MCP previews/telemetry and full-resolution exports, so agents see exactly what the user sees.

## Spec coverage

**Implemented**

- **DAM (§3.1):** folders and network drives as live sources (files stay in place), Immich albums and favorites browsed in place, Google Photos picks, cloud storage folders (WebDAV, S3, Dropbox, OneDrive) downloaded on demand, Duplicate between any of them; EXIF, thumbnails; ratings, flags, colour labels, keywords; filters (folder, text, rating, flag, label, edited); virtual copies; `.olxmp` sidecars (read on import, written on edit).
- **Auto functions:** Catalog & Culling (subject sharpness, exposure, bursts, stars, picks, rejects, keywords; also on new photos), non-destructive auto editing, Subject/Sky/Background/People masking, denoise and super resolution into a new linear float master, blurred-subject and duplicate finding, each with advanced settings and as the `auto_run` MCP tool.
- **Develop (§3.2–3.3):** white balance, exposure, contrast, highlights/shadows/whites/blacks, texture, clarity, dehaze, vibrance (skin-protected), saturation; B&W profile; parametric + point curves (RGB/R/G/B); 8-band colour mixer; Point Color (8 swatches, eyedropper, 3-D range, falloff); 3-way + global colour grading; sharpening, colour noise reduction; vignette; crop and straighten (auto-constrained); history with undo/redo; before/after; copy/paste/sync settings; live RGB histogram with clipping.
- **Masks (§3.4):** linear/radial gradient, luminance range, colour range and AI-outline (polygon, edge-snapped by a colour model and guided filter) components with add/subtract/intersect, invert, feather, edge shift, local adjustments and an overlay; click-to-select with the AI; full per-mask tuning (every Develop panel except crop and effects).
- **Remove (§3.4.2):** object removal from AI outlines, healed from an automatically chosen nearby area and colour-matched to the surroundings; touching outlines heal as one; click-to-heal spot removal with Visualize Spots.
- **Optics and calibration (§3.3, §3.6):** manual distortion, lateral chromatic aberration (auto-measured), defringe, lens vignetting; Upright Auto/Level/Vertical/Full/Guided and manual perspective with constrained crop; camera calibration; luminance noise reduction; grain; soft-proofing gamut and clipping warnings.
- **Export (§3.5, §3.7):** JPEG, AVIF, 16-bit PNG/TIFF in sRGB, Display P3 or Adobe RGB with embedded ICC and the EXIF capture date (not TIFF); Rec. 2020 PQ HDR for 16-bit files; upload to Immich, Google Photos or cloud storage.
- **MCP (§4) and router (§5):** everything listed above.

**Not yet**

- **AI models:** model-based semantic masks (Auto masking estimates subject and sky, AI providers trace outlines), depth, generative remove/expand (removal heals from nearby texture and cannot invent hidden structure), learned denoise and super resolution (the built-in ones are classical; a command-line engine such as Real-ESRGAN can be plugged in), lens blur, face/eye detection for culling (sharpness is judged on the estimated main subject), CLIP search (`catalog_query` falls back to keywords), and the generative tasks of `ai_reconstruct_and_retouch`. These need ONNX models that are not bundled.
- **Colour and optics:** DCP/ICC camera profiles and LUTs, lens-profile corrections (Lensfun/LCP database; the manual corrections are in), flat-field correction, embedded-ICC input colour management. Upright assumes a ~32mm-equivalent lens instead of reading the EXIF focal length, so its aspect may need the Aspect slider.
- **Library features:** smart collections, survey/compare/reference views, brush masks.
- **HDR and output:** HDR display, HDR/panorama merges, tethering, soft proofing against printer ICC profiles (gamut warnings for the export colour spaces are in), C2PA signing, video, and the Map/Book/Print/Slideshow/Web modules.
- **Sync and web:** two-way cloud sync (cloud folders are downloaded and exports uploaded; edits and deletions do not travel back), camera EXIF in exports (make, model, exposure) and EXIF in TIFF, the web/PWA build, Adobe `crs:` XMP interop.
- **Performance SLAs (§7):** the pipeline runs on the CPU (rayon) over a 1600 px proxy rather than `wgpu`, and the grid is not virtualised for very large catalogs.
