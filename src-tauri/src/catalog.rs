//! DAM catalog (spec §3.1): SQLite `.olcat`, folder import, EXIF, thumbnails,
//! history (undo/redo), virtual copies and `.olxmp` sidecars.

use crate::{
    Core,
    develop::{self, Develop, es},
    vision::Metrics,
};
use image::{DynamicImage, metadata::Orientation};
use rayon::prelude::*;
use rusqlite::{Connection, params, params_from_iter, types::Value as Sql};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    fs,
    path::{MAIN_SEPARATOR, Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};

const SCHEMA: &str = "
PRAGMA journal_mode=WAL;
PRAGMA foreign_keys=ON;
PRAGMA busy_timeout=5000;
CREATE TABLE IF NOT EXISTS photos (
  id INTEGER PRIMARY KEY,
  path TEXT NOT NULL,
  folder TEXT NOT NULL,
  filename TEXT NOT NULL,
  master_id INTEGER,
  copy_name TEXT,
  width INTEGER NOT NULL DEFAULT 0,
  height INTEGER NOT NULL DEFAULT 0,
  captured_at TEXT,
  camera TEXT,
  lens TEXT,
  iso INTEGER,
  aperture REAL,
  shutter TEXT,
  focal_length REAL,
  rating INTEGER NOT NULL DEFAULT 0,
  flag INTEGER NOT NULL DEFAULT 0,
  color_label TEXT NOT NULL DEFAULT '',
  keywords TEXT NOT NULL DEFAULT '',
  develop_json TEXT NOT NULL DEFAULT '{}',
  history_pos INTEGER,
  thumb_v INTEGER NOT NULL DEFAULT 0,
  imported_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  edited_at TEXT,
  deleted_at TEXT
);
CREATE UNIQUE INDEX IF NOT EXISTS photos_path ON photos(path) WHERE master_id IS NULL;
CREATE INDEX IF NOT EXISTS photos_folder ON photos(folder);
CREATE TABLE IF NOT EXISTS history (
  id INTEGER PRIMARY KEY,
  photo_id INTEGER NOT NULL REFERENCES photos(id) ON DELETE CASCADE,
  label TEXT NOT NULL,
  develop_json TEXT NOT NULL,
  created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS history_photo ON history(photo_id, id);
CREATE TABLE IF NOT EXISTS immich_albums (
  album TEXT NOT NULL,
  path TEXT NOT NULL,
  PRIMARY KEY (album, path)
);
CREATE TABLE IF NOT EXISTS analysis (
  path TEXT PRIMARY KEY,
  stamp TEXT NOT NULL,
  metrics TEXT NOT NULL
);
";

pub fn open(path: &Path) -> Result<Connection, String> {
    let c = Connection::open(path).map_err(es)?;
    c.execute_batch(SCHEMA).map_err(es)?;
    let _ = c.execute("ALTER TABLE photos ADD COLUMN deleted_at TEXT", []);
    let _ = c.execute("CREATE INDEX IF NOT EXISTS photos_deleted ON photos(deleted_at)", []);
    let ids: Vec<i64> = c
        .prepare("SELECT id FROM photos WHERE json_array_length(json_extract(develop_json, '$.masks')) > 0")
        .and_then(|mut st| st.query_map([], |r| r.get(0)).map(|rows| rows.flatten().collect()))
        .unwrap_or_default();
    for id in ids {
        let _ = get_develop(&c, id);
    }
    Ok(c)
}

#[derive(Serialize, Clone, Debug)]
pub struct Photo {
    pub id: i64,
    pub path: String,
    pub folder: String,
    pub filename: String,
    pub master_id: Option<i64>,
    pub copy_name: Option<String>,
    pub width: i64,
    pub height: i64,
    pub captured_at: Option<String>,
    pub camera: Option<String>,
    pub lens: Option<String>,
    pub iso: Option<i64>,
    pub aperture: Option<f64>,
    pub shutter: Option<String>,
    pub focal_length: Option<f64>,
    pub rating: i64,
    /// 1 pick, 0 unflagged, -1 reject
    pub flag: i64,
    pub color_label: String,
    pub keywords: String,
    pub edited: bool,
    pub thumb_v: i64,
    pub deleted_at: Option<String>,
}

const COLS: &str = "id, path, folder, filename, master_id, copy_name, width, height, captured_at, camera, lens, \
                    iso, aperture, shutter, focal_length, rating, flag, color_label, keywords, edited_at IS NOT NULL, thumb_v, deleted_at";

fn row(r: &rusqlite::Row) -> rusqlite::Result<Photo> {
    Ok(Photo {
        id: r.get(0)?,
        path: r.get(1)?,
        folder: r.get(2)?,
        filename: r.get(3)?,
        master_id: r.get(4)?,
        copy_name: r.get(5)?,
        width: r.get(6)?,
        height: r.get(7)?,
        captured_at: r.get(8)?,
        camera: r.get(9)?,
        lens: r.get(10)?,
        iso: r.get(11)?,
        aperture: r.get(12)?,
        shutter: r.get(13)?,
        focal_length: r.get(14)?,
        rating: r.get(15)?,
        flag: r.get(16)?,
        color_label: r.get(17)?,
        keywords: r.get(18)?,
        edited: r.get(19)?,
        thumb_v: r.get(20)?,
        deleted_at: r.get(21)?,
    })
}

#[derive(Deserialize, Default, Debug)]
#[serde(default)]
pub struct Filter {
    /// Folder and its subfolders.
    pub folder: Option<String>,
    /// Every word must match filename, camera, lens or keywords.
    pub text: Option<String>,
    /// Any word may match (keyword fallback for semantic queries).
    pub text_any: Option<String>,
    pub min_rating: Option<i64>,
    /// pick | unflagged | reject
    pub flag: Option<String>,
    pub color_label: Option<String>,
    pub edited: Option<bool>,
    /// original | copy | edited | unedited_original | edited_original
    pub copy_type: Option<String>,
    /// landscape | portrait | square
    pub aspect: Option<String>,
    pub ids: Option<Vec<i64>>,
    /// Pages of an Immich album loaded so far (album id, or "favorites").
    pub immich_album: Option<String>,
    pub limit: Option<i64>,
    /// Query items in the bin (true) or normal items (false / None).
    pub in_bin: Option<bool>,
}

const TEXT_COLS: &str = "(filename LIKE ? OR camera LIKE ? OR lens LIKE ? OR keywords LIKE ?)";

/// pick | unflagged | reject for a stored flag.
pub fn flag_name(f: i64) -> &'static str {
    ["reject", "unflagged", "pick"][(f + 1).clamp(0, 2) as usize]
}

pub fn flag_value(s: &str) -> Result<i64, String> {
    match s {
        "pick" => Ok(1),
        "unflagged" => Ok(0),
        "reject" => Ok(-1),
        _ => Err(format!("flag must be pick, unflagged or reject (got '{s}')")),
    }
}

pub fn query(c: &Connection, f: &Filter) -> Result<Vec<Photo>, String> {
    let mut sql = format!("SELECT {COLS} FROM photos WHERE 1=1");
    let mut args: Vec<Sql> = vec![];
    if let Some(folder) = &f.folder {
        let prefix = format!("{folder}{MAIN_SEPARATOR}");
        sql += " AND (folder = ? OR substr(folder, 1, ?) = ?)";
        args.extend([folder.clone().into(), (prefix.chars().count() as i64).into(), prefix.into()]);
    }
    let words = |s: &Option<String>| -> Vec<String> {
        s.as_deref().unwrap_or("").split_whitespace().map(|w| format!("%{w}%")).collect()
    };
    for w in words(&f.text) {
        sql += &format!(" AND {TEXT_COLS}");
        args.extend(std::iter::repeat_n(Sql::from(w), 4));
    }
    let any = words(&f.text_any);
    if !any.is_empty() {
        sql += &format!(" AND ({})", vec![TEXT_COLS; any.len()].join(" OR "));
        for w in any {
            args.extend(std::iter::repeat_n(Sql::from(w), 4));
        }
    }
    if let Some(r) = f.min_rating {
        sql += " AND rating >= ?";
        args.push(r.into());
    }
    if let Some(flag) = f.flag.as_deref().filter(|s| !s.is_empty() && *s != "any") {
        sql += " AND flag = ?";
        args.push(flag_value(flag)?.into());
    }
    if let Some(label) = f.color_label.as_deref().filter(|s| !s.is_empty() && *s != "any") {
        sql += " AND color_label = ?";
        args.push(if label == "none" { String::new() } else { label.to_string() }.into());
    }
    if let Some(e) = f.edited {
        sql += " AND (edited_at IS NOT NULL) = ?";
        args.push(e.into());
    }
    if let Some(ct) = f.copy_type.as_deref().filter(|s| !s.is_empty() && *s != "all") {
        match ct {
            "original" => sql += " AND master_id IS NULL",
            "copy" | "saved_copy" | "saved_new" => sql += " AND master_id IS NOT NULL",
            "edited" => sql += " AND (edited_at IS NOT NULL)",
            "unedited_original" => sql += " AND master_id IS NULL AND edited_at IS NULL",
            "edited_original" => sql += " AND master_id IS NULL AND edited_at IS NOT NULL",
            _ => {}
        }
    }
    if let Some(aspect) = f.aspect.as_deref().filter(|s| !s.is_empty() && *s != "all") {
        match aspect {
            "landscape" => sql += " AND width > height AND height > 0",
            "portrait" => sql += " AND height > width AND width > 0",
            "square" => sql += " AND width = height AND width > 0",
            _ => {}
        }
    }
    if let Some(album) = &f.immich_album {
        sql += " AND path IN (SELECT path FROM immich_albums WHERE album = ?)";
        args.push(album.clone().into());
    }
    if let Some(ids) = &f.ids {
        if ids.is_empty() {
            return Ok(vec![]); // e.g. an empty Immich album; `IN ()` is not valid SQL
        }
        // inline: a whole catalog's results can pass SQLite's limit of 32766 parameters
        sql += &format!(" AND id IN ({})", ids.iter().map(i64::to_string).collect::<Vec<_>>().join(","));
    }
    match f.in_bin {
        Some(true) => sql += " AND deleted_at IS NOT NULL",
        Some(false) => sql += " AND deleted_at IS NULL",
        None => {
            if f.ids.is_none() {
                sql += " AND deleted_at IS NULL";
            }
        }
    }
    sql += " ORDER BY COALESCE(captured_at, ''), filename, id LIMIT ?";
    args.push(f.limit.unwrap_or(1_000_000).into());
    let mut st = c.prepare(&sql).map_err(es)?;
    let rows = st.query_map(params_from_iter(args), row).map_err(es)?;
    rows.collect::<Result<_, _>>().map_err(es)
}

/// The photos with these ids, any number of them (SQLite caps the variables of one query).
pub fn photos(c: &Connection, ids: &[i64]) -> Result<Vec<Photo>, String> {
    let mut out = vec![];
    for chunk in ids.chunks(10_000) {
        out.extend(query(c, &Filter { ids: Some(chunk.to_vec()), ..Default::default() })?);
    }
    Ok(out)
}

/// Local folders only; Immich rows are browsed through their albums.
pub fn folders(c: &Connection) -> Result<Vec<(String, i64)>, String> {
    let mut st = c
        .prepare("SELECT folder, COUNT(*) FROM photos WHERE path NOT LIKE 'immich://%' AND deleted_at IS NULL GROUP BY folder ORDER BY folder")
        .map_err(es)?;
    let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).map_err(es)?;
    rows.collect::<Result<_, _>>().map_err(es)
}

#[derive(Deserialize, Default, Debug)]
#[serde(default)]
pub struct Culling {
    pub rating: Option<i64>,
    pub flag: Option<String>,
    pub color_label: Option<String>,
    pub keywords: Option<String>,
}

pub const COLOR_LABELS: &[&str] = &["none", "red", "yellow", "green", "blue", "purple"];

pub fn update(c: &Connection, ids: &[i64], u: &Culling) -> Result<usize, String> {
    let mut sets: Vec<(&str, Sql)> = vec![];
    if let Some(r) = u.rating {
        if !(0..=5).contains(&r) {
            return Err("rating must be 0-5".into());
        }
        sets.push(("rating", r.into()));
    }
    if let Some(f) = &u.flag {
        sets.push(("flag", flag_value(f)?.into()));
    }
    if let Some(l) = &u.color_label {
        if !COLOR_LABELS.contains(&l.as_str()) {
            return Err(format!("color_label must be one of {}", COLOR_LABELS.join(", ")));
        }
        sets.push(("color_label", if l == "none" { String::new() } else { l.clone() }.into()));
    }
    if let Some(k) = &u.keywords {
        sets.push(("keywords", k.trim().to_string().into()));
    }
    if sets.is_empty() || ids.is_empty() {
        return Ok(0);
    }
    let sql = format!(
        "UPDATE photos SET {} WHERE id IN ({})",
        sets.iter().map(|(k, _)| format!("{k} = ?")).collect::<Vec<_>>().join(", "),
        vec!["?"; ids.len()].join(",")
    );
    let args = sets.into_iter().map(|(_, v)| v).chain(ids.iter().map(|i| Sql::from(*i)));
    c.execute(&sql, params_from_iter(args)).map_err(es)
}

/// `add` merged into a comma-separated keyword list (case-insensitive; the existing order is kept).
pub fn merge_keywords(cur: &str, add: &[String]) -> String {
    let mut out: Vec<String> = cur.split(',').map(|k| k.trim().to_string()).filter(|k| !k.is_empty()).collect();
    for k in add.iter().map(|k| k.trim()).filter(|k| !k.is_empty()) {
        if !out.iter().any(|o| o.eq_ignore_ascii_case(k)) {
            out.push(k.to_string());
        }
    }
    out.join(", ")
}

/// Removes photos and their virtual copies; returns every id that went.
pub fn remove(c: &Connection, ids: &[i64]) -> Result<Vec<i64>, String> {
    let mut gone = vec![];
    for ids in ids.chunks(10_000) {
        // two variables per id; SQLite allows 32766
        let list = vec!["?"; ids.len()].join(",");
        let args: Vec<Sql> = ids.iter().chain(ids).map(|i| Sql::from(*i)).collect();
        let mut st = c
            .prepare(&format!("DELETE FROM photos WHERE id IN ({list}) OR master_id IN ({list}) RETURNING id"))
            .map_err(es)?;
        let rows = st.query_map(params_from_iter(args), |r| r.get(0)).map_err(es)?;
        gone.extend(rows.collect::<Result<Vec<i64>, _>>().map_err(es)?);
    }
    Ok(gone)
}

/// Returns the count of photos currently in the bin.
pub fn bin_count(c: &Connection) -> Result<i64, String> {
    c.query_row("SELECT COUNT(*) FROM photos WHERE deleted_at IS NOT NULL", [], |r| r.get(0)).map_err(es)
}

/// Moves photos and their virtual copies to the bin (soft delete); returns affected ids.
pub fn move_to_bin(c: &Connection, ids: &[i64]) -> Result<Vec<i64>, String> {
    let mut affected = vec![];
    for chunk in ids.chunks(10_000) {
        let list = vec!["?"; chunk.len()].join(",");
        let args: Vec<Sql> = chunk.iter().chain(chunk).map(|i| Sql::from(*i)).collect();
        let mut st = c
            .prepare(&format!(
                "UPDATE photos SET deleted_at = CURRENT_TIMESTAMP WHERE (id IN ({list}) OR master_id IN ({list})) AND deleted_at IS NULL RETURNING id"
            ))
            .map_err(es)?;
        let rows = st.query_map(params_from_iter(args), |r| r.get(0)).map_err(es)?;
        affected.extend(rows.collect::<Result<Vec<i64>, _>>().map_err(es)?);
    }
    Ok(affected)
}

/// Restores photos and their virtual copies from the bin; returns restored ids.
pub fn restore_from_bin(c: &Connection, ids: &[i64]) -> Result<Vec<i64>, String> {
    let mut affected = vec![];
    for chunk in ids.chunks(10_000) {
        let list = vec!["?"; chunk.len()].join(",");
        let args: Vec<Sql> = chunk.iter().chain(chunk).map(|i| Sql::from(*i)).collect();
        let mut st = c
            .prepare(&format!(
                "UPDATE photos SET deleted_at = NULL WHERE (id IN ({list}) OR master_id IN ({list})) AND deleted_at IS NOT NULL RETURNING id"
            ))
            .map_err(es)?;
        let rows = st.query_map(params_from_iter(args), |r| r.get(0)).map_err(es)?;
        affected.extend(rows.collect::<Result<Vec<i64>, _>>().map_err(es)?);
    }
    Ok(affected)
}

// ---------------------------------------------------------------------------
// Edit state + history
// ---------------------------------------------------------------------------

pub fn get_develop(c: &Connection, id: i64) -> Result<Develop, String> {
    let s: String = c
        .query_row("SELECT develop_json FROM photos WHERE id = ?", [id], |r| r.get(0))
        .map_err(|_| format!("photo {id} not found"))?;
    let raw: Develop = serde_json::from_str(&s).unwrap_or_default();
    let mut d = raw.clone();
    d.clamp();
    if d.masks != raw.masks
        && let Ok(json) = serde_json::to_string(&d)
    {
        let _ = c.execute("UPDATE photos SET develop_json = ? WHERE id = ?", params![json, id]);
    }
    Ok(d)
}

fn set_state(c: &Connection, id: i64, json: &str, hid: i64, edited: bool) -> Result<(), String> {
    c.execute(
        "UPDATE photos SET develop_json = ?, history_pos = ?, edited_at = CASE WHEN ? THEN datetime('now') END WHERE id = ?",
        params![json, hid, edited, id],
    )
    .map_err(es)?;
    Ok(())
}

/// New history step; drops any redo branch.
pub fn save_develop(c: &Connection, id: i64, d: &Develop, label: &str) -> Result<(), String> {
    let json = serde_json::to_string(d).map_err(es)?;
    let pos: Option<i64> = c
        .query_row("SELECT history_pos FROM photos WHERE id = ?", [id], |r| r.get(0))
        .map_err(|_| format!("photo {id} not found"))?;
    if let Some(pos) = pos {
        c.execute("DELETE FROM history WHERE photo_id = ? AND id > ?", params![id, pos]).map_err(es)?;
    }
    c.execute("INSERT INTO history (photo_id, label, develop_json) VALUES (?, ?, ?)", params![id, label, json])
        .map_err(es)?;
    set_state(c, id, &json, c.last_insert_rowid(), *d != Develop::default())
}

pub fn history(c: &Connection, id: i64) -> Result<Vec<Value>, String> {
    let pos: Option<i64> =
        c.query_row("SELECT history_pos FROM photos WHERE id = ?", [id], |r| r.get(0)).map_err(es)?;
    let mut st =
        c.prepare("SELECT id, label, created_at FROM history WHERE photo_id = ? ORDER BY id DESC").map_err(es)?;
    let rows = st
        .query_map([id], |r| {
            let hid: i64 = r.get(0)?;
            Ok(json!({"id": hid, "label": r.get::<_, String>(1)?, "created_at": r.get::<_, String>(2)?, "current": Some(hid) == pos}))
        })
        .map_err(es)?;
    rows.collect::<Result<_, _>>().map_err(es)
}

pub fn goto(c: &Connection, id: i64, hid: i64) -> Result<Develop, String> {
    let json: String = c
        .query_row("SELECT develop_json FROM history WHERE id = ? AND photo_id = ?", [hid, id], |r| r.get(0))
        .map_err(|_| "history step not found".to_string())?;
    let mut d: Develop = serde_json::from_str(&json).unwrap_or_default();
    d.clamp();
    let json = serde_json::to_string(&d).map_err(es)?;
    set_state(c, id, &json, hid, d != Develop::default())?;
    Ok(d)
}

/// Undo (dir < 0) or redo (dir > 0). None when there is nothing to step to.
pub fn step(c: &Connection, id: i64, dir: i32) -> Result<Option<Develop>, String> {
    let pos: Option<i64> =
        c.query_row("SELECT history_pos FROM photos WHERE id = ?", [id], |r| r.get(0)).map_err(es)?;
    let sql = if dir < 0 {
        "SELECT MAX(id) FROM history WHERE photo_id = ? AND id < ?"
    } else {
        "SELECT MIN(id) FROM history WHERE photo_id = ? AND id > ?"
    };
    let target: Option<i64> = c.query_row(sql, [id, pos.unwrap_or(0)], |r| r.get(0)).map_err(es)?;
    target.map(|t| goto(c, id, t)).transpose()
}

/// Undo steps: every history step but each photo's current one.
const PAST_STEPS: &str = "FROM history WHERE id NOT IN (SELECT history_pos FROM photos WHERE history_pos IS NOT NULL)";

/// (steps, bytes) that `clear_history` would drop.
pub fn past_steps(c: &Connection) -> Result<(i64, i64), String> {
    c.query_row(&format!("SELECT COUNT(*), COALESCE(SUM(LENGTH(develop_json)), 0) {PAST_STEPS}"), [], |r| Ok((r.get(0)?, r.get(1)?)))
        .map_err(es)
}

/// Drops the undo steps; every photo keeps its current edit.
pub fn clear_history(c: &Connection) -> Result<usize, String> {
    c.execute(&format!("DELETE {PAST_STEPS}"), []).map_err(es)
}

/// Rewrites the catalog without its free pages (and analyses of photos no longer in it) and empties the
/// write-ahead log.
pub fn compact(c: &Connection) -> Result<(), String> {
    c.execute_batch("DELETE FROM analysis WHERE path NOT IN (SELECT path FROM photos); VACUUM; PRAGMA wal_checkpoint(TRUNCATE);").map_err(es)
}

/// What the auto functions measured on a file, if it was measured with these settings (`stamp`).
pub fn analysis(c: &Connection, path: &str, stamp: &str) -> Option<Metrics> {
    let json: String = c.query_row("SELECT metrics FROM analysis WHERE path = ? AND stamp = ?", [path, stamp], |r| r.get(0)).ok()?;
    serde_json::from_str(&json).ok()
}

/// The last measurement of a file, whatever its settings.
pub fn any_analysis(c: &Connection, path: &str) -> Option<Metrics> {
    let json: String = c.query_row("SELECT metrics FROM analysis WHERE path = ?", [path], |r| r.get(0)).ok()?;
    serde_json::from_str(&json).ok()
}

pub fn save_analysis(c: &Connection, path: &str, stamp: &str, m: &Metrics) -> Result<(), String> {
    let json = serde_json::to_string(m).map_err(es)?;
    c.execute("INSERT OR REPLACE INTO analysis (path, stamp, metrics) VALUES (?, ?, ?)", params![path, stamp, json]).map_err(es)?;
    Ok(())
}

pub fn virtual_copy(c: &Connection, id: i64) -> Result<i64, String> {
    let master: i64 =
        c.query_row("SELECT COALESCE(master_id, id) FROM photos WHERE id = ?", [id], |r| r.get(0)).map_err(es)?;
    let n: i64 =
        c.query_row("SELECT COUNT(*) FROM photos WHERE master_id = ?", [master], |r| r.get(0)).map_err(es)?;
    c.execute(
        "INSERT INTO photos (path, folder, filename, master_id, copy_name, width, height, captured_at, camera, lens, iso,
           aperture, shutter, focal_length, rating, flag, color_label, keywords, develop_json, edited_at)
         SELECT path, folder, filename, ?, ?, width, height, captured_at, camera, lens, iso,
           aperture, shutter, focal_length, rating, flag, color_label, keywords, develop_json, edited_at
         FROM photos WHERE id = ?",
        params![master, format!("Copy {}", n + 1), id],
    )
    .map_err(es)?;
    let new = c.last_insert_rowid();
    let d = get_develop(c, new)?;
    save_develop(c, new, &d, "Virtual copy")?;
    Ok(new)
}

pub fn save_as_new_photo(c: &Connection, id: i64, develop: Option<&Develop>, copy_name: Option<&str>) -> Result<i64, String> {
    let master: i64 =
        c.query_row("SELECT COALESCE(master_id, id) FROM photos WHERE id = ?", [id], |r| r.get(0)).map_err(es)?;
    let n: i64 =
        c.query_row("SELECT COUNT(*) FROM photos WHERE master_id = ?", [master], |r| r.get(0)).map_err(es)?;
    let name = match copy_name.map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => s.to_string(),
        None => format!("Copy {}", n + 1),
    };
    c.execute(
        "INSERT INTO photos (path, folder, filename, master_id, copy_name, width, height, captured_at, camera, lens, iso,
           aperture, shutter, focal_length, rating, flag, color_label, keywords, develop_json, edited_at)
         SELECT path, folder, filename, ?, ?, width, height, captured_at, camera, lens, iso,
           aperture, shutter, focal_length, rating, flag, color_label, keywords, develop_json, CURRENT_TIMESTAMP
         FROM photos WHERE id = ?",
        params![master, name, id],
    )
    .map_err(es)?;
    let new = c.last_insert_rowid();
    if let Some(d) = develop {
        save_develop(c, new, d, "Save as new photo")?;
    } else {
        let d = get_develop(c, new)?;
        save_develop(c, new, &d, "Save as new photo")?;
    }
    Ok(new)
}

pub fn summary(c: &Connection) -> Result<Value, String> {
    let one = |sql: &str| -> Result<i64, String> { c.query_row(sql, [], |r| r.get(0)).map_err(es) };
    let top = |col: &str| -> Result<Vec<Value>, String> {
        let mut st = c
            .prepare(&format!(
                "SELECT {col}, COUNT(*) n FROM photos WHERE {col} IS NOT NULL GROUP BY {col} ORDER BY n DESC LIMIT 20"
            ))
            .map_err(es)?;
        let rows = st.query_map([], |r| Ok(json!({"name": r.get::<_, String>(0)?, "count": r.get::<_, i64>(1)?})));
        rows.map_err(es)?.collect::<Result<_, _>>().map_err(es)
    };
    Ok(json!({
        "total_photos": one("SELECT COUNT(*) FROM photos")?,
        "virtual_copies": one("SELECT COUNT(*) FROM photos WHERE master_id IS NOT NULL")?,
        "edited": one("SELECT COUNT(*) FROM photos WHERE edited_at IS NOT NULL")?,
        "picks": one("SELECT COUNT(*) FROM photos WHERE flag = 1")?,
        "rejects": one("SELECT COUNT(*) FROM photos WHERE flag = -1")?,
        "rated_3_plus": one("SELECT COUNT(*) FROM photos WHERE rating >= 3")?,
        "folders": folders(c)?.into_iter().map(|(f, n)| json!({"folder": f, "count": n})).collect::<Vec<_>>(),
        "cameras": top("camera")?,
        "lenses": top("lens")?,
    }))
}

// ---------------------------------------------------------------------------
// Sidecars (spec §6): `<file>.olxmp` JSON next to the original
// ---------------------------------------------------------------------------

pub fn sidecar_path(path: &str) -> PathBuf {
    PathBuf::from(format!("{path}.olxmp"))
}

/// The `.olxmp` document of a photo's edits.
pub fn sidecar_json(p: &Photo, d: &Develop) -> String {
    let doc = json!({"schema_version": "1.0.0", "photo_id": format!("ols:{}", p.id), "source": p.filename, "pipeline": d});
    serde_json::to_string_pretty(&doc).unwrap_or_default()
}

pub fn read_sidecar(path: &Path) -> Option<Develop> {
    let s = fs::read_to_string(sidecar_path(&path.to_string_lossy())).ok()?;
    let v: Value = serde_json::from_str(&s).ok()?;
    serde_json::from_value(v.get("pipeline")?.clone()).ok()
}

/// Masters only (virtual copies live in the catalog, as in Lightroom); Immich photos keep
/// their edits in the catalog. Best effort: read-only volumes just skip the sidecar.
/// ponytail: OpenLight JSON only; read/write Adobe `crs:` XMP when ACR round-trips matter.
pub fn write_sidecar(p: &Photo, d: &Develop) {
    if p.master_id.is_some() || crate::remote::immich_id(&p.path).is_some() {
        return;
    }
    if let Err(e) = fs::write(sidecar_path(&p.path), sidecar_json(p, d)) {
        eprintln!("sidecar {}: {e}", p.path);
    }
}

// ---------------------------------------------------------------------------
// Import
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct Meta {
    pub captured_at: Option<String>,
    pub camera: Option<String>,
    pub lens: Option<String>,
    pub iso: Option<i64>,
    pub aperture: Option<f64>,
    pub shutter: Option<String>,
    pub focal_length: Option<f64>,
    pub orientation: u8,
}

/// New master row with its first history step; None when `path` is already in the catalog.
pub fn add_photo(c: &Connection, path: &str, folder: &str, filename: &str, m: &Meta, d: &Develop) -> Result<Option<i64>, String> {
    let json = serde_json::to_string(d).map_err(es)?;
    let n = c
        .execute(
            "INSERT OR IGNORE INTO photos (path, folder, filename, captured_at, camera, lens, iso, aperture, shutter,
               focal_length, develop_json, edited_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, CASE WHEN ? THEN datetime('now') END)",
            params![
                path,
                folder,
                filename,
                m.captured_at,
                m.camera,
                m.lens,
                m.iso,
                m.aperture,
                m.shutter,
                m.focal_length,
                json,
                *d != Develop::default(),
            ],
        )
        .map_err(es)?;
    if n == 0 {
        return Ok(None);
    }
    let id = c.last_insert_rowid();
    c.execute("INSERT INTO history (photo_id, label, develop_json) VALUES (?, 'Import', ?)", params![id, json]).map_err(es)?;
    c.execute("UPDATE photos SET history_pos = ? WHERE id = ?", params![c.last_insert_rowid(), id]).map_err(es)?;
    Ok(Some(id))
}

/// "Make Model" without repeating the brand ("Canon" + "Canon EOS R5" → "Canon EOS R5").
pub fn camera_name(make: Option<String>, model: Option<String>) -> Option<String> {
    match (make, model) {
        (Some(mk), Some(md)) => {
            let brand = mk.split_whitespace().next().unwrap_or("").to_lowercase();
            Some(if md.to_lowercase().starts_with(&brand) { md } else { format!("{mk} {md}") })
        }
        (mk, md) => md.or(mk),
    }
}

/// EXIF orientation of a file (1 = upright).
pub fn exif_orientation(path: &Path) -> u8 {
    read_exif(path).orientation
}

fn read_exif(path: &Path) -> Meta {
    use exif::{In, Tag, Value as EV};
    let mut m = Meta { orientation: 1, ..Default::default() };
    let Ok(file) = fs::File::open(path) else { return m };
    let Ok(ex) = exif::Reader::new().read_from_container(&mut std::io::BufReader::new(file)) else { return m };
    let field = |t: Tag| ex.get_field(t, In::PRIMARY);
    let s = |t: Tag| {
        field(t).map(|f| f.display_value().to_string().trim_matches('"').trim().to_string()).filter(|s| !s.is_empty())
    };
    let f = |t: Tag| {
        field(t).and_then(|f| match &f.value {
            EV::Rational(v) if !v.is_empty() && v[0].denom != 0 => Some(v[0].to_f64()),
            _ => None,
        })
    };
    m.captured_at = s(Tag::DateTimeOriginal).or_else(|| s(Tag::DateTime));
    m.camera = camera_name(s(Tag::Make), s(Tag::Model));
    m.lens = s(Tag::LensModel);
    m.iso = field(Tag::PhotographicSensitivity).and_then(|f| f.value.get_uint(0)).map(i64::from);
    m.aperture = f(Tag::FNumber);
    m.shutter = s(Tag::ExposureTime);
    m.focal_length = f(Tag::FocalLength);
    m.orientation = field(Tag::Orientation).and_then(|f| f.value.get_uint(0)).unwrap_or(1) as u8;
    m
}

pub const THUMB: u32 = 400;

/// Thumbnail plus the (oriented) pixel size of the decoded source. For RAW files the
/// size comes from the embedded full-size preview.
fn make_thumb(path: &Path, orientation: u8) -> Result<(DynamicImage, u32, u32), String> {
    let img = if develop::is_raw(path) {
        match develop::raw_preview(path) {
            Some(mut img) => {
                if let Some(o) = Orientation::from_exif(orientation) {
                    img.apply_orientation(o);
                }
                img
            }
            None => to_dynamic(&develop::load(path, None)?),
        }
    } else {
        develop::displayable(develop::decode_std(path)?)
    };
    Ok((img.thumbnail(THUMB, THUMB), img.width(), img.height()))
}

pub fn to_dynamic(img: &develop::Img) -> DynamicImage {
    DynamicImage::ImageRgba8(
        image::RgbaImage::from_raw(img.w as u32, img.h as u32, develop::to_rgba8(img)).expect("rgba buffer size"),
    )
}

pub fn write_jpeg(path: &Path, img: &DynamicImage) -> Result<(), String> {
    let file = std::io::BufWriter::new(fs::File::create(path).map_err(es)?);
    img.to_rgb8().write_with_encoder(image::codecs::jpeg::JpegEncoder::new_with_quality(file, 82)).map_err(es)
}

fn scan(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        if e.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        if p.is_dir() {
            scan(&p, out);
        } else if develop::is_supported(&p) {
            out.push(p);
        }
    }
}

/// Classic catalog import: files stay where they are. Returns the new photos' ids.
pub fn import(core: &Core, folder: &Path, progress: &(dyn Fn(usize, usize) + Sync)) -> Result<Vec<i64>, String> {
    let mut files = vec![];
    scan(folder, &mut files);
    import_files(core, files, progress)
}

/// Import these files (those already in the catalog are skipped); returns the new photos' ids.
pub fn import_files(core: &Core, mut files: Vec<PathBuf>, progress: &(dyn Fn(usize, usize) + Sync)) -> Result<Vec<i64>, String> {
    let existing: HashSet<String> = {
        let c = core.db();
        let mut st = c.prepare("SELECT path FROM photos WHERE master_id IS NULL").map_err(es)?;
        let rows = st.query_map([], |r| r.get(0)).map_err(es)?;
        rows.collect::<Result<_, _>>().map_err(es)?
    };
    files.retain(|p| !existing.contains(&*p.to_string_lossy()));
    let metas: Vec<(PathBuf, Meta, Develop)> = files
        .into_par_iter()
        .map(|p| {
            let m = read_exif(&p);
            let d = read_sidecar(&p).unwrap_or_default();
            (p, m, d)
        })
        .collect();
    let mut added: Vec<(i64, PathBuf, u8)> = vec![];
    {
        let mut c = core.db();
        let tx = c.transaction().map_err(es)?;
        for (p, m, d) in &metas {
            let folder = p.parent().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
            let name = p.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
            if let Some(id) = add_photo(&tx, &p.to_string_lossy(), &folder, &name, m, d)? {
                added.push((id, p.clone(), m.orientation));
            }
        }
        tx.commit().map_err(es)?;
    }
    let total = added.len();
    let done = AtomicUsize::new(0);
    let dims: Vec<(i64, u32, u32)> = added
        .par_iter()
        .filter_map(|(id, p, o)| {
            let r = make_thumb(p, *o).and_then(|(t, w, h)| write_jpeg(&core.thumb_path(*id), &t).map(|_| (w, h)));
            progress(done.fetch_add(1, Ordering::Relaxed) + 1, total);
            match r {
                Ok((w, h)) => Some((*id, w, h)),
                Err(e) => {
                    eprintln!("thumbnail {}: {e}", p.display());
                    None
                }
            }
        })
        .collect();
    let c = core.db();
    for (id, w, h) in dims {
        c.execute("UPDATE photos SET width = ?, height = ? WHERE id = ?", params![w, h, id]).map_err(es)?;
    }
    Ok(added.into_iter().map(|a| a.0).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_filter_and_save_as_new_photo() {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(SCHEMA).unwrap();

        // Insert original unedited photo
        c.execute(
            "INSERT INTO photos (id, path, folder, filename, width, height) VALUES (1, '/photos/a.jpg', '/photos', 'a.jpg', 6000, 4000)",
            [],
        )
        .unwrap();

        // Insert original edited photo
        c.execute(
            "INSERT INTO photos (id, path, folder, filename, width, height, edited_at) VALUES (2, '/photos/b.jpg', '/photos', 'b.jpg', 4000, 6000, CURRENT_TIMESTAMP)",
            [],
        )
        .unwrap();

        // 1. Check all photos
        let all = query(&c, &Filter::default()).unwrap();
        assert_eq!(all.len(), 2);

        // 2. Filter original photos
        let orig = query(&c, &Filter { copy_type: Some("original".into()), ..Default::default() }).unwrap();
        assert_eq!(orig.len(), 2);

        // 3. Filter edited photos
        let edited = query(&c, &Filter { copy_type: Some("edited".into()), ..Default::default() }).unwrap();
        assert_eq!(edited.len(), 1);
        assert_eq!(edited[0].id, 2);

        // 4. Filter unedited original photos
        let unedited = query(&c, &Filter { copy_type: Some("unedited_original".into()), ..Default::default() }).unwrap();
        assert_eq!(unedited.len(), 1);
        assert_eq!(unedited[0].id, 1);

        // 5. Save as new photo from photo 1 with custom develop and custom name
        let mut d = Develop::default();
        d.basic_tone.exposure = 1.5;
        let new_id = save_as_new_photo(&c, 1, Some(&d), Some("B&W High Contrast")).unwrap();
        assert_eq!(new_id, 3);

        // Verify the saved new edited photo
        let p3 = query(&c, &Filter { ids: Some(vec![3]), ..Default::default() }).unwrap();
        assert_eq!(p3.len(), 1);
        assert_eq!(p3[0].master_id, Some(1));
        assert_eq!(p3[0].copy_name.as_deref(), Some("B&W High Contrast"));
        assert!(p3[0].edited);

        // 6. Filter saved new edited photos (copy)
        let copies = query(&c, &Filter { copy_type: Some("copy".into()), ..Default::default() }).unwrap();
        assert_eq!(copies.len(), 1);
        assert_eq!(copies[0].id, 3);

        // 7. Check aspect filter
        let landscape = query(&c, &Filter { aspect: Some("landscape".into()), ..Default::default() }).unwrap();
        assert_eq!(landscape.len(), 2); // id 1 (6000x4000) and id 3 (copied from 1)
        let portrait = query(&c, &Filter { aspect: Some("portrait".into()), ..Default::default() }).unwrap();
        assert_eq!(portrait.len(), 1); // id 2 (4000x6000)

        // 8. Test bin functions: move to bin, bin_count, query in_bin, restore_from_bin
        assert_eq!(bin_count(&c).unwrap(), 0);
        let moved = move_to_bin(&c, &[1]).unwrap();
        // Photo 1 and its virtual copy Photo 3 should both be moved to bin
        assert!(moved.contains(&1));
        assert!(moved.contains(&3));
        assert_eq!(bin_count(&c).unwrap(), 2);

        // Regular query now only returns photo 2
        let regular = query(&c, &Filter::default()).unwrap();
        assert_eq!(regular.len(), 1);
        assert_eq!(regular[0].id, 2);

        // Query bin explicitly
        let in_bin = query(&c, &Filter { in_bin: Some(true), ..Default::default() }).unwrap();
        assert_eq!(in_bin.len(), 2);

        // Restore photo 1 from bin
        let restored = restore_from_bin(&c, &[1]).unwrap();
        assert!(restored.contains(&1));
        assert!(restored.contains(&3));
        assert_eq!(bin_count(&c).unwrap(), 0);

        let regular_after = query(&c, &Filter::default()).unwrap();
        assert_eq!(regular_after.len(), 3);
    }

    #[test]
    fn test_migration_from_legacy_catalog_without_deleted_at() {
        let dir = std::env::temp_dir().join(format!("ols-migration-test-{}", crate::new_token()));
        fs::create_dir_all(&dir).unwrap();
        let db_path = dir.join("legacy.olcat");

        // Create a database with the old schema (without deleted_at column and without photos_deleted index)
        {
            let conn = Connection::open(&db_path).unwrap();
            conn.execute_batch(
                "CREATE TABLE photos (
                  id INTEGER PRIMARY KEY,
                  path TEXT NOT NULL,
                  folder TEXT NOT NULL,
                  filename TEXT NOT NULL,
                  master_id INTEGER,
                  copy_name TEXT,
                  width INTEGER NOT NULL DEFAULT 0,
                  height INTEGER NOT NULL DEFAULT 0,
                  captured_at TEXT,
                  camera TEXT,
                  lens TEXT,
                  iso INTEGER,
                  aperture REAL,
                  shutter TEXT,
                  focal_length REAL,
                  rating INTEGER NOT NULL DEFAULT 0,
                  flag INTEGER NOT NULL DEFAULT 0,
                  color_label TEXT NOT NULL DEFAULT '',
                  keywords TEXT NOT NULL DEFAULT '',
                  develop_json TEXT NOT NULL DEFAULT '{}',
                  history_pos INTEGER,
                  thumb_v INTEGER NOT NULL DEFAULT 0,
                  imported_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                  edited_at TEXT
                );
                CREATE UNIQUE INDEX photos_path ON photos(path) WHERE master_id IS NULL;
                CREATE INDEX photos_folder ON photos(folder);
                INSERT INTO photos (path, folder, filename) VALUES ('/tmp/photo1.jpg', '/tmp', 'photo1.jpg');",
            )
            .unwrap();
        }

        // Now open the legacy database with catalog::open - this should successfully migrate it!
        let c = open(&db_path).expect("open should succeed and migrate the legacy database");

        // Verify that deleted_at column is now usable
        assert_eq!(bin_count(&c).unwrap(), 0);
        let list = query(&c, &Filter::default()).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, 1);
        assert_eq!(list[0].deleted_at, None);

        // Verify that moving to bin works on the migrated catalog
        let moved = move_to_bin(&c, &[1]).unwrap();
        assert_eq!(moved, vec![1]);
        assert_eq!(bin_count(&c).unwrap(), 1);

        // Verify that restore works
        let restored = restore_from_bin(&c, &[1]).unwrap();
        assert_eq!(restored, vec![1]);
        assert_eq!(bin_count(&c).unwrap(), 0);
        drop(c);
        let _ = fs::remove_dir_all(&dir);
    }
}
