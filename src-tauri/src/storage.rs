//! Settings → Storage: what the catalog folder holds, a library analysis and clean-up.

use crate::{Core, catalog, develop::es, remote};
use rayon::prelude::*;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashSet},
    fs, io,
    path::{Path, PathBuf},
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

    let immich_dir = dir.join("remote").join("Immich");
    let mut immich_albums = Vec::new();
    let mut immich_albums_bytes = 0u64;
    let mut immich_albums_count = 0u64;
    if immich_dir.is_dir() {
        if let Ok(entries) = fs::read_dir(&immich_dir) {
            for entry in entries.flatten() {
                if entry.file_name() != "originals" && entry.path().is_dir() {
                    let (b, c) = du(&entry.path());
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
            if name != "Immich" && name != "Google Photos" {
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
                        count += du(&entry.path()).1 as usize;
                        let _ = fs::remove_dir_all(entry.path());
                    }
                }
            }
            count
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
        let _ = add(album_photo);

        let r = report(&core).unwrap();
        assert_eq!(r["immich_albums"][0]["name"], "Trip 2026");
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
