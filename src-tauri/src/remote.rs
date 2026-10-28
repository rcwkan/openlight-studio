//! Remote photo sources: a local Immich server and Google Photos.
//!
//! Originals are downloaded to `<data dir>/remote/<source>/<album>/` and imported like any
//! folder, so develop, sidecars and export work unchanged. Immich authenticates with an API
//! key. Google Photos goes through the Picker API (the Library API no longer lists a user's
//! photos) with the user's own OAuth "Desktop app" client. Secrets live in the OS keychain.

use crate::{
    Core, Settings, ai, catalog,
    develop::{self, es},
};
use base64::Engine;
use rayon::prelude::*;
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    fs, io,
    path::Path,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
    time::{Duration, Instant},
};

const IMMICH_KEY: &str = "remote:immich";
/// Keychain entry holding `{client_secret, refresh_token}`.
const GOOGLE_KEY: &str = "remote:google-photos";
const GOOGLE_TOKEN: &str = "https://oauth2.googleapis.com/token";
const PICKER: &str = "https://photospicker.googleapis.com/v1";

type Reply = Result<ureq::http::Response<ureq::Body>, ureq::Error>;

/// No overall deadline (originals can be large); a stalled read gives up after a minute.
fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_connect(Some(Duration::from_secs(10)))
        .timeout_per_read(Some(Duration::from_secs(60)))
        .build()
        .into()
}

fn json_of(what: &str, reply: Reply) -> Result<Value, String> {
    let mut r = reply.map_err(|e| format!("{what}: {e}"))?;
    let status = r.status().as_u16();
    let body = r.body_mut().read_to_string().map_err(es)?;
    if status >= 400 {
        return Err(format!("{what} returned HTTP {status}: {}", body.chars().take(300).collect::<String>()));
    }
    serde_json::from_str(&body).map_err(|_| format!("{what} did not reply with JSON; check the address"))
}

fn update_settings(core: &Core, f: impl FnOnce(&mut Settings)) -> Result<Settings, String> {
    let s = {
        let mut s = core.settings.lock().unwrap();
        f(&mut s);
        s.clone()
    };
    core.save_settings()?;
    Ok(s)
}

pub fn disconnect(core: &Core, source: &str) -> Result<Settings, String> {
    match source {
        "immich" => {
            ai::set_key(IMMICH_KEY, "")?;
            update_settings(core, |s| s.immich_url.clear())
        }
        "google" => {
            ai::set_key(GOOGLE_KEY, "")?;
            update_settings(core, |s| s.google_client_id.clear())
        }
        _ => Err(format!("unknown source '{source}'")),
    }
}

// ---------------------------------------------------------------------------
// Immich
// ---------------------------------------------------------------------------

/// `host:2283`, `http://host:2283/`, `…/api` → `http://host:2283`.
fn immich_base(url: &str) -> String {
    let u = url.trim().trim_end_matches('/');
    let u = u.strip_suffix("/api").unwrap_or(u);
    if u.contains("://") { u.into() } else { format!("http://{u}") }
}

fn immich_albums_at(url: &str, key: &str) -> Result<Vec<Value>, String> {
    let v = json_of("Immich", agent().get(&format!("{url}/api/albums")).header("x-api-key", key).call())?;
    let albums = v.as_array().ok_or("Immich did not return an album list")?;
    Ok(albums.iter().map(|a| json!({"id": a["id"], "name": a["albumName"], "count": a["assetCount"]})).collect())
}

fn immich(core: &Core) -> Result<(String, String), String> {
    let url = core.settings.lock().unwrap().immich_url.clone();
    let key = ai::key(IMMICH_KEY).filter(|_| !url.is_empty()).ok_or("Immich is not connected")?;
    Ok((url, key))
}

/// Checks the address and key (listing albums needs `album.read`), then stores both.
pub fn immich_connect(core: &Core, url: &str, key: &str) -> Result<Settings, String> {
    let url = immich_base(url);
    let parsed = tauri::Url::parse(&url).map_err(|e| format!("invalid Immich address: {e}"))?;
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return Err("Immich address must use http or https".into());
    }
    let key = key.trim();
    immich_albums_at(&url, key)?;
    ai::set_key(IMMICH_KEY, key)?;
    update_settings(core, |s| s.immich_url = url)
}

pub fn immich_albums(core: &Core) -> Result<Vec<Value>, String> {
    let (url, key) = immich(core)?;
    immich_albums_at(&url, &key)
}

/// Download and import an album, or the favorites when `album` is None.
pub fn immich_import(core: &Core, album: Option<&str>) -> Result<Value, String> {
    let (url, key) = immich(core)?;
    import_from_immich(core, &url, &key, album)
}

fn import_from_immich(core: &Core, url: &str, key: &str, album: Option<&str>) -> Result<Value, String> {
    let (folder, count) = match album {
        Some(id) => {
            let a = immich_albums_at(url, key)?
                .into_iter()
                .find(|a| a["id"] == id)
                .ok_or("album not found on the Immich server")?;
            (a["name"].as_str().unwrap_or("Album").to_string(), a["count"].as_u64())
        }
        None => ("Favorites".to_string(), None),
    };
    // ponytail: pre-3.2 search fields (deprecated, still served by Immich 3.x) keep older servers working; move to `filter` + `cursor` once those are rare
    let mut assets = vec![];
    let mut page = 1;
    loop {
        let mut q = json!({"type": "IMAGE", "size": 1000, "page": page});
        match album {
            Some(id) => q["albumIds"] = json!([id]),
            None => q["isFavorite"] = json!(true),
        }
        let r = json_of("Immich", agent().post(&format!("{url}/api/search/metadata")).header("x-api-key", key).send_json(&q))?;
        assets.extend(r["assets"]["items"].as_array().cloned().unwrap_or_default());
        match r["assets"]["nextPage"].as_str().and_then(|p| p.parse().ok()) {
            Some(next) if next > page => page = next,
            _ => break,
        }
    }
    // A server that ignored a filter would otherwise hand over the whole library.
    if album.is_none() {
        assets.retain(|a| a["isFavorite"] == true);
    } else if count.is_some_and(|n| assets.len() as u64 > n) {
        return Err("this Immich server ignored the album filter; update Immich and try again".into());
    }
    let items: Vec<Item> = assets
        .iter()
        .filter_map(|a| {
            let (id, name) = (a["id"].as_str()?, a["originalFileName"].as_str()?);
            save_ext(name, "")?; // originals keep their format: skip HEIC, video… before downloading
            Some(Item { id: id.into(), name: name.into(), url: format!("{url}/api/assets/{id}/original") })
        })
        .collect();
    fetch_and_import(core, &["Immich", &folder], assets.len() - items.len(), items, ("x-api-key", key))
}

// ---------------------------------------------------------------------------
// Google Photos
// ---------------------------------------------------------------------------

fn open_browser(url: &str) -> Result<(), String> {
    let parsed = tauri::Url::parse(url).map_err(|e| format!("invalid URL ({e}): {url}"))?;
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return Err(format!("refusing to open non-http/https URL: {url}"));
    }
    let (prog, args): (&str, &[&str]) = if cfg!(target_os = "macos") {
        ("open", &["--"])
    } else if cfg!(windows) {
        ("rundll32", &["url.dll,FileProtocolHandler"])
    } else {
        ("xdg-open", &["--"])
    };
    let mut child =
        Command::new(prog).args(args).arg(url).spawn().map_err(|e| format!("could not open a browser ({e}); visit {url}"))?;
    std::thread::spawn(move || child.wait());
    Ok(())
}

/// Sign in through the browser: loopback redirect + PKCE (RFC 8252). Stores the refresh token.
pub fn google_connect(core: &Core, client_id: &str, client_secret: &str) -> Result<Settings, String> {
    let (client_id, client_secret) = (client_id.trim(), client_secret.trim());
    let server = tiny_http::Server::http("127.0.0.1:0").map_err(es)?;
    let port = server.server_addr().to_ip().ok_or("no loopback port")?.port();
    let redirect = format!("http://127.0.0.1:{port}");
    let (verifier, state) = (crate::new_token(), crate::new_token());
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(ring::digest::digest(&ring::digest::SHA256, verifier.as_bytes()));
    let auth = tauri::Url::parse_with_params(
        "https://accounts.google.com/o/oauth2/v2/auth",
        [
            ("client_id", client_id),
            ("redirect_uri", redirect.as_str()),
            ("response_type", "code"),
            ("scope", "https://www.googleapis.com/auth/photospicker.mediaitems.readonly"),
            ("code_challenge", challenge.as_str()),
            ("code_challenge_method", "S256"),
            ("state", state.as_str()),
            ("access_type", "offline"),
            ("prompt", "consent"),
        ],
    )
    .map_err(es)?;
    open_browser(auth.as_str())?;
    let code = wait_for_code(&server, &state)?;
    let tok = json_of(
        "Google sign-in",
        agent().post(GOOGLE_TOKEN).send_form([
            ("code", code.as_str()),
            ("client_id", client_id),
            ("client_secret", client_secret),
            ("redirect_uri", redirect.as_str()),
            ("grant_type", "authorization_code"),
            ("code_verifier", verifier.as_str()),
        ]),
    )?;
    let refresh = tok["refresh_token"].as_str().ok_or("Google returned no refresh token")?;
    ai::set_key(GOOGLE_KEY, &json!({"client_secret": client_secret, "refresh_token": refresh}).to_string())?;
    update_settings(core, |s| s.google_client_id = client_id.into())
}

/// Serve Google's redirect on the loopback listener and return the authorization code.
fn wait_for_code(server: &tiny_http::Server, state: &str) -> Result<String, String> {
    let deadline = Instant::now() + Duration::from_secs(300);
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        let Some(req) = server.recv_timeout(left).map_err(es)? else { break };
        let url = tauri::Url::parse(&format!("http://127.0.0.1{}", req.url())).map_err(es)?;
        let q: HashMap<String, String> = url.query_pairs().into_owned().collect();
        if q.get("state").map(String::as_str) != Some(state) {
            let _ = req.respond(tiny_http::Response::empty(404)); // favicon, stray or forged requests
            continue;
        }
        let res = match (q.get("code"), q.get("error")) {
            (Some(code), _) => Ok(code.clone()),
            (_, err) => Err(format!("Google sign-in failed: {}", err.map_or("no code returned", String::as_str))),
        };
        let page = if res.is_ok() { "OpenLight Studio is connected to Google Photos." } else { "Google sign-in did not complete." };
        let ct = tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"text/plain; charset=utf-8"[..]).expect("static header");
        let nosniff = tiny_http::Header::from_bytes(&b"X-Content-Type-Options"[..], &b"nosniff"[..]).expect("static header");
        let _ = req.respond(
            tiny_http::Response::from_string(format!("{page} You can close this tab."))
                .with_header(ct)
                .with_header(nosniff),
        );
        return res;
    }
    Err("timed out waiting for Google sign-in".into())
}

fn google_access_token(core: &Core) -> Result<String, String> {
    let client_id = core.settings.lock().unwrap().google_client_id.clone();
    let saved: Value = ai::key(GOOGLE_KEY)
        .filter(|_| !client_id.is_empty())
        .and_then(|s| serde_json::from_str(&s).ok())
        .ok_or("Google Photos is not connected")?;
    let tok = json_of(
        "Google sign-in",
        agent().post(GOOGLE_TOKEN).send_form([
            ("client_id", client_id.as_str()),
            ("client_secret", saved["client_secret"].as_str().unwrap_or("")),
            ("refresh_token", saved["refresh_token"].as_str().unwrap_or("")),
            ("grant_type", "refresh_token"),
        ]),
    )?;
    tok["access_token"].as_str().map(String::from).ok_or_else(|| "Google returned no access token".into())
}

/// Open the Google Photos picker in the browser, wait for the user's choice, then
/// download and import the picked photos.
pub fn google_import(core: &Core) -> Result<Value, String> {
    let bearer = format!("Bearer {}", google_access_token(core)?);
    let agent = agent();
    let session = json_of(
        "Google Photos",
        agent.post(&format!("{PICKER}/sessions")).header("Authorization", &bearer).send_json(json!({})),
    )?;
    let id = session["id"].as_str().ok_or("Google Photos did not start a picking session")?;
    let res = (|| {
        // `open` would also launch a local path, so only hand it a web link.
        let picker = session["pickerUri"].as_str().filter(|u| u.starts_with("https://")).ok_or("Google Photos returned no picker link")?;
        open_browser(&format!("{picker}/autoclose"))?;
        // Durations arrive as "3.5s"; clamp so a bad value cannot stall or panic.
        let secs = |v: &Value| v.as_str().and_then(|s| s.trim_end_matches('s').parse::<f64>().ok()).map(|s| s.max(0.0).min(7200.0));
        let deadline = Instant::now() + Duration::from_secs_f64(secs(&session["pollingConfig"]["timeoutIn"]).unwrap_or(1800.0));
        let mut s = session.clone();
        while s["mediaItemsSet"] != true {
            if Instant::now() > deadline {
                return Err("timed out waiting for photos to be picked in Google Photos".to_string());
            }
            std::thread::sleep(Duration::from_secs_f64(secs(&s["pollingConfig"]["pollInterval"]).unwrap_or(5.0).clamp(1.0, 30.0)));
            s = json_of("Google Photos", agent.get(&format!("{PICKER}/sessions/{id}")).header("Authorization", &bearer).call())?;
        }
        let (mut items, mut picked, mut token) = (vec![], 0, String::new());
        loop {
            let mut req = agent
                .get(&format!("{PICKER}/mediaItems"))
                .header("Authorization", &bearer)
                .query("sessionId", id)
                .query("pageSize", "100");
            if !token.is_empty() {
                req = req.query("pageToken", &token);
            }
            let page = json_of("Google Photos", req.call())?;
            for m in page["mediaItems"].as_array().into_iter().flatten() {
                picked += 1;
                if let (true, Some(mid), Some(base)) = (m["type"] == "PHOTO", m["id"].as_str(), m["mediaFile"]["baseUrl"].as_str()) {
                    let name = m["mediaFile"]["filename"].as_str().unwrap_or_default();
                    items.push(Item { id: mid.into(), name: name.into(), url: format!("{base}=d") });
                }
            }
            match page["nextPageToken"].as_str() {
                Some(t) if !t.is_empty() => token = t.into(),
                _ => break,
            }
        }
        fetch_and_import(core, &["Google Photos"], picked - items.len(), items, ("Authorization", &bearer))
    })();
    let _ = agent.delete(&format!("{PICKER}/sessions/{id}")).header("Authorization", &bearer).call();
    res
}

// ---------------------------------------------------------------------------
// Download + import
// ---------------------------------------------------------------------------

struct Item {
    id: String,
    /// Remote file name (untrusted).
    name: String,
    url: String,
}

/// Remote names are untrusted: keep one plain, portable path component.
fn safe_name(s: &str) -> String {
    let s: String = s.chars().map(|c| if c.is_control() || r#"/\:*?"<>|"#.contains(c) { '_' } else { c }).take(100).collect();
    let s = s.trim().trim_matches('.').trim();
    if s.is_empty() { "untitled".into() } else { s.into() }
}

/// `IMG_0001_<id tail>`: stable across re-imports and unique per remote item.
fn local_stem(it: &Item) -> String {
    let name = safe_name(&it.name);
    let stem = Path::new(&name).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let id: String = it.id.chars().filter(char::is_ascii_alphanumeric).collect();
    format!("{stem}_{}", &id[id.len().saturating_sub(8)..])
}

/// Extension to store a download under, None when OpenLight can't decode it. The remote
/// name decides, unless the server sends a plain image type for a name we can't read as
/// one (Google may serve a JPEG of a HEIC or RAW original).
fn save_ext(name: &str, content_type: &str) -> Option<String> {
    let named = Path::new(name).extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    let served = match content_type.split(';').next().unwrap_or("").trim().to_ascii_lowercase().as_str() {
        "image/jpeg" => Some("jpg"),
        "image/png" => Some("png"),
        "image/webp" => Some("webp"),
        "image/tiff" => Some("tif"),
        _ => None,
    };
    let raw = develop::RAW_EXTS.contains(&named.as_str()) && matches!(served, None | Some("tif")); // DNG and most RAWs are TIFF containers
    if raw || develop::IMG_EXTS.contains(&named.as_str()) { Some(named) } else { served.map(String::from) }
}

/// Stream one original to `<dir>/<stem>.<ext>`; Ok(false) when its format is unsupported.
fn download(agent: &ureq::Agent, it: &Item, auth: (&str, &str), dir: &Path, stem: &str) -> Result<bool, String> {
    let mut r = agent.get(&it.url).header(auth.0, auth.1).call().map_err(es)?;
    let status = r.status().as_u16();
    if status >= 400 {
        return Err(format!("HTTP {status}"));
    }
    let ct = r.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or("");
    let Some(ext) = save_ext(&it.name, ct) else { return Ok(false) };
    // Write aside, then rename: an interrupted download never looks complete.
    let part = dir.join(format!("{stem}.{ext}.part"));
    let res = fs::File::create(&part)
        .and_then(|mut f| io::copy(&mut r.body_mut().as_reader(), &mut f))
        .and_then(|_| fs::rename(&part, dir.join(format!("{stem}.{ext}"))));
    if let Err(e) = res {
        let _ = fs::remove_file(&part);
        return Err(es(e));
    }
    Ok(true)
}

/// Download what `remote/<folder…>` doesn't have yet, then import that folder.
fn fetch_and_import(core: &Core, folder: &[&str], skipped: usize, items: Vec<Item>, auth: (&str, &str)) -> Result<Value, String> {
    let dir = folder.iter().fold(core.dir.join("remote"), |d, f| d.join(safe_name(f)));
    fs::create_dir_all(&dir).map_err(es)?;
    let have: HashSet<String> = fs::read_dir(&dir)
        .map_err(es)?
        .flatten()
        .filter_map(|e| e.path().file_stem().map(|s| s.to_string_lossy().into_owned()))
        .collect();
    let todo: Vec<(String, Item)> =
        items.into_iter().map(|it| (local_stem(&it), it)).filter(|(stem, _)| !have.contains(stem)).collect();
    let (agent, done) = (agent(), AtomicUsize::new(0));
    let results: Vec<Result<bool, String>> = todo
        .par_iter()
        .map(|(stem, it)| {
            let r = download(&agent, it, auth, &dir, stem).map_err(|e| format!("{}: {e}", it.name));
            let n = done.fetch_add(1, Ordering::Relaxed) + 1;
            core.emit("import-progress", json!({"label": "Downloading", "done": n, "total": todo.len()}));
            r
        })
        .collect();
    let failed: Vec<&String> = results.iter().filter_map(|r| r.as_ref().err()).collect();
    for e in &failed {
        eprintln!("remote download {e}");
    }
    let imported = catalog::import(core, &dir, &|done, total| core.emit("import-progress", json!({"done": done, "total": total})))?;
    core.emit("catalog-changed", json!({}));
    Ok(json!({
        "folder": dir,
        "imported": imported,
        "skipped": skipped + results.iter().filter(|r| matches!(r, Ok(false))).count(),
        "failed": failed.len(),
        "error": failed.first(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_names_stay_inside_the_folder() {
        assert_eq!(safe_name("../../.zshrc"), "_.._.zshrc");
        assert_eq!(safe_name(".."), "untitled");
        assert_eq!(safe_name("a\\b:c/IMG_1.JPG"), "a_b_c_IMG_1.JPG");
        assert_eq!(safe_name("  Trip. "), "Trip");
        let it = Item { id: "0198c1d2-7e4f-7a3b-9c5d-1e2f3a4b5c6d".into(), name: "../IMG_0001.CR3".into(), url: String::new() };
        assert_eq!(local_stem(&it), "_IMG_0001_3a4b5c6d");
    }

    #[test]
    fn saved_extension_follows_the_bytes() {
        assert_eq!(save_ext("IMG_1.JPG", "").as_deref(), Some("jpg"));
        assert_eq!(save_ext("IMG_1.CR3", "application/octet-stream").as_deref(), Some("cr3"));
        assert_eq!(save_ext("IMG_1.DNG", "image/tiff").as_deref(), Some("dng"));
        assert_eq!(save_ext("IMG_1.DNG", "image/jpeg").as_deref(), Some("jpg"));
        assert_eq!(save_ext("IMG_1.HEIC", "image/jpeg; charset=binary").as_deref(), Some("jpg"));
        assert_eq!(save_ext("IMG_1.HEIC", "image/heic"), None);
        assert_eq!(save_ext("clip.mp4", "video/mp4"), None);
    }

    #[test]
    fn immich_addresses_normalise() {
        assert_eq!(immich_base("192.168.1.5:2283/api/"), "http://192.168.1.5:2283");
        assert_eq!(immich_base(" https://photos.example.net/ "), "https://photos.example.net");
    }

    /// Fake Immich on loopback: a two-page album with a JPEG, a HEIC and an asset that fails.
    #[test]
    fn immich_album_downloads_and_imports_once() {
        let mut jpeg = vec![];
        image::DynamicImage::new_rgb8(8, 8).write_to(&mut io::Cursor::new(&mut jpeg), image::ImageFormat::Jpeg).unwrap();
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let url = format!("http://127.0.0.1:{}", server.server_addr().to_ip().unwrap().port());
        std::thread::spawn(move || {
            for mut req in server.incoming_requests() {
                assert!(req.headers().iter().any(|h| h.field.equiv("x-api-key") && h.value.as_str() == "k"));
                let mut body = String::new();
                req.as_reader().read_to_string(&mut body).unwrap();
                let (ct, data) = match req.url() {
                    "/api/albums" => ("application/json", json!([{"id": "a1", "albumName": "Trip/2024", "assetCount": 3}]).to_string().into_bytes()),
                    "/api/search/metadata" => {
                        let first = serde_json::from_str::<Value>(&body).unwrap()["page"] == 1;
                        let items = if first {
                            json!([{"id": "u-1", "originalFileName": "IMG_1.JPG"}, {"id": "u-2", "originalFileName": "IMG_2.HEIC"}])
                        } else {
                            json!([{"id": "u-3", "originalFileName": "IMG_3.JPG"}])
                        };
                        ("application/json", json!({"assets": {"items": items, "nextPage": if first { json!("2") } else { Value::Null }}}).to_string().into_bytes())
                    }
                    "/api/assets/u-1/original" => ("image/jpeg", jpeg.clone()),
                    _ => ("text/plain", vec![]),
                };
                let code = if data.is_empty() { 500 } else { 200 };
                let h = tiny_http::Header::from_bytes(&b"Content-Type"[..], ct.as_bytes()).unwrap();
                let _ = req.respond(tiny_http::Response::from_data(data).with_status_code(code).with_header(h));
            }
        });
        let dir = std::env::temp_dir().join(format!("ols-remote-test-{}", crate::new_token()));
        let core = Core::open(dir.clone()).unwrap();
        let counts = |r: &Value| (r["imported"].as_u64(), r["skipped"].as_u64(), r["failed"].as_u64());
        let r = import_from_immich(&core, &url, "k", Some("a1")).unwrap();
        assert_eq!(counts(&r), (Some(1), Some(1), Some(1)), "{r}");
        assert!(dir.join("remote/Immich/Trip_2024/IMG_1_u1.jpg").exists());
        let again = import_from_immich(&core, &url, "k", Some("a1")).unwrap();
        assert_eq!(counts(&again), (Some(0), Some(1), Some(1)), "{again}");
        let _ = fs::remove_dir_all(&dir);
    }
}
