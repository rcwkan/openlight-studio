//! Settings → Storage: what the catalog folder holds, a library analysis and clean-up; network drives;
//! duplicating photos from one place to another.

use crate::{
    Core, Settings, auto, catalog, cloud,
    develop::{self, es},
    remote,
};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs, io,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::Arc,
    time::{Duration, Instant},
};

/// (bytes, files) under `path`; symlinks are not followed.
fn du(path: &Path) -> (u64, u64) {
    match fs::symlink_metadata(path) {
        Ok(m) if m.is_dir() => {
            fs::read_dir(path).into_iter().flatten().flatten().map(|e| du(&e.path())).fold((0, 0), |a, b| (a.0 + b.0, a.1 + b.1))
        }
        Ok(m) => (m.len(), 1),
        Err(_) => (0, 0),
    }
}

/// Photo files directly in `dir`; `.olxmp` sidecars and partial downloads are not photos.
fn photo_files(dir: &Path) -> u64 {
    fs::read_dir(dir).into_iter().flatten().flatten().filter(|e| develop::is_supported(&e.path())).count() as u64
}

/// Thumbnails whose photo is no longer in the catalog. The folder is listed before the ids
/// are read: rows are written before their thumbnails, so a running import is never caught.
fn orphan_thumbs(core: &Core) -> Result<Vec<PathBuf>, String> {
    let files: Vec<PathBuf> = fs::read_dir(core.dir.join("thumbs")).map_err(es)?.flatten().map(|e| e.path()).collect();
    let ids: HashSet<i64> = {
        let c = core.db();
        let mut st = c.prepare("SELECT id FROM photos").map_err(es)?;
        st.query_map([], |r| r.get(0)).map_err(es)?.collect::<Result<_, _>>().map_err(es)?
    };
    let id = |p: &Path| p.extension().filter(|e| *e == "jpg").and(p.file_stem()?.to_str()?.parse::<i64>().ok());
    Ok(files.into_iter().filter(|p| id(p).is_some_and(|id| !ids.contains(&id))).collect())
}

#[derive(Default)]
struct Folder {
    photos: u64,
    bytes: u64,
    online: bool,
    /// Masters whose file is gone while the folder is still there.
    missing: Vec<i64>,
}

/// Local folders (Immich photos live on the server) and their photo files. A folder that is
/// gone altogether may be on an unplugged drive: it is offline, and nothing in it is missing.
fn folders(core: &Core) -> Result<BTreeMap<String, Folder>, String> {
    let rows: Vec<(i64, String, String)> = {
        let c = core.db();
        let mut st = c
            .prepare("SELECT id, path, folder FROM photos WHERE master_id IS NULL AND path NOT LIKE 'immich://%'")
            .map_err(es)?;
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).map_err(es)?.collect::<Result<_, _>>().map_err(es)?
    };
    let sizes: Vec<Option<u64>> = rows.par_iter().map(|(_, path, _)| fs::metadata(path).ok().map(|m| m.len())).collect();
    let mut out: BTreeMap<String, Folder> = BTreeMap::new();
    for ((id, _, folder), size) in rows.into_iter().zip(sizes) {
        let f = out.entry(folder).or_insert_with_key(|k| Folder { online: Path::new(k).is_dir(), ..Default::default() });
        f.photos += 1;
        match size {
            Some(n) => f.bytes += n,
            None if f.online => f.missing.push(id),
            None => {}
        }
    }
    Ok(out)
}

pub fn report(core: &Core) -> Result<Value, String> {
    let dir = &core.dir;
    let orphans = orphan_thumbs(core)?;
    let originals = du(&remote::originals_dir(core));
    let folders = folders(core)?;
    let db_bytes = du(&dir.join("catalog.olcat")).0;
    let wal = du(&dir.join("catalog.olcat-wal")).0;
    let shm = du(&dir.join("catalog.olcat-shm")).0;
    let catalog = db_bytes + wal + shm;
    let (total, thumbs, google) = (du(dir).0, du(&dir.join("thumbs")).0, du(&dir.join("remote").join("Google Photos")).0);
    let clouds = du(&cloud::cloud_dir(core));

    let immich_dir = dir.join("remote").join("Immich");
    let mut immich_albums = Vec::new();
    let mut immich_albums_bytes = 0u64;
    let mut immich_albums_count = 0u64;
    if immich_dir.is_dir() {
        if let Ok(entries) = fs::read_dir(&immich_dir) {
            for entry in entries.flatten() {
                if entry.file_name() != "originals" && entry.path().is_dir() {
                    let (b, c) = (du(&entry.path()).0, photo_files(&entry.path()));
                    immich_albums_bytes += b;
                    immich_albums_count += c;
                    immich_albums.push(json!({
                        "name": entry.file_name().to_string_lossy(),
                        "path": entry.path().to_string_lossy(),
                        "bytes": b,
                        "photos": c,
                    }));
                }
            }
        }
    }

    let mut other_details = Vec::new();
    let mut other_bytes = 0u64;
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name == "thumbs" || name == "remote" || name.starts_with("catalog.olcat") {
                continue;
            }
            let (b, _) = du(&entry.path());
            other_bytes += b;
            other_details.push(json!({
                "name": name,
                "bytes": b,
            }));
        }
    }
    let remote_dir = dir.join("remote");
    if let Ok(entries) = fs::read_dir(&remote_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name != "Immich" && name != "Google Photos" && name != "Cloud" {
                let (b, _) = du(&entry.path());
                other_bytes += b;
                other_details.push(json!({
                    "name": format!("remote/{name}"),
                    "bytes": b,
                }));
            }
        }
    }

    let c = core.db();
    let one = |sql: &str| -> Result<i64, String> { c.query_row(sql, [], |r| r.get(0)).map_err(es) };
    let free = one("SELECT (SELECT freelist_count FROM pragma_freelist_count()) * (SELECT page_size FROM pragma_page_size())")?;
    let (steps, step_bytes) = catalog::past_steps(&c)?;
    let ai: (i64, i64) = c
        .query_row(
            "SELECT COUNT(*), COALESCE(SUM(n), 0) FROM
               (SELECT json_array_length(develop_json, '$.ai_provenance_log') n FROM photos) WHERE n > 0",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(es)?;
    let missing: usize = folders.values().map(|f| f.missing.len()).sum();
    let orphan_bytes: u64 = orphans.iter().map(|p| du(p).0).sum();
    let all_count = orphans.len() + originals.1 as usize + steps.max(0) as usize;
    let all_bytes = orphan_bytes + originals.0 + step_bytes.max(0) as u64 + free.max(0) as u64 + wal;

    Ok(json!({
        "data_dir": dir,
        "total": total,
        "catalog": catalog,
        "catalog_details": {
            "db": db_bytes,
            "wal": wal,
            "shm": shm,
            "free": free,
        },
        "thumbs": thumbs,
        "originals": originals.0,
        "immich_albums": immich_albums,
        "immich_albums_bytes": immich_albums_bytes,
        "google": google,
        "cloud": clouds.0,
        "other": other_bytes,
        "other_details": other_details,
        "summary": catalog::summary(&c)?,
        "immich_photos": one("SELECT COUNT(*) FROM photos WHERE master_id IS NULL AND path LIKE 'immich://%'")?,
        "history_steps": one("SELECT COUNT(*) FROM history")?,
        "ai": {"photos": ai.0, "edits": ai.1},
        "folders": folders.iter().map(|(k, f)| json!({"folder": k, "photos": f.photos, "bytes": f.bytes, "missing": f.missing.len(), "online": f.online})).collect::<Vec<_>>(),
        "clean": {
            "all": {"count": all_count, "bytes": all_bytes},
            "thumbs": {"count": orphans.len(), "bytes": orphan_bytes},
            "originals": {"count": originals.1, "bytes": originals.0},
            "immich_albums": {"count": immich_albums_count, "bytes": immich_albums_bytes},
            "cloud": {"count": clouds.1, "bytes": clouds.0},
            "missing": {"count": missing, "bytes": 0},
            "history": {"count": steps, "bytes": step_bytes},
            "compact": {"count": 0, "bytes": free as u64 + wal},
        },
    }))
}

/// One clean-up from the report; returns how many files, photos or history steps went.
pub fn clean(core: &Core, what: &str) -> Result<usize, String> {
    let n = match what {
        "thumbs" => orphan_thumbs(core)?.iter().filter(|p| fs::remove_file(p).is_ok()).count(),
        "originals" => {
            let dir = remote::originals_dir(core);
            let n = du(&dir).1 as usize;
            match fs::remove_dir_all(&dir) {
                Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(es(e)),
                _ => n,
            }
        }
        "immich_albums" => {
            let immich_dir = core.dir.join("remote").join("Immich");
            let mut count = 0;
            if immich_dir.is_dir() {
                for entry in fs::read_dir(&immich_dir).into_iter().flatten().flatten() {
                    if entry.file_name() != "originals" && entry.path().is_dir() {
                        let folder_str = entry.path().to_string_lossy().to_string();
                        let ids: Vec<i64> = {
                            let c = core.db();
                            let mut st = c.prepare("SELECT id FROM photos WHERE folder = ?").map_err(es)?;
                            st.query_map([&folder_str], |r| r.get(0)).map_err(es)?.collect::<Result<_, _>>().map_err(es)?
                        };
                        if !ids.is_empty() {
                            core.remove(&ids)?;
                        }
                        count += photo_files(&entry.path()) as usize;
                        let _ = fs::remove_dir_all(entry.path());
                    }
                }
            }
            count
        }
        "cloud" => {
            // downloaded cloud folders leave the catalog with their files; the clouds keep the originals
            let dir = cloud::cloud_dir(core);
            let ids: Vec<i64> = catalog::query(&core.db(), &catalog::Filter { folder: Some(dir.to_string_lossy().into_owned()), ..Default::default() })?.into_iter().map(|p| p.id).collect();
            core.remove(&ids)?;
            let n = du(&dir).1 as usize;
            match fs::remove_dir_all(&dir) {
                Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(es(e)),
                _ => {}
            }
            remote::update_settings(core, |s| s.clouds.iter_mut().for_each(|a| a.folders.clear()))?;
            n
        }
        "missing" => core.remove(&folders(core)?.into_values().flat_map(|f| f.missing).collect::<Vec<_>>())?,
        "history" => catalog::clear_history(&core.db())?,
        "compact" => 0,
        "all" => {
            let mut count = 0;
            if let Ok(orphans) = orphan_thumbs(core) {
                count += orphans.iter().filter(|p| fs::remove_file(p).is_ok()).count();
            }
            let dir = remote::originals_dir(core);
            count += du(&dir).1 as usize;
            let _ = fs::remove_dir_all(&dir);
            if let Ok(h) = catalog::clear_history(&core.db()) {
                count += h;
            }
            count
        }
        _ => return Err(format!("unknown clean-up '{what}'")),
    };
    if what != "thumbs" && what != "originals" {
        catalog::compact(&core.db())?; // deleted rows only free disk space once the file is rewritten
    }
    Ok(n)
}

// ---------------------------------------------------------------------------
// Network drives
// ---------------------------------------------------------------------------

/// A share on a NAS or server, browsed in place like a folder.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct NetworkDrive {
    /// `net_…`
    pub id: String,
    pub name: String,
    /// smb://, afp://, nfs:// or https:// (WebDAV) address: how the OS connects the share. May be empty
    /// when the share is connected some other way.
    pub url: String,
    /// Where its photos appear as files once connected, e.g. /Volumes/photos or \\nas\photos.
    pub path: String,
}

const SHARE_SCHEMES: &[&str] = &["smb", "afp", "nfs", "http", "https", "dav", "davs"];

/// Where a share usually appears once connected: macOS /Volumes/<share>, Linux's GVfs mount, a Windows
/// UNC path; the address's subfolders follow. None when there is no usual place.
pub fn mount_point(url: &str) -> Option<String> {
    mount_point_on(url, std::env::consts::OS, std::env::var("XDG_RUNTIME_DIR").ok().as_deref())
}

fn mount_point_on(url: &str, os: &str, runtime: Option<&str>) -> Option<String> {
    let u = tauri::Url::parse(url.trim()).ok()?;
    let host = u.host_str()?.to_string();
    let segs: Vec<String> = u.path_segments()?.filter(|s| !s.is_empty()).map(cloud::unpct).collect();
    let (share, rest) = segs.split_first()?;
    match (os, u.scheme()) {
        ("macos", "smb" | "afp") => Some(format!("/Volumes/{share}{}", rest.iter().map(|s| format!("/{s}")).collect::<String>())),
        ("linux", "smb") => {
            let user = if u.username().is_empty() { String::new() } else { format!(",user={}", u.username()) };
            let tail: String = rest.iter().map(|s| format!("/{s}")).collect();
            Some(format!("{}/gvfs/smb-share:server={},share={}{user}{tail}", runtime?, host.to_lowercase(), share.to_lowercase()))
        }
        ("windows", "smb") => Some(format!(r"\\{host}\{}", segs.join(r"\"))),
        _ => None,
    }
}

/// Run a helper for at most `secs` seconds; true when it succeeded.
fn quietly(prog: &str, args: &[&str], secs: u64) -> bool {
    let Ok(mut child) = Command::new(prog).args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn() else { return false };
    let deadline = Instant::now() + Duration::from_secs(secs);
    loop {
        match child.try_wait() {
            Ok(Some(s)) => return s.success(),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(100)),
            _ => {
                let _ = child.kill();
                return false;
            }
        }
    }
}

/// Ask the OS to connect the share at `url` (it asks for a password itself when the keychain has none)
/// and wait up to a minute for `path` to appear.
fn connect_share(url: &str, path: &Path) -> Result<(), String> {
    let u = tauri::Url::parse(url.trim()).map_err(|e| format!("invalid share address ({e}): {url}"))?;
    if !SHARE_SCHEMES.contains(&u.scheme()) {
        return Err(format!("{}:// shares can't be connected from here; use smb://, afp://, nfs:// or https://", u.scheme()));
    }
    if cfg!(target_os = "macos") {
        quietly("open", &["--", u.as_str()], 10);
    } else if cfg!(windows) {
        let unc = mount_point_on(u.as_str(), "windows", None).ok_or("on Windows, connect smb:// shares, or the folder's UNC path directly")?;
        quietly("explorer.exe", &[&unc], 10);
    } else if !quietly("gio", &["mount", u.as_str()], 30) {
        // no password in the keyring: the file manager asks for it
        quietly("xdg-open", &[u.as_str()], 10);
    }
    let deadline = Instant::now() + Duration::from_secs(60);
    while Instant::now() < deadline {
        if path.is_dir() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    Err(format!("{} did not appear after connecting {url}; connect it in your file manager, or check the folder", path.display()))
}

fn drive(core: &Core, id: &str) -> Result<NetworkDrive, String> {
    core.settings.lock().unwrap().network_drives.iter().find(|d| d.id == id).cloned().ok_or_else(|| "that network drive is not in the list".to_string())
}

/// Add a network drive; with only an address, the folder is where the share usually appears.
pub fn add_drive(core: &Core, mut d: NetworkDrive) -> Result<Settings, String> {
    d.url = d.url.trim().to_string();
    if !d.url.is_empty() {
        let u = tauri::Url::parse(&d.url).map_err(|e| format!("invalid share address ({e})"))?;
        if !SHARE_SCHEMES.contains(&u.scheme()) {
            return Err("the address must start with smb://, afp://, nfs:// or https://".into());
        }
    }
    d.path = match d.path.trim() {
        "" => mount_point(&d.url).ok_or("choose the folder where the share appears once connected")?,
        p => p.to_string(),
    };
    if d.name.trim().is_empty() {
        d.name = Path::new(&d.path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "Network drive".into());
    }
    d.id = format!("net_{}", &crate::new_token()[..10]);
    remote::update_settings(core, |s| s.network_drives.push(d))
}

/// Forget a network drive; its photos stay in the catalog.
pub fn remove_drive(core: &Core, id: &str) -> Result<Settings, String> {
    remote::update_settings(core, |s| s.network_drives.retain(|d| d.id != id))
}

/// Which network drives are connected now (their folder is there).
pub fn drives_online(core: &Core) -> Vec<Value> {
    let drives = core.settings.lock().unwrap().network_drives.clone();
    drives.par_iter().map(|d| json!({ "id": d.id, "online": Path::new(&d.path).is_dir() })).collect()
}

/// Open a network drive like a folder: connect it when its folder is not there, then pick up new photos.
pub fn open_drive(core: &Arc<Core>, id: &str) -> Result<Value, String> {
    let d = drive(core, id)?;
    let path = PathBuf::from(&d.path);
    if !path.is_dir() {
        if d.url.is_empty() {
            return Err(format!("{} is not connected: connect the share, or add its address so OpenLight can connect it", d.path));
        }
        connect_share(&d.url, &path)?;
    }
    let ids = catalog::import(core, &path, &|done, total| core.emit("import-progress", json!({ "label": format!("Reading {}", d.name), "done": done, "total": total })))?;
    core.emit("catalog-changed", json!({}));
    let n = ids.len();
    auto::after_import(core, ids, "folders");
    Ok(json!({ "path": d.path, "new": n }))
}

// ---------------------------------------------------------------------------
// Duplicate: copy originals from one place to another
// ---------------------------------------------------------------------------

/// Where photos are duplicated from or to.
#[derive(Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Place {
    /// A folder on this computer or a network drive (with its subfolders).
    Folder { path: String },
    /// Catalog photos (a source only): their files.
    Photos { ids: Vec<i64> },
    /// Immich: an album (none: the favorites) as a source, the library as a destination.
    Immich {
        #[serde(default)]
        album: Option<String>,
    },
    /// Google Photos: the picked photos as a source, the library as a destination.
    Google,
    /// A folder of a cloud storage account.
    Cloud {
        account: String,
        #[serde(default)]
        path: String,
    },
}

#[derive(Deserialize, Clone, Debug)]
#[serde(default)]
pub struct DuplicateOptions {
    /// Include subfolders of a folder.
    pub subfolders: bool,
    /// Only picks, and photos rated at least `min_rating` (they need to be in the catalog, or Immich).
    pub only_picks: bool,
    pub min_rating: i64,
    /// Skip files the destination has already (same name and size, and date when dates are kept).
    pub skip_existing: bool,
    /// Edits travel along as `.olxmp` sidecars (to folders and cloud storage).
    pub sidecars: bool,
    /// Read each copy back and compare checksums (folders).
    pub verify: bool,
    /// Copies keep the original's modified date (folders).
    pub keep_dates: bool,
    /// Copies in a folder join the catalog with the originals' ratings, flags, labels and keywords.
    pub catalog: bool,
    /// Then Auto Catalog & Culling runs on them.
    pub cull: bool,
}
impl Default for DuplicateOptions {
    fn default() -> Self {
        Self { subfolders: true, only_picks: false, min_rating: 0, skip_existing: true, sidecars: true, verify: false, keep_dates: true, catalog: true, cull: false }
    }
}

enum Source {
    File(PathBuf),
    Immich(String),
    Cloud(String),
}

/// One file to duplicate.
struct Item {
    /// Path under the destination, "/"-separated (folders keep their subfolders).
    rel: String,
    size: Option<u64>,
    src: Source,
    /// Its catalog photo: ratings, edits and the capture time travel with it.
    photo: Option<catalog::Photo>,
    /// Immich's favorite and stars for photos not browsed here yet.
    pick: bool,
    rating: i64,
}

/// Supported files under `dir` (hidden ones skipped), subfolders when `deep`.
fn walk(dir: &Path, deep: bool, out: &mut Vec<PathBuf>) {
    for e in fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if e.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        if p.is_dir() {
            if deep {
                walk(&p, deep, out);
            }
        } else if develop::is_supported(&p) {
            out.push(p);
        }
    }
}

fn rel_of(p: &Path, root: &Path) -> String {
    p.strip_prefix(root).unwrap_or(p).components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/")
}

fn items(core: &Core, from: &Place, o: &DuplicateOptions, cloud_from: Option<&cloud::Client>) -> Result<Vec<Item>, String> {
    let item = |rel: String, size: Option<u64>, src: Source, photo: Option<catalog::Photo>| {
        let (pick, rating) = photo.as_ref().map_or((false, 0), |p| (p.flag == 1, p.rating));
        Item { rel, size, src, photo, pick, rating }
    };
    let folder = |root: &Path| -> Result<Vec<Item>, String> {
        if !root.is_dir() {
            return Err(format!("{} is not there (is the drive connected?)", root.display()));
        }
        let mut files = vec![];
        walk(root, o.subfolders, &mut files);
        let known: HashMap<String, catalog::Photo> = catalog::query(&core.db(), &catalog::Filter { folder: Some(root.to_string_lossy().into_owned()), ..Default::default() })?
            .into_iter()
            .filter(|p| p.master_id.is_none())
            .map(|p| (p.path.clone(), p))
            .collect();
        Ok(files
            .into_iter()
            .map(|f| {
                let photo = known.get(&*f.to_string_lossy()).cloned();
                item(rel_of(&f, root), fs::metadata(&f).ok().map(|m| m.len()), Source::File(f), photo)
            })
            .collect())
    };
    Ok(match from {
        Place::Folder { path } => folder(Path::new(path))?,
        Place::Google => folder(&core.dir.join("remote").join("Google Photos"))?,
        Place::Photos { ids } => {
            let photos = catalog::photos(&core.db(), ids)?;
            let masters: HashSet<i64> = photos.iter().map(|p| p.master_id.unwrap_or(p.id)).collect();
            catalog::photos(&core.db(), &masters.into_iter().collect::<Vec<_>>())?
                .into_iter()
                .map(|p| {
                    let src = match remote::immich_id(&p.path) {
                        Some(a) => Source::Immich(a.to_string()),
                        None => Source::File(PathBuf::from(&p.path)),
                    };
                    let size = fs::metadata(&p.path).ok().map(|m| m.len());
                    item(remote::safe_name(&p.filename), size, src, Some(p))
                })
                .collect()
        }
        Place::Immich { album } => {
            let known: HashMap<String, catalog::Photo> = {
                let db = core.db();
                let mut st = db.prepare("SELECT id FROM photos WHERE path LIKE 'immich://%' AND master_id IS NULL").map_err(es)?;
                let ids: Vec<i64> = st.query_map([], |r| r.get(0)).map_err(es)?.collect::<Result<_, _>>().map_err(es)?;
                drop(st);
                catalog::photos(&db, &ids)?.into_iter().map(|p| (p.path.clone(), p)).collect()
            };
            remote::immich_assets(core, album.as_deref())?
                .iter()
                .filter_map(|a| {
                    let (id, name) = (a["id"].as_str()?, a["originalFileName"].as_str()?);
                    let photo = known.get(&format!("immich://{id}")).cloned();
                    let mut it = item(remote::safe_name(name), a["exifInfo"]["fileSizeInByte"].as_u64(), Source::Immich(id.into()), photo);
                    if it.photo.is_none() {
                        (it.pick, it.rating) = (a["isFavorite"] == true, a["exifInfo"]["rating"].as_i64().unwrap_or(0));
                    }
                    Some(it)
                })
                .collect()
        }
        Place::Cloud { path, .. } => {
            let client = cloud_from.ok_or("cloud storage is not connected")?;
            let path = cloud::clean(path);
            client
                .files(&path, if o.subfolders { 8 } else { 0 })?
                .into_iter()
                .filter(|e| develop::is_supported(Path::new(&e.name)))
                .map(|e| item(cloud::clean(e.path.strip_prefix(&path).unwrap_or(&e.path)), Some(e.size), Source::Cloud(e.path.clone()), None))
                .collect()
        }
    })
}

/// Where the copies go, ready to receive files.
enum Target {
    Folder(PathBuf),
    Immich { url: String, key: String },
    Google { bearer: String },
    Cloud { client: cloud::Client, path: String, have: HashMap<String, u64> },
}

/// `name` in `dir` if free, else `stem-1.ext`, …
fn free_path(dir: &Path, name: &str) -> PathBuf {
    let taken: HashSet<String> = fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.file_name().to_string_lossy().to_lowercase()).collect();
    dir.join(cloud::free_name(name, &taken))
}

fn sha256_of(path: &Path) -> Result<Vec<u8>, String> {
    let mut f = fs::File::open(path).map_err(es)?;
    let mut ctx = ring::digest::Context::new(&ring::digest::SHA256);
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let k = io::Read::read(&mut f, &mut buf).map_err(es)?;
        if k == 0 {
            return Ok(ctx.finish().as_ref().to_vec());
        }
        ctx.update(&buf[..k]);
    }
}

enum Copied {
    /// Bytes copied, and the copy when it is a local file.
    Done(u64, Option<PathBuf>),
    Skipped,
}

fn modified(p: &Path) -> Option<std::time::SystemTime> {
    fs::metadata(p).and_then(|m| m.modified()).ok()
}

/// Copy one file to the target; `tmp` holds downloads on the way.
fn copy_one(core: &Core, it: &Item, to: &mut Target, o: &DuplicateOptions, tmp: &Path, cloud_from: Option<&cloud::Client>) -> Result<Copied, String> {
    // already there? the quick check of rsync: same size and, when dates are kept, the same date
    let local_target = match &*to {
        Target::Folder(root) => {
            let t = cloud::clean(&it.rel).split('/').fold(root.clone(), |d, s| d.join(remote::safe_name(s)));
            if let Ok(m) = fs::metadata(&t) {
                let same_date = match (&it.src, o.keep_dates) {
                    (Source::File(f), true) => modified(f).zip(m.modified().ok()).is_some_and(|(a, b)| a.max(b).duration_since(a.min(b)).unwrap_or_default() < Duration::from_secs(2)),
                    _ => true,
                };
                if o.skip_existing && it.size.is_none_or(|s| s == m.len()) && same_date {
                    return Ok(Copied::Skipped);
                }
            }
            Some(t)
        }
        Target::Cloud { have, .. } if o.skip_existing && it.size.is_some_and(|s| have.get(&it.rel.to_lowercase()) == Some(&s)) => return Ok(Copied::Skipped),
        _ => None,
    };
    let fetched;
    let src: &Path = match &it.src {
        Source::File(p) => p,
        Source::Immich(id) => {
            fetched = remote::immich_original(core, id)?;
            &fetched
        }
        Source::Cloud(path) => {
            fetched = tmp.join(remote::safe_name(path.rsplit('/').next().unwrap_or("photo")));
            cloud_from.ok_or("cloud storage is not connected")?.download(path, &fetched)?;
            &fetched
        }
    };
    let bytes = fs::metadata(src).map_err(es)?.len();
    // the edits as a sidecar: the original's own, else the catalog's
    let sidecar = || -> Option<String> {
        fs::read_to_string(catalog::sidecar_path(&src.to_string_lossy())).ok().or_else(|| {
            let p = it.photo.as_ref()?;
            let d = catalog::get_develop(&core.db(), p.id).ok()?;
            (d != develop::Develop::default()).then(|| catalog::sidecar_json(p, &d))
        })
    };
    // capture time (UTC) for uploads: the catalog's, else the file's date
    let taken = || -> String {
        let db = core.db();
        it.photo.as_ref().and_then(|p| db.query_row(remote::UPLOAD_ROW, [p.id], |r| r.get::<_, String>(1)).ok()).unwrap_or_else(|| {
            let secs = modified(src).and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs() as i64);
            db.query_row("SELECT strftime('%Y-%m-%dT%H:%M:%SZ', ?, 'unixepoch')", [secs], |r| r.get(0)).unwrap_or_default()
        })
    };
    match to {
        Target::Folder(_) => {
            let mut t = local_target.ok_or("no target")?;
            if t.exists() {
                t = free_path(t.parent().ok_or("no folder")?, &t.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
            }
            fs::create_dir_all(t.parent().ok_or("no folder")?).map_err(es)?;
            fs::copy(src, &t).map_err(es)?;
            if o.keep_dates
                && let Some(m) = modified(src)
            {
                let _ = fs::File::options().write(true).open(&t).and_then(|f| f.set_modified(m));
            }
            if o.verify && sha256_of(src)? != sha256_of(&t)? {
                let _ = fs::remove_file(&t);
                return Err("the copy does not match the original".into());
            }
            if o.sidecars
                && let Some(doc) = sidecar()
            {
                fs::write(catalog::sidecar_path(&t.to_string_lossy()), doc).map_err(es)?;
            }
            Ok(Copied::Done(bytes, Some(t)))
        }
        Target::Immich { url, key } => {
            let v = remote::immich_post(&remote::agent(), url, key, src, &taken())?;
            Ok(if v["status"] == "duplicate" { Copied::Skipped } else { Copied::Done(bytes, None) })
        }
        Target::Google { bearer } => {
            remote::google_upload(&remote::agent(), bearer, &src.to_string_lossy())?;
            Ok(Copied::Done(bytes, None))
        }
        Target::Cloud { client, path, have } => {
            // never replace a different file of that name, including one copied earlier in this run
            let (dir, name) = it.rel.rsplit_once('/').map_or(("", it.rel.as_str()), |(d, n)| (d, n));
            let taken: HashSet<String> = have
                .keys()
                .filter_map(|k| match k.rsplit_once('/') {
                    Some((d, n)) if d == dir.to_lowercase() => Some(n.to_string()),
                    None if dir.is_empty() => Some(k.clone()),
                    _ => None,
                })
                .collect();
            let name = cloud::free_name(name, &taken);
            let rel = cloud::join(dir, &name);
            client.upload(&cloud::join(path, &rel), src)?;
            have.insert(rel.to_lowercase(), bytes);
            if o.sidecars
                && let Some(doc) = sidecar()
            {
                let side = tmp.join(format!("{}.olxmp", remote::safe_name(&name)));
                fs::write(&side, doc).map_err(es)?;
                client.upload(&format!("{}.olxmp", cloud::join(path, &rel)), &side)?;
            }
            Ok(Copied::Done(bytes, None))
        }
    }
}

/// Duplicate the originals at `from` to `to`: new files only (unless told otherwise), subfolders kept,
/// edits along as sidecars; copies in a folder join the catalog with their ratings and keywords, and can
/// be culled automatically. Never deletes or replaces anything.
pub fn duplicate(core: &Arc<Core>, from: &Place, to: &Place, o: &DuplicateOptions, job: &auto::Job) -> Result<Value, String> {
    if from == to {
        return Err("choose a different destination".into());
    }
    if let (Place::Folder { path: a }, Place::Folder { path: b }) = (from, to)
        && Path::new(b).starts_with(a)
        && o.subfolders
    {
        return Err("the destination is inside the source folder".into());
    }
    let cloud_from = match from {
        Place::Cloud { account, .. } => Some(cloud::open(core, account)?.1),
        _ => None,
    };
    job.progress("Listing", 0, 0);
    let all = items(core, from, o, cloud_from.as_ref())?;
    let listed = all.len();
    let todo: Vec<Item> = all.into_iter().filter(|it| (!o.only_picks || it.pick) && it.rating >= o.min_rating).collect();
    let mut target = match to {
        Place::Folder { path } => {
            fs::create_dir_all(path).map_err(|e| format!("cannot write to {path}: {e}"))?;
            Target::Folder(PathBuf::from(path))
        }
        Place::Immich { .. } => {
            let (url, key) = remote::immich(core)?;
            Target::Immich { url, key }
        }
        Place::Google => Target::Google { bearer: format!("Bearer {}", remote::google_access_token(core)?) },
        Place::Cloud { account, path } => {
            let client = cloud::open(core, account)?.1;
            let path = cloud::clean(path);
            let base = if path.is_empty() { String::new() } else { format!("{path}/") };
            // a destination folder that does not exist yet is empty
            let have = client.files(&path, 8).unwrap_or_default().into_iter().map(|e| (e.path.strip_prefix(&base).unwrap_or(&e.path).to_lowercase(), e.size)).collect();
            Target::Cloud { client, path, have }
        }
        Place::Photos { .. } => return Err("catalog photos can't be a destination: choose a folder or a service".into()),
    };
    let tmp = std::env::temp_dir().join(format!("openlight-duplicate-{}", crate::new_token()));
    fs::create_dir_all(&tmp).map_err(es)?;
    let (mut copied, mut skipped, mut bytes, mut errors, mut written) = (0, 0, 0u64, vec![], vec![]);
    for (k, it) in todo.iter().enumerate() {
        if job.cancelled() {
            break;
        }
        job.progress("Copying", k, todo.len());
        match copy_one(core, it, &mut target, o, &tmp, cloud_from.as_ref()) {
            Ok(Copied::Done(n, local)) => {
                copied += 1;
                bytes += n;
                if let Some(t) = local {
                    written.push((t, k));
                }
            }
            Ok(Copied::Skipped) => skipped += 1,
            Err(e) => errors.push(format!("{}: {e}", it.rel)),
        }
        // downloads on the way are not kept
        for e in fs::read_dir(&tmp).into_iter().flatten().flatten() {
            let _ = fs::remove_file(e.path());
        }
    }
    let _ = fs::remove_dir_all(&tmp);
    job.progress("Copying", todo.len(), todo.len());
    // copies in a folder join the catalog, with the originals' culling (edits come with the sidecars)
    let mut new_ids = vec![];
    if o.catalog && !written.is_empty() && !job.cancelled() {
        let ids = catalog::import_files(core, written.iter().map(|w| w.0.clone()).collect(), &|done, total| job.progress("Adding to the catalog", done, total))?;
        let added: HashMap<String, i64> = catalog::photos(&core.db(), &ids)?.into_iter().map(|p| (p.path, p.id)).collect();
        for (path, k) in &written {
            let (Some(&id), Some(p)) = (added.get(&*path.to_string_lossy()), todo[*k].photo.as_ref()) else { continue };
            let u = catalog::Culling {
                rating: Some(p.rating),
                flag: Some(catalog::flag_name(p.flag).into()),
                color_label: Some(if p.color_label.is_empty() { "none".into() } else { p.color_label.clone() }),
                keywords: Some(p.keywords.clone()),
            };
            catalog::update(&core.db(), &[id], &u)?;
            if !o.sidecars
                && let Some(d) = core.develop(p.id).ok().filter(|d| *d != develop::Develop::default())
            {
                catalog::save_develop(&core.db(), id, &d, "Duplicate")?;
                core.refresh(id, &d)?;
            }
        }
        new_ids = ids;
        core.emit("catalog-changed", json!({}));
    }
    let culled = if o.cull && !new_ids.is_empty() && !job.cancelled() {
        let (s, provider) = {
            let s = core.settings.lock().unwrap();
            (s.auto.cull.clone(), s.active_provider.clone())
        };
        let r = auto::cull(core, &new_ids, &s, &provider, job)?;
        json!({ "picks": r["picks"], "rejects": r["rejects"], "rated": r["rated"] })
    } else {
        Value::Null
    };
    Ok(json!({
        "listed": listed, "selected": todo.len(), "copied": copied, "skipped": skipped, "failed": errors.len(), "bytes": bytes,
        "errors": errors.into_iter().take(5).collect::<Vec<_>>(), "cataloged": new_ids.len(), "culled": culled, "cancelled": job.cancelled(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A file deleted from a folder that is still there is missing; a folder that is gone
    /// (an unplugged drive) is offline and left alone. Each clean-up removes what was reported.
    #[test]
    fn report_then_clean_up() {
        let dir = std::env::temp_dir().join(format!("ols-storage-test-{}", crate::new_token()));
        let core = Core::open(dir.join("data")).unwrap();
        let photos = dir.join("photos");
        fs::create_dir_all(&photos).unwrap();
        fs::write(photos.join("a.jpg"), [0u8; 10]).unwrap();
        let add = |p: PathBuf| {
            let folder = p.parent().unwrap().to_string_lossy().to_string();
            catalog::add_photo(&core.db(), &p.to_string_lossy(), &folder, "x.jpg", &Default::default(), &Default::default()).unwrap().unwrap()
        };
        let (kept, missing, offline) = (add(photos.join("a.jpg")), add(photos.join("b.jpg")), add(dir.join("unplugged").join("c.jpg")));
        let mut d = core.develop(kept).unwrap();
        for ev in [1.0, 2.0] {
            d.basic_tone.exposure = ev;
            catalog::save_develop(&core.db(), kept, &d, "Exposure").unwrap();
        }
        fs::write(core.thumb_path(kept), [0u8; 5]).unwrap();
        fs::write(core.thumb_path(999), [0u8; 5]).unwrap();
        fs::write(dir.join("data/thumbs/.DS_Store"), [0u8; 5]).unwrap();
        fs::create_dir_all(remote::originals_dir(&core)).unwrap();
        fs::write(remote::originals_dir(&core).join("IMG_1_u1.jpg"), [0u8; 7]).unwrap();

        let r = report(&core).unwrap();
        assert_eq!(r["clean"]["thumbs"], json!({"count": 1, "bytes": 5}));
        assert_eq!(r["clean"]["originals"], json!({"count": 1, "bytes": 7}));
        assert_eq!(r["clean"]["missing"]["count"], 1);
        assert_eq!(r["clean"]["history"]["count"], 2, "Import and the first exposure step; the current one stays");
        assert_eq!(r["folders"][0], json!({"folder": photos, "photos": 2, "bytes": 10, "missing": 1, "online": true}));
        assert_eq!(r["folders"][1]["online"], false);

        assert_eq!(clean(&core, "thumbs").unwrap(), 1);
        assert!(core.thumb_path(kept).exists() && dir.join("data/thumbs/.DS_Store").exists());
        assert_eq!(clean(&core, "originals").unwrap(), 1);
        assert_eq!(clean(&core, "missing").unwrap(), 1);
        assert!(core.photo(missing).is_err() && core.photo(offline).is_ok());
        assert_eq!(clean(&core, "history").unwrap(), 2);
        assert_eq!(core.develop(kept).unwrap(), d);
        assert_eq!(clean(&core, "compact").unwrap(), 0);
        let r = report(&core).unwrap();
        for k in ["thumbs", "originals", "missing", "history"] {
            assert_eq!(r["clean"][k]["count"], 0, "{k}");
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn network_drive_mount_points() {
        assert_eq!(mount_point_on("smb://nas.local/Photos/2024", "macos", None).as_deref(), Some("/Volumes/Photos/2024"));
        assert_eq!(
            mount_point_on("smb://alice@NAS/Photo%20Share/x", "linux", Some("/run/user/1000")).as_deref(),
            Some("/run/user/1000/gvfs/smb-share:server=nas,share=photo share,user=alice/x")
        );
        assert_eq!(mount_point_on("smb://nas/photos/2024", "windows", None).as_deref(), Some(r"\\nas\photos\2024"));
        assert_eq!(mount_point_on("https://cloud.example/dav", "macos", None), None);
        assert_eq!(mount_point_on("smb://nas", "macos", None), None, "no share named");
        assert_eq!(mount_point_on("smb://nas/photos", "linux", None), None, "no GVfs runtime folder");
    }

    #[test]
    fn duplicate_a_folder_with_its_culling_and_edits() {
        let dir = std::env::temp_dir().join(format!("ols-duplicate-test-{}", crate::new_token()));
        let core = Arc::new(Core::open(dir.join("data")).unwrap());
        let (src, dst) = (dir.join("card"), dir.join("nas"));
        fs::create_dir_all(src.join("day 2")).unwrap();
        let jpeg = |p: PathBuf, shade: u8| image::RgbImage::from_pixel(32, 24, image::Rgb([shade, 90, 40])).save(p).unwrap();
        jpeg(src.join("a.jpg"), 200);
        jpeg(src.join("day 2").join("b.jpg"), 100);
        let ids = catalog::import(&core, &src, &|_, _| {}).unwrap();
        let a = catalog::photos(&core.db(), &ids).unwrap().into_iter().find(|p| p.filename == "a.jpg").unwrap();
        catalog::update(&core.db(), &[a.id], &catalog::Culling { rating: Some(4), flag: Some("pick".into()), keywords: Some("beach".into()), ..Default::default() }).unwrap();
        let mut d = core.develop(a.id).unwrap();
        d.basic_tone.exposure = 1.0;
        core.commit(a.id, &d, "Exposure").unwrap();

        let (from, to) = (Place::Folder { path: src.to_string_lossy().into() }, Place::Folder { path: dst.to_string_lossy().into() });
        let job = auto::Job::start(&core, "dup");
        let r = duplicate(&core, &from, &to, &DuplicateOptions { verify: true, ..Default::default() }, &job).unwrap();
        assert_eq!((r["copied"].as_u64(), r["cataloged"].as_u64(), r["failed"].as_u64()), (Some(2), Some(2), Some(0)), "{r}");
        assert!(dst.join("day 2").join("b.jpg").is_file() && dst.join("a.jpg.olxmp").is_file());
        let copy = catalog::query(&core.db(), &catalog::Filter { folder: Some(dst.to_string_lossy().into()), ..Default::default() }).unwrap();
        let ca = copy.iter().find(|p| p.filename == "a.jpg").unwrap();
        assert_eq!((ca.rating, ca.flag, ca.keywords.as_str()), (4, 1, "beach"), "the culling travels");
        assert_eq!(core.develop(ca.id).unwrap().basic_tone.exposure, 1.0, "the edits travel in the sidecar");
        assert_eq!(modified(&src.join("a.jpg")), modified(&dst.join("a.jpg")), "dates are kept");

        // again: nothing new; picks only: one photo
        let r = duplicate(&core, &from, &to, &DuplicateOptions::default(), &job).unwrap();
        assert_eq!((r["copied"].as_u64(), r["skipped"].as_u64()), (Some(0), Some(2)), "{r}");
        let picks = Place::Folder { path: dir.join("picks").to_string_lossy().into() };
        let r = duplicate(&core, &from, &picks, &DuplicateOptions { only_picks: true, catalog: false, ..Default::default() }, &job).unwrap();
        assert_eq!((r["selected"].as_u64(), r["copied"].as_u64(), r["cataloged"].as_u64()), (Some(1), Some(1), Some(0)), "{r}");
        // a different photo with the same name is kept beside the copy, never over it
        image::RgbImage::from_fn(64, 48, |x, y| image::Rgb([(x * 4) as u8, (y * 5) as u8, 9])).save(src.join("a.jpg")).unwrap();
        let r = duplicate(&core, &from, &to, &DuplicateOptions { catalog: false, ..Default::default() }, &job).unwrap();
        assert_eq!(r["copied"].as_u64(), Some(1), "{r}");
        assert!(dst.join("a-1.jpg").is_file());
        let inside = Place::Folder { path: src.join("day 2").to_string_lossy().into() };
        assert!(duplicate(&core, &from, &inside, &DuplicateOptions::default(), &job).is_err());
        assert!(duplicate(&core, &from, &Place::Photos { ids: vec![] }, &DuplicateOptions::default(), &job).is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn clean_all_and_immich_albums() {
        let dir = std::env::temp_dir().join(format!("ols-storage-test-all-{}", crate::new_token()));
        let core = Core::open(dir.join("data")).unwrap();
        let photos = dir.join("photos");
        fs::create_dir_all(&photos).unwrap();
        fs::write(photos.join("a.jpg"), [0u8; 10]).unwrap();
        let add = |p: PathBuf| {
            let folder = p.parent().unwrap().to_string_lossy().to_string();
            catalog::add_photo(&core.db(), &p.to_string_lossy(), &folder, "x.jpg", &Default::default(), &Default::default()).unwrap().unwrap()
        };
        let kept = add(photos.join("a.jpg"));
        let mut d = core.develop(kept).unwrap();
        d.basic_tone.exposure = 1.0;
        catalog::save_develop(&core.db(), kept, &d, "Exposure").unwrap();
        fs::write(core.thumb_path(kept), [0u8; 5]).unwrap();
        fs::write(core.thumb_path(999), [0u8; 5]).unwrap();
        fs::create_dir_all(remote::originals_dir(&core)).unwrap();
        fs::write(remote::originals_dir(&core).join("cached.jpg"), [0u8; 7]).unwrap();

        // Create a mock Immich album
        let immich_album = dir.join("data").join("remote").join("Immich").join("Trip 2026");
        fs::create_dir_all(&immich_album).unwrap();
        let album_photo = immich_album.join("photo1.jpg");
        fs::write(&album_photo, [0u8; 20]).unwrap();
        fs::write(immich_album.join("photo1.jpg.olxmp"), b"{}").unwrap();
        let _ = add(album_photo);

        let r = report(&core).unwrap();
        assert_eq!(r["immich_albums"][0]["name"], "Trip 2026");
        assert_eq!(r["immich_albums"][0]["photos"], 1, "the sidecar is not a photo");
        assert_eq!(r["clean"]["all"]["count"].as_u64().unwrap(), 3); // 1 orphan thumb + 1 original + 1 history step

        // clean("all") cleans orphan thumbs, originals, history, and compacts
        let cleaned = clean(&core, "all").unwrap();
        assert_eq!(cleaned, 3);

        // clean("immich_albums") cleans the downloaded album and photos
        let cleaned_albums = clean(&core, "immich_albums").unwrap();
        assert_eq!(cleaned_albums, 1);
        assert!(!immich_album.exists());

        let _ = fs::remove_dir_all(&dir);
    }
}
