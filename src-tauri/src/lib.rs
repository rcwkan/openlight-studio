mod ai;
mod auto;
mod catalog;
mod cloud;
mod develop;
mod enhance;
mod mcp;
mod remote;
mod storage;
mod vision;

use develop::{Develop, Img, Opts, es};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard, OnceLock, atomic::AtomicBool},
};
use tauri::{Emitter, Manager};

pub const APP_ID: &str = "org.openlight.studio";
/// Long edge of the in-memory develop proxy.
pub const PROXY_EDGE: u32 = 1600;

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct Settings {
    pub mcp_enabled: bool,
    pub mcp_port: u16,
    pub mcp_token: String,
    pub providers: Vec<ai::Provider>,
    pub active_provider: String,
    /// Connected Immich server; its API key is in the keychain.
    pub immich_url: String,
    /// OAuth client of the signed-in Google account; secret and refresh token are in the keychain.
    pub google_client_id: String,
    /// Advanced settings of the auto functions, and what runs on new photos.
    pub auto: auto::AutoSettings,
    /// Shares on a NAS or server, browsed in place like folders.
    pub network_drives: Vec<storage::NetworkDrive>,
    /// Cloud storage accounts; their secrets are in the keychain.
    pub clouds: Vec<cloud::Account>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            mcp_enabled: true,
            mcp_port: 41920,
            mcp_token: new_token(),
            providers: ai::default_providers(),
            active_provider: "cli:claude".into(),
            immich_url: String::new(),
            google_client_id: String::new(),
            auto: Default::default(),
            network_drives: vec![],
            clouds: vec![],
        }
    }
}

impl Settings {
    pub fn load(dir: &Path) -> Settings {
        let mut s: Settings = fs::read_to_string(dir.join("settings.json"))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        for p in &mut s.providers {
            if p.id == "cli:antigravity" && (p.command.starts_with("antigravity run") || p.command.is_empty()) {
                p.command = "antigravity -p {{AGENT_PROMPT}} --dangerously-skip-permissions".into();
            }
        }
        s
    }
}

pub fn new_token() -> String {
    let mut b = [0u8; 24];
    getrandom::fill(&mut b).expect("OS random source");
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Catalog location; `OPENLIGHT_DATA_DIR` selects another catalog folder.
pub fn data_dir() -> PathBuf {
    std::env::var_os("OPENLIGHT_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| dirs::data_dir().unwrap_or_else(std::env::temp_dir).join(APP_ID))
}

/// Shared engine state used by the GUI commands, the MCP server and the AI router.
pub struct Core {
    db: Mutex<rusqlite::Connection>,
    pub dir: PathBuf,
    pub settings: Mutex<Settings>,
    pub selection: Mutex<Vec<i64>>,
    pub mcp_status: Mutex<String>,
    /// Cancel flags of the running jobs (auto functions, duplicating), by job id.
    pub jobs: Mutex<HashMap<String, Arc<AtomicBool>>>,
    cache: Mutex<Vec<(String, Arc<Img>)>>,
    // ponytail: one full-size decode at a time bounds memory; a decode pool if imports/exports need throughput
    decode_lock: Mutex<()>,
    app: OnceLock<tauri::AppHandle>,
}

impl Core {
    pub fn open(dir: PathBuf) -> Result<Self, String> {
        fs::create_dir_all(dir.join("thumbs")).map_err(es)?;
        let core = Core {
            db: Mutex::new(catalog::open(&dir.join("catalog.olcat"))?),
            settings: Mutex::new(Settings::load(&dir)),
            dir,
            selection: Default::default(),
            mcp_status: Mutex::new("not started".into()),
            jobs: Default::default(),
            cache: Default::default(),
            decode_lock: Mutex::new(()),
            app: OnceLock::new(),
        };
        core.save_settings()?; // persists the token generated on first run
        Ok(core)
    }

    pub fn db(&self) -> MutexGuard<'_, rusqlite::Connection> {
        self.db.lock().unwrap()
    }

    pub fn save_settings(&self) -> Result<(), String> {
        let path = self.dir.join("settings.json");
        let s = serde_json::to_string_pretty(&*self.settings.lock().unwrap()).map_err(es)?;
        fs::write(&path, s).map_err(es)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600)); // holds the MCP token
        }
        Ok(())
    }

    pub fn thumb_path(&self, id: i64) -> PathBuf {
        self.dir.join("thumbs").join(format!("{id}.jpg"))
    }

    pub fn emit(&self, event: &str, payload: Value) {
        if let Some(app) = self.app.get() {
            let _ = app.emit(event, payload);
        }
    }

    /// Activity line for the AI & MCP console.
    pub fn log(&self, source: &str, message: impl Into<String>) {
        self.emit("agent-log", json!({"source": source, "message": message.into()}));
    }

    pub fn photo(&self, id: i64) -> Result<catalog::Photo, String> {
        let f = catalog::Filter { ids: Some(vec![id]), ..Default::default() };
        catalog::query(&self.db(), &f)?.pop().ok_or_else(|| format!("photo {id} not found"))
    }

    pub fn develop(&self, id: i64) -> Result<Develop, String> {
        catalog::get_develop(&self.db(), id)
    }

    /// The file behind a catalog path; an `immich://` photo downloads on first use.
    pub fn local_file(&self, path: &str) -> Result<PathBuf, String> {
        match remote::immich_id(path) {
            Some(id) => remote::immich_original(self, id),
            None => Ok(PathBuf::from(path)),
        }
    }

    /// Full decode of a file, one at a time (full-size decodes are memory-heavy).
    pub fn decode(&self, file: &Path, max_edge: Option<u32>) -> Result<Img, String> {
        let _g = self.decode_lock.lock().unwrap();
        develop::load(file, max_edge)
    }

    /// Decoded working-space proxy, cached for the last few files.
    pub fn proxy(&self, path: &str) -> Result<Arc<Img>, String> {
        if let Some((_, img)) = self.cache.lock().unwrap().iter().find(|(p, _)| p == path) {
            return Ok(img.clone());
        }
        let file = self.local_file(path)?;
        let img = Arc::new(self.decode(&file, Some(PROXY_EDGE))?);
        let mut c = self.cache.lock().unwrap();
        c.retain(|(p, _)| p != path);
        c.insert(0, (path.to_string(), img.clone()));
        c.truncate(4);
        Ok(img)
    }

    /// Render the current edit of a photo at proxy size.
    pub fn render(&self, id: i64, opts: &Opts) -> Result<Img, String> {
        let p = self.photo(id)?;
        let d = self.develop(id)?;
        Ok(develop::render(&*self.proxy(&p.path)?, &d, opts))
    }

    /// Persist an edit as one history step (one undo = one GUI gesture or one AI/MCP call).
    pub fn commit(self: &Arc<Self>, id: i64, d: &Develop, label: &str) -> Result<(), String> {
        catalog::save_develop(&self.db(), id, d, label)?;
        self.refresh(id, d)
    }

    /// Sidecar + thumbnail + UI notification after the stored state changed.
    pub fn refresh(self: &Arc<Self>, id: i64, d: &Develop) -> Result<(), String> {
        let photo = self.photo(id)?;
        catalog::write_sidecar(&photo, d);
        self.emit("photo-changed", json!({"id": id}));
        let (core, d) = (self.clone(), d.clone());
        std::thread::spawn(move || {
            let res = (|| -> Result<(), String> {
                let out = develop::render(&*core.proxy(&photo.path)?, &d, &Opts::default());
                let thumb = catalog::to_dynamic(&out).thumbnail(catalog::THUMB, catalog::THUMB);
                catalog::write_jpeg(&core.thumb_path(id), &thumb)?;
                core.db().execute("UPDATE photos SET thumb_v = thumb_v + 1 WHERE id = ?", [id]).map_err(es)?;
                Ok(())
            })();
            match res {
                Ok(()) => core.emit("thumb-changed", json!({"id": id})),
                Err(e) => eprintln!("thumbnail {id}: {e}"),
            }
        });
        Ok(())
    }

    /// Rating, flag, label or keyword change. Stars and pick/reject of Immich photos are
    /// mirrored to the server in the background; a failure shows in the status bar.
    pub fn cull(self: &Arc<Self>, ids: &[i64], u: &catalog::Culling) -> Result<usize, String> {
        let n = catalog::update(&self.db(), ids, u)?;
        if u.rating.is_some() || u.flag.is_some() {
            let (core, ids) = (self.clone(), ids.to_vec());
            std::thread::spawn(move || {
                if let Err(e) = remote::immich_push(&core, &ids) {
                    eprintln!("immich push: {e}");
                    core.emit("remote-error", json!(format!("Immich was not updated: {e}")));
                }
            });
        }
        Ok(n)
    }

    pub fn virtual_copy(&self, id: i64) -> Result<i64, String> {
        let new = catalog::virtual_copy(&self.db(), id)?;
        let _ = fs::copy(self.thumb_path(id), self.thumb_path(new));
        self.emit("catalog-changed", json!({}));
        Ok(new)
    }

    /// Removes photos and their virtual copies from the catalog; files on disk stay.
    pub fn remove(&self, ids: &[i64]) -> Result<usize, String> {
        let gone = catalog::remove(&self.db(), ids)?;
        for &id in &gone {
            let _ = fs::remove_file(self.thumb_path(id));
        }
        self.emit("catalog-changed", json!({}));
        Ok(gone.len())
    }

    /// Full-resolution export of the current edits. Never overwrites existing files.
    pub fn export(&self, ids: &[i64], folder: &Path, format: &str, color_space: &str, quality: u8, max_edge: Option<u32>) -> Result<Vec<String>, String> {
        fs::create_dir_all(folder).map_err(es)?;
        let ext = develop::file_ext(format);
        let mut out = vec![];
        for (i, &id) in ids.iter().enumerate() {
            let p = self.photo(id)?;
            let d = self.develop(id)?;
            let src = self.decode(&self.local_file(&p.path)?, max_edge)?;
            let img = develop::render(&src, &d, &Opts::default());
            let stem = Path::new(&p.filename).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            let stem = match &p.copy_name {
                Some(c) => format!("{stem}-{}", c.replace(' ', "")),
                None => stem,
            };
            let mut stem: String = stem.chars().filter(|c| !r#"/\:*?"<>|"#.contains(*c) && !c.is_control()).collect();
            if stem.is_empty() {
                stem = "export".into();
            }
            let mut target = folder.join(format!("{stem}.{ext}"));
            let mut n = 1;
            while target.exists() {
                target = folder.join(format!("{stem}-{n}.{ext}"));
                n += 1;
            }
            develop::export(&img, &target, format, color_space, quality, p.captured_at.as_deref())?;
            self.emit("export-progress", json!({"done": i + 1, "total": ids.len()}));
            out.push(target.display().to_string());
        }
        Ok(out)
    }
}

// ---------------------------------------------------------------------------
// Tauri commands
// ---------------------------------------------------------------------------

type St<'a> = tauri::State<'a, Arc<Core>>;

#[tauri::command]
fn app_info(core: St) -> Value {
    let s = core.settings.lock().unwrap();
    json!({
        "data_dir": core.dir,
        "thumbs_dir": core.dir.join("thumbs"),
        "exe": std::env::current_exe().ok(),
        "mcp_url": format!("http://127.0.0.1:{}/mcp", s.mcp_port),
        "mcp_status": *core.mcp_status.lock().unwrap(),
    })
}

#[tauri::command(async)]
fn query_photos(core: St, filter: catalog::Filter) -> Result<Vec<catalog::Photo>, String> {
    catalog::query(&core.db(), &filter)
}

#[tauri::command(async)]
fn list_folders(core: St) -> Result<Vec<(String, i64)>, String> {
    catalog::folders(&core.db())
}

#[tauri::command(async)]
fn import_folder(core: St, path: String) -> Result<usize, String> {
    let c = core.inner().clone();
    let ids = catalog::import(&core, Path::new(&path), &|done, total| {
        c.emit("import-progress", json!({"done": done, "total": total}))
    })?;
    core.emit("catalog-changed", json!({}));
    let n = ids.len();
    auto::after_import(core.inner(), ids, "folders");
    Ok(n)
}

#[tauri::command]
fn set_selection(core: St, ids: Vec<i64>) {
    *core.selection.lock().unwrap() = ids;
}

#[tauri::command(async)]
fn update_photos(core: St, ids: Vec<i64>, update: catalog::Culling) -> Result<usize, String> {
    core.cull(&ids, &update)
}

#[tauri::command(async)]
fn virtual_copy(core: St, id: i64) -> Result<i64, String> {
    core.virtual_copy(id)
}

#[tauri::command(async)]
fn remove_photos(core: St, ids: Vec<i64>) -> Result<usize, String> {
    core.remove(&ids)
}

#[tauri::command(async)]
fn get_develop(core: St, id: i64) -> Result<Develop, String> {
    core.develop(id)
}

#[tauri::command]
fn default_develop() -> Develop {
    Develop::default()
}

/// Preview-only overlays: soft-proof gamut (sRGB | DisplayP3 | AdobeRGB) and clipping warnings,
/// Visualize Spots threshold.
#[derive(Deserialize, Default)]
#[serde(default)]
struct Proof {
    gamut: Option<String>,
    clipping: bool,
    spots: Option<f32>,
}

/// Binary reply: u32 width, u32 height, 4x256 u32 histogram (R, G, B, luma), RGBA8 pixels.
/// The histogram is of the photo itself, not of the overlays.
#[tauri::command(async)]
fn render_preview(core: St, id: i64, develop: Develop, skip_crop: bool, overlay_mask: Option<usize>, full_frame: Option<bool>, proof: Option<Proof>) -> Result<tauri::ipc::Response, String> {
    let photo = core.photo(id)?;
    let opts = Opts { skip_crop, overlay_mask, full_frame: full_frame.unwrap_or(false) };
    let mut out = develop::render(&*core.proxy(&photo.path)?, &develop, &opts);
    let mut rgba = develop::to_rgba8(&out);
    let hist = develop::histogram(&rgba);
    let proof = proof.unwrap_or_default();
    if proof.gamut.is_some() || proof.clipping || proof.spots.is_some() {
        if let Some(t) = proof.spots {
            develop::visualize_spots(&mut out, t);
        }
        develop::proof(&mut out, proof.gamut.as_deref(), proof.clipping);
        rgba = develop::to_rgba8(&out);
    }
    let mut buf = Vec::with_capacity(8 + 4096 + rgba.len());
    buf.extend((out.w as u32).to_le_bytes());
    buf.extend((out.h as u32).to_le_bytes());
    for ch in hist {
        for v in ch {
            buf.extend(v.to_le_bytes());
        }
    }
    buf.extend(rgba);
    Ok(tauri::ipc::Response::new(buf))
}

/// Upright: the Transform that `mode` (off | auto | level | vertical | full | guided) gives `develop`.
#[tauri::command(async)]
fn upright(core: St, id: i64, develop: Develop, mode: String) -> Result<develop::Transform, String> {
    develop::upright(&*core.proxy(&core.photo(id)?.path)?, &develop, &mode)
}

/// Lateral chromatic aberration measured on the photo: [ca_red_cyan, ca_blue_yellow].
#[tauri::command(async)]
fn auto_ca(core: St, id: i64) -> Result<[f32; 2], String> {
    Ok(develop::estimate_ca(&*core.proxy(&core.photo(id)?.path)?))
}

/// Spot removal: heals a circle of `radius` (fraction of the long edge) at (x, y) of the full frame
/// from a matching nearby area; one history step.
#[tauri::command(async)]
fn heal_spot(core: St, id: i64, x: f32, y: f32, radius: f32) -> Result<Develop, String> {
    if !x.is_finite() || !y.is_finite() || !radius.is_finite() || radius <= 0.0 || radius > 1.0 {
        return Err("invalid spot coordinates or radius".to_string());
    }
    let img = core.proxy(&core.photo(id)?.path)?;
    let n = img.w.max(img.h) as f32;
    let (rx, ry) = (radius * n / img.w as f32, radius * n / img.h as f32);
    let points: Vec<[f32; 2]> = (0..24)
        .map(|k| {
            let a = k as f32 / 24.0 * std::f32::consts::TAU;
            [(x + rx * a.cos()).clamp(0.0, 1.0), (y + ry * a.sin()).clamp(0.0, 1.0)]
        })
        .collect();
    let mut d = core.develop(id)?;
    mcp::remove_objects(&core, id, &mut d, &json!([{ "name": "Spot", "points": points }]))?;
    d.clamp();
    core.commit(id, &d, "Spot removal")?;
    Ok(d)
}

/// Auto fine tune to a profile (develop::PROFILES): white balance and tone measured on the photo plus the look.
#[tauri::command(async)]
fn auto_develop(core: St, id: i64, develop: Develop, profile: String) -> Result<Develop, String> {
    develop::auto(&*core.proxy(&core.photo(id)?.path)?, &develop, &profile)
}

#[tauri::command(async)]
fn auto_white_balance(core: St, id: i64) -> Result<develop::WhiteBalance, String> {
    Ok(develop::auto_wb(&*core.proxy(&core.photo(id)?.path)?))
}

/// Auto tone under the white balance of `develop`.
#[tauri::command(async)]
fn auto_tone(core: St, id: i64, develop: Develop) -> Result<develop::BasicTone, String> {
    Ok(develop::auto_tone(&*core.proxy(&core.photo(id)?.path)?, &develop.white_balance))
}

/// Removes the objects in selection `mask` (its AI outlines), each healed from a matching nearby area,
/// and drops the selection; one history step.
#[tauri::command(async)]
fn remove_selection(core: St, id: i64, mask: usize) -> Result<Develop, String> {
    let mut d = core.develop(id)?;
    let m = d.masks.get(mask).ok_or("that selection no longer exists")?;
    let name = m.name.clone();
    let objs: Vec<Value> = m
        .components
        .iter()
        .filter(|c| c.kind == "polygon" && c.op == "add")
        .map(|c| json!({ "name": if c.name.is_empty() { &name } else { &c.name }, "points": c.points }))
        .collect();
    if objs.is_empty() {
        return Err(format!("'{name}' has no AI-selected object to remove: pick one with ✦ Select"));
    }
    mcp::remove_objects(&core, id, &mut d, &json!(objs))?;
    if mask < d.masks.len() {
        d.masks.remove(mask);
    }
    d.clamp();
    core.commit(id, &d, &format!("Remove {name}"))?;
    Ok(d)
}

#[tauri::command(async)]
fn save_develop(core: St, id: i64, develop: Develop, label: String) -> Result<Develop, String> {
    let mut d = develop;
    d.clamp();
    core.commit(id, &d, &label)?;
    Ok(d)
}

#[tauri::command(async)]
fn get_history(core: St, id: i64) -> Result<Vec<Value>, String> {
    catalog::history(&core.db(), id)
}

#[tauri::command(async)]
fn goto_history(core: St, id: i64, hid: i64) -> Result<Develop, String> {
    let d = catalog::goto(&core.db(), id, hid)?;
    core.refresh(id, &d)?;
    Ok(d)
}

#[tauri::command(async)]
fn undo_redo(core: St, id: i64, dir: i32) -> Result<Option<Develop>, String> {
    let d = catalog::step(&core.db(), id, dir)?;
    if let Some(d) = &d {
        core.refresh(id, d)?;
    }
    Ok(d)
}

#[derive(Deserialize)]
struct ExportReq {
    ids: Vec<i64>,
    folder: String,
    format: String,
    color_space: String,
    quality: u8,
    max_edge: Option<u32>,
    /// `immich` | `google` | `cloud:<account id>`: upload the renders instead of keeping them in `folder`.
    upload: Option<String>,
    /// cloud: the folder in the account to upload into.
    #[serde(default)]
    upload_path: String,
}

#[tauri::command(async)]
fn export_photos(core: St, req: ExportReq) -> Result<Vec<String>, String> {
    let export = |folder: &Path| core.export(&req.ids, folder, &req.format, &req.color_space, req.quality.clamp(1, 100), req.max_edge);
    let Some(dest) = &req.upload else { return export(Path::new(&req.folder)) };
    let tmp = std::env::temp_dir().join(format!("openlight-upload-{}", new_token()));
    let res = export(&tmp).and_then(|files| match dest.strip_prefix("cloud:") {
        Some(id) => cloud::upload_files(&core, id, &req.upload_path, &files),
        None => remote::upload(&core, dest, &req.ids, &files),
    });
    let _ = fs::remove_dir_all(&tmp);
    res
}

#[tauri::command]
fn get_settings(core: St) -> Settings {
    core.settings.lock().unwrap().clone()
}

#[tauri::command]
fn save_settings(core: St, mut settings: Settings) -> Result<(), String> {
    {
        let mut s = core.settings.lock().unwrap();
        // drives and cloud accounts change through their own commands (and downloads add folders in the
        // background): the UI's copy of them may be out of date
        settings.network_drives = std::mem::take(&mut s.network_drives);
        settings.clouds = std::mem::take(&mut s.clouds);
        *s = settings;
    }
    core.save_settings()
}

#[tauri::command]
fn regenerate_token(core: St) -> Result<String, String> {
    let t = new_token();
    core.settings.lock().unwrap().mcp_token = t.clone();
    core.save_settings()?;
    Ok(t)
}

#[tauri::command]
fn set_provider_key(core: St, id: String, key: String) -> Result<(), String> {
    ai::set_key(&id, &key)?;
    if let Some(p) = core.settings.lock().unwrap().providers.iter_mut().find(|p| p.id == id) {
        p.has_key = !key.is_empty();
    }
    core.save_settings()
}

#[tauri::command(async)]
fn ai_run(core: St, id: i64, prompt: String, provider: String) -> Result<String, String> {
    ai::run(core.inner(), id, &prompt, &provider)
}

/// AI selection: the object at (x, y) of the full frame joins mask `mask` (or a new one).
#[tauri::command(async)]
fn ai_select(core: St, id: i64, x: f32, y: f32, provider: String, mask: Option<usize>, subtract: bool) -> Result<Value, String> {
    if !x.is_finite() || !y.is_finite() || !(0.0..=1.0).contains(&x) || !(0.0..=1.0).contains(&y) {
        return Err("invalid selection coordinates".to_string());
    }
    let (develop, mask) = ai::select(core.inner(), id, [x, y], &provider, mask, subtract)?;
    Ok(json!({ "develop": develop, "mask": mask }))
}

#[tauri::command(async)]
fn immich_connect(core: St, url: String, key: String) -> Result<Settings, String> {
    remote::immich_connect(&core, &url, &key)
}

#[tauri::command(async)]
fn immich_albums(core: St) -> Result<Vec<Value>, String> {
    remote::immich_albums(&core)
}

/// Browse an album (None: the favorites) in place; returns the catalog ids to show.
#[tauri::command(async)]
fn immich_open(core: St, album: Option<String>, page: Option<u32>) -> Result<Value, String> {
    remote::immich_open(core.inner(), album.as_deref(), page.unwrap_or(1))
}

#[tauri::command(async)]
fn google_connect(core: St, client_id: String, client_secret: String) -> Result<Settings, String> {
    remote::google_connect(&core, &client_id, &client_secret)
}

#[tauri::command(async)]
fn google_import(core: St) -> Result<Value, String> {
    remote::google_import(core.inner())
}

#[tauri::command]
fn remote_disconnect(core: St, source: String) -> Result<Settings, String> {
    remote::disconnect(&core, &source)
}

/// Run an auto function (auto::FUNCTIONS) on photos with its settings; `job` names it for cancel_job.
#[tauri::command(async)]
fn auto_run(core: St, function: String, ids: Vec<i64>, settings: Value, job: String) -> Result<Value, String> {
    let job = auto::Job::start(&core, &job);
    auto::run(core.inner(), &function, &ids, &settings, &job)
}

/// The auto functions' default settings (for Reset).
#[tauri::command]
fn auto_defaults() -> auto::AutoSettings {
    Default::default()
}

#[tauri::command]
fn cancel_job(core: St, job: String) -> bool {
    auto::cancel(&core, &job)
}

#[tauri::command(async)]
fn storage_report(core: St) -> Result<Value, String> {
    storage::report(&core)
}

/// `thumbs` | `originals` | `immich_albums` | `cloud` | `missing` | `history` | `compact` | `all`
#[tauri::command(async)]
fn clean_up(core: St, what: String) -> Result<usize, String> {
    storage::clean(&core, &what)
}

#[tauri::command]
fn network_add(core: St, drive: storage::NetworkDrive) -> Result<Settings, String> {
    storage::add_drive(&core, drive)
}

#[tauri::command]
fn network_remove(core: St, id: String) -> Result<Settings, String> {
    storage::remove_drive(&core, &id)
}

/// Where a share usually appears once connected (to fill in the folder).
#[tauri::command]
fn network_mount_point(url: String) -> Option<String> {
    storage::mount_point(&url)
}

/// Connect the share if needed, without reading it into the catalog; returns its folder.
#[tauri::command(async)]
fn network_connect(core: St, id: String) -> Result<String, String> {
    storage::connect_drive(&core, &id)
}

#[tauri::command(async)]
fn network_status(core: St) -> Vec<Value> {
    storage::drives_online(&core)
}

/// Connect the share if needed and pick up new photos, like opening a folder.
#[tauri::command(async)]
fn network_open(core: St, id: String) -> Result<Value, String> {
    storage::open_drive(core.inner(), &id)
}

/// WebDAV / S3 account with its password or secret key.
#[tauri::command(async)]
fn cloud_connect(core: St, account: cloud::Account, secret: String) -> Result<Settings, String> {
    cloud::connect(&core, account, &secret)
}

#[tauri::command]
fn cloud_dropbox_start(app_key: String) -> Result<(), String> {
    cloud::dropbox_start(&app_key)
}

#[tauri::command(async)]
fn cloud_dropbox_finish(core: St, account: cloud::Account, code: String) -> Result<Settings, String> {
    cloud::dropbox_finish(&core, account, &code)
}

#[tauri::command(async)]
fn cloud_onedrive_connect(core: St, account: cloud::Account) -> Result<Settings, String> {
    cloud::onedrive_connect(&core, account)
}

#[tauri::command]
fn cloud_disconnect(core: St, id: String) -> Result<Settings, String> {
    cloud::disconnect(&core, &id)
}

#[tauri::command(async)]
fn cloud_browse(core: St, id: String, path: String) -> Result<Vec<cloud::Entry>, String> {
    cloud::open(&core, &id)?.1.list(&path)
}

/// Download what's new in a cloud folder and show it in the catalog.
#[tauri::command(async)]
fn cloud_sync(core: St, id: String, path: String) -> Result<Value, String> {
    cloud::sync(core.inner(), &id, &path)
}

/// Where each downloaded cloud folder is kept: [{id, path, local}].
#[tauri::command]
fn cloud_local_dirs(core: St) -> Vec<Value> {
    cloud::local_dirs(&core)
}

#[tauri::command]
fn cloud_forget_folder(core: St, id: String, path: String) -> Result<Settings, String> {
    cloud::forget_folder(&core, &id, &path)
}

/// Copy the originals at `from` to `to`; `job` names it for cancel_job.
#[tauri::command(async)]
fn duplicate(core: St, from: storage::Place, to: storage::Place, options: storage::DuplicateOptions, job: String) -> Result<Value, String> {
    let job = auto::Job::start(&core, &job);
    storage::duplicate(core.inner(), &from, &to, &options, &job)
}

pub fn run() {
    let core = Arc::new(Core::open(data_dir()).expect("could not open the OpenLight catalog"));
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(move |app| {
            let _ = core.app.set(app.handle().clone());
            app.asset_protocol_scope().allow_directory(core.dir.join("thumbs"), true)?;
            app.manage(core.clone());
            mcp::start_http(core.clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_info,
            query_photos,
            list_folders,
            import_folder,
            set_selection,
            update_photos,
            virtual_copy,
            remove_photos,
            get_develop,
            default_develop,
            render_preview,
            upright,
            auto_ca,
            heal_spot,
            auto_develop,
            auto_white_balance,
            auto_tone,
            remove_selection,
            save_develop,
            get_history,
            goto_history,
            undo_redo,
            export_photos,
            get_settings,
            save_settings,
            regenerate_token,
            set_provider_key,
            ai_run,
            ai_select,
            immich_connect,
            immich_albums,
            immich_open,
            google_connect,
            google_import,
            remote_disconnect,
            storage_report,
            clean_up,
            auto_run,
            auto_defaults,
            cancel_job,
            network_add,
            network_remove,
            network_mount_point,
            network_status,
            network_connect,
            network_open,
            cloud_connect,
            cloud_dropbox_start,
            cloud_dropbox_finish,
            cloud_onedrive_connect,
            cloud_disconnect,
            cloud_browse,
            cloud_sync,
            cloud_local_dirs,
            cloud_forget_folder,
            duplicate,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// `openlight-studio --import <folder>`: headless import into the catalog.
pub fn run_import(folder: &str) -> Result<usize, String> {
    let core = Core::open(data_dir())?;
    Ok(catalog::import(&core, Path::new(folder), &|done, total| eprint!("\rthumbnails {done}/{total}"))?.len())
}

/// `openlight-studio --mcp-stdio`: stdio MCP transport (spec §4.1).
pub fn run_mcp_stdio() {
    mcp::run_stdio();
}
