use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_store::{Store, StoreExt};

// ─── Settings ───────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AppSettings {
    pub install_path: String,
    pub build_server_url: String,
    pub sso_url: String,
    pub signing_identity: String,
    pub apple_team_id: String,
    pub windows_cert_path: String,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            install_path: default_install_path(),
            build_server_url: "http://147.93.30.25:8888/rizzeadmin/SWBuild/raw/branch/main"
                .to_string(),
            sso_url: "https://sso.lightningworks.io".to_string(),
            signing_identity: String::new(),
            apple_team_id: String::new(),
            windows_cert_path: String::new(),
        }
    }
}

fn default_install_path() -> String {
    if cfg!(target_os = "macos") {
        dirs::home_dir()
            .map(|h| h.join("Games").join("Siege Worlds").to_string_lossy().to_string())
            .unwrap_or_else(|| "/Applications/Siege Worlds".to_string())
    } else {
        "C:\\Games\\Siege Worlds".to_string()
    }
}

fn get_store(app: &AppHandle) -> Arc<Store<tauri::Wry>> {
    app.store("settings.json").expect("failed to access store")
}

#[tauri::command]
pub fn get_settings(app: AppHandle) -> AppSettings {
    let store = get_store(&app);
    let defaults = AppSettings::default();
    AppSettings {
        install_path: store
            .get("install_path")
            .and_then(|v| v.as_str().map(|s| s.to_string()))
            .unwrap_or(defaults.install_path),
        build_server_url: store
            .get("build_server_url")
            .and_then(|v| v.as_str().map(|s| s.to_string()))
            .unwrap_or(defaults.build_server_url),
        sso_url: store
            .get("sso_url")
            .and_then(|v| v.as_str().map(|s| s.to_string()))
            .unwrap_or(defaults.sso_url),
        signing_identity: store
            .get("signing_identity")
            .and_then(|v| v.as_str().map(|s| s.to_string()))
            .unwrap_or(defaults.signing_identity),
        apple_team_id: store
            .get("apple_team_id")
            .and_then(|v| v.as_str().map(|s| s.to_string()))
            .unwrap_or(defaults.apple_team_id),
        windows_cert_path: store
            .get("windows_cert_path")
            .and_then(|v| v.as_str().map(|s| s.to_string()))
            .unwrap_or(defaults.windows_cert_path),
    }
}

#[tauri::command]
pub fn save_settings(app: AppHandle, settings: AppSettings) -> Result<(), String> {
    let store = get_store(&app);
    store.set("install_path", serde_json::json!(settings.install_path));
    store.set(
        "build_server_url",
        serde_json::json!(settings.build_server_url),
    );
    store.set("sso_url", serde_json::json!(settings.sso_url));
    store.set(
        "signing_identity",
        serde_json::json!(settings.signing_identity),
    );
    store.set("apple_team_id", serde_json::json!(settings.apple_team_id));
    store.set(
        "windows_cert_path",
        serde_json::json!(settings.windows_cert_path),
    );
    store.save().map_err(|e| format!("Failed to save: {}", e))
}

#[tauri::command]
pub async fn select_install_path(app: AppHandle) -> Result<String, String> {
    let path = app
        .dialog()
        .file()
        .blocking_pick_folder()
        .map(|p| p.to_string())
        .ok_or_else(|| "No folder selected".to_string())?;

    let store = get_store(&app);
    store.set("install_path", serde_json::json!(&path));
    let _ = store.save();
    Ok(path)
}

// ─── Path Safety ────────────────────────────────────────────────────────────

/// Validate that a manifest file path is safe (no path traversal attacks).
fn validate_manifest_path(path: &str) -> Result<(), String> {
    // Reject absolute paths
    if path.starts_with('/') || path.starts_with('\\') {
        return Err(format!("Rejected absolute path in manifest: {}", path));
    }
    // Reject Windows drive letters (e.g. C:\)
    if path.len() >= 2 && path.as_bytes()[1] == b':' {
        return Err(format!("Rejected absolute path in manifest: {}", path));
    }
    // Reject path traversal components
    for component in path.split(['/', '\\']) {
        if component == ".." {
            return Err(format!(
                "Rejected path traversal in manifest: {}",
                path
            ));
        }
    }
    Ok(())
}

// ─── File Hashing ───────────────────────────────────────────────────────────

/// Compute SHA-256 hash of a file, returned as lowercase hex string.
fn hash_file(path: &PathBuf) -> Result<String, String> {
    let mut file =
        std::fs::File::open(path).map_err(|e| format!("Failed to open for hashing: {}", e))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 8192];
    loop {
        let n = file
            .read(&mut buffer)
            .map_err(|e| format!("Failed to read for hashing: {}", e))?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// Compute SHA-256 hash of a byte slice, returned as lowercase hex string.
fn hash_bytes(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

// ─── Game Download & Launch ─────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct ManifestEntry {
    path: String,
    hash: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
struct DownloadProgress {
    current: usize,
    total: usize,
    file: String,
}

/// Shared logic to fetch and parse the manifest, with an HTTPS warning.
async fn fetch_manifest(
    app: &AppHandle,
    base_url: &str,
) -> Result<Vec<ManifestEntry>, String> {
    // Warn if build server is not using HTTPS
    if base_url.starts_with("http://") {
        app.emit(
            "log",
            "WARNING: Build server is using HTTP (not HTTPS). Downloads are not encrypted."
                .to_string(),
        )
        .map_err(|e| e.to_string())?;
    }

    let manifest_url = format!("{}/file_manifest.json", base_url);
    let client = reqwest::Client::new();
    let res = client
        .get(&manifest_url)
        .send()
        .await
        .map_err(|e| format!("Failed to fetch manifest: {}", e))?;

    let text = res
        .text()
        .await
        .map_err(|e| format!("Failed to read manifest: {}", e))?;

    let entries: Vec<ManifestEntry> =
        serde_json::from_str(&text).map_err(|e| format!("Failed to parse manifest: {}", e))?;

    // Validate all paths before returning
    for entry in &entries {
        validate_manifest_path(&entry.path)?;
    }

    Ok(entries)
}

#[tauri::command]
pub async fn check_updates(app: AppHandle) -> Result<String, String> {
    let store = get_store(&app);
    let base_url = store
        .get("build_server_url")
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| AppSettings::default().build_server_url);

    let install_path = store
        .get("install_path")
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| AppSettings::default().install_path);

    let entries = fetch_manifest(&app, &base_url).await?;
    let install_dir = PathBuf::from(&install_path);

    // Compare local files against manifest to count what needs updating
    let mut needs_download = 0;
    let mut up_to_date = 0;

    for entry in &entries {
        let file_path = install_dir.join(&entry.path);
        if file_path.exists() {
            if let Some(expected_hash) = &entry.hash {
                match hash_file(&file_path) {
                    Ok(local_hash) if local_hash == *expected_hash => {
                        up_to_date += 1;
                    }
                    _ => {
                        needs_download += 1;
                    }
                }
            } else {
                // No hash in manifest — can't verify, assume up to date
                up_to_date += 1;
            }
        } else {
            needs_download += 1;
        }
    }

    if needs_download == 0 {
        Ok(format!(
            "All {} files are up to date!",
            entries.len()
        ))
    } else {
        Ok(format!(
            "{} files need updating ({} already up to date)",
            needs_download, up_to_date
        ))
    }
}

#[tauri::command]
pub async fn download_game(app: AppHandle) -> Result<(), String> {
    let store = get_store(&app);
    let base_url = store
        .get("build_server_url")
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| AppSettings::default().build_server_url);

    let install_path = store
        .get("install_path")
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| AppSettings::default().install_path);

    app.emit("log", "Fetching file manifest...".to_string())
        .map_err(|e| e.to_string())?;

    let entries = fetch_manifest(&app, &base_url).await?;
    let install_dir = PathBuf::from(&install_path);
    std::fs::create_dir_all(&install_dir)
        .map_err(|e| format!("Failed to create install directory: {}", e))?;

    // Determine which files actually need downloading (incremental update)
    let mut to_download: Vec<&ManifestEntry> = Vec::new();
    let mut skipped = 0;

    for entry in &entries {
        let file_path = install_dir.join(&entry.path);
        if file_path.exists() {
            if let Some(expected_hash) = &entry.hash {
                match hash_file(&file_path) {
                    Ok(local_hash) if local_hash == *expected_hash => {
                        skipped += 1;
                        continue;
                    }
                    _ => {}
                }
            } else {
                // No hash available — skip existing file (can't determine if changed)
                skipped += 1;
                continue;
            }
        }
        to_download.push(entry);
    }

    if to_download.is_empty() {
        app.emit("log", "All files are up to date!".to_string())
            .map_err(|e| e.to_string())?;
        return Ok(());
    }

    app.emit(
        "log",
        format!(
            "Downloading {} files ({} already up to date)",
            to_download.len(),
            skipped
        ),
    )
    .map_err(|e| e.to_string())?;

    let client = reqwest::Client::new();
    let total = to_download.len();

    for (i, entry) in to_download.iter().enumerate() {
        let file_url = format!("{}/{}", base_url, entry.path);
        let file_path = install_dir.join(&entry.path);

        // Create parent directories
        if let Some(parent) = file_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create directory: {}", e))?;
        }

        app.emit(
            "download-progress",
            DownloadProgress {
                current: i + 1,
                total,
                file: entry.path.clone(),
            },
        )
        .map_err(|e| e.to_string())?;

        app.emit("log", format!("Downloading: {}", entry.path))
            .map_err(|e| e.to_string())?;

        let response = client
            .get(&file_url)
            .send()
            .await
            .map_err(|e| format!("Failed to download {}: {}", entry.path, e))?;

        if !response.status().is_success() {
            return Err(format!(
                "Server returned {} for {}",
                response.status(),
                entry.path
            ));
        }

        let bytes = response
            .bytes()
            .await
            .map_err(|e| format!("Failed to read {}: {}", entry.path, e))?;

        // Verify hash of downloaded data before writing to disk
        if let Some(expected_hash) = &entry.hash {
            let actual_hash = hash_bytes(&bytes);
            if actual_hash != *expected_hash {
                return Err(format!(
                    "Hash mismatch for {}: expected {}, got {}. Download may be corrupted or tampered with.",
                    entry.path, expected_hash, actual_hash
                ));
            }
        }

        std::fs::write(&file_path, &bytes)
            .map_err(|e| format!("Failed to write {}: {}", entry.path, e))?;
    }

    app.emit("log", "Download complete!".to_string())
        .map_err(|e| e.to_string())?;
    app.emit(
        "download-progress",
        DownloadProgress {
            current: total,
            total,
            file: "Complete".to_string(),
        },
    )
    .map_err(|e| e.to_string())?;

    Ok(())
}

#[tauri::command]
pub async fn launch_game(app: AppHandle) -> Result<(), String> {
    let store = get_store(&app);
    let install_path = store
        .get("install_path")
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| AppSettings::default().install_path);

    let install_dir = PathBuf::from(&install_path);

    let exe_path = if cfg!(target_os = "macos") {
        let app_path = install_dir.join("Siege Worlds.app");
        if app_path.exists() {
            app_path
        } else {
            install_dir.join("Siege Worlds")
        }
    } else {
        install_dir.join("Siege Worlds.exe")
    };

    if !exe_path.exists() {
        return Err(format!(
            "Game not found at {}. Please download it first.",
            exe_path.display()
        ));
    }

    app.emit("log", format!("Launching: {}", exe_path.display()))
        .map_err(|e| e.to_string())?;

    if cfg!(target_os = "macos") {
        Command::new("open")
            .arg(&exe_path)
            .spawn()
            .map_err(|e| format!("Failed to launch game: {}", e))?;
    } else {
        Command::new(&exe_path)
            .spawn()
            .map_err(|e| format!("Failed to launch game: {}", e))?;
    }

    Ok(())
}

// ─── SSO Authentication ─────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct SSOUser {
    pub id: String,
    pub email: String,
    pub username: String,
    pub display_name: String,
    pub role: String,
    pub avatar_url: Option<String>,
    pub avatar_outer_color: String,
    pub avatar_inner_color: String,
    pub avatar_pan_x: f64,
    pub avatar_pan_y: f64,
    pub avatar_zoom: f64,
    pub created_at: String,
    pub last_sign_in: Option<String>,
}

#[derive(Debug, Deserialize)]
struct VerifyResponse {
    valid: bool,
    user: Option<SSOUser>,
    error: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
pub struct AuthState {
    pub logged_in: bool,
    pub user: Option<SSOUser>,
}

/// SSO login timeout: 2 minutes for the user to complete browser login.
const SSO_TIMEOUT: Duration = Duration::from_secs(120);

#[tauri::command]
pub async fn start_sso_login(app: AppHandle) -> Result<AuthState, String> {
    let store = get_store(&app);
    let sso_url = store
        .get("sso_url")
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| AppSettings::default().sso_url);

    // Start a localhost callback server
    let listener =
        TcpListener::bind("127.0.0.1:0").map_err(|e| format!("Failed to bind: {}", e))?;
    let port = listener
        .local_addr()
        .map_err(|e| format!("Failed to get port: {}", e))?
        .port();

    let login_url = format!(
        "{}/login?app=siegeworlds&redirect=http://localhost:{}/callback",
        sso_url, port
    );

    // Open browser
    app.emit("log", "Opening browser for sign in...".to_string())
        .map_err(|e| e.to_string())?;

    open::that(&login_url).map_err(|e| format!("Failed to open browser: {}", e))?;

    // Wait for the callback in a blocking thread, WITH a timeout
    let token_result: Result<(String, String), String> =
        tokio::task::spawn_blocking(move || {
            // Set timeout so the listener doesn't block forever
            listener
                .set_nonblocking(false)
                .map_err(|e| format!("Failed to set blocking: {}", e))?;
            listener
                .set_ttl(120)
                .ok(); // best-effort

            // Use SO_RCVTIMEO equivalent via accept timeout
            // We'll set a read timeout on each accepted stream instead,
            // and use the listener's non-blocking + polling approach
            listener
                .set_nonblocking(true)
                .map_err(|e| format!("Failed to set non-blocking: {}", e))?;

            let callback_html = r#"<!DOCTYPE html>
<html>
<head><title>Signing in...</title></head>
<body style="background:#1a112e;color:#fff;font-family:sans-serif;display:flex;align-items:center;justify-content:center;height:100vh;margin:0">
<div style="text-align:center"><h2>Signing in...</h2></div>
<script>
  var hash = window.location.hash.substring(1);
  var params = new URLSearchParams(hash);
  var token = params.get('access_token');
  if (token) {
    fetch('/receive-token?token=' + encodeURIComponent(token) + '&refresh=' + encodeURIComponent(params.get('refresh_token') || ''))
      .then(function() {
        document.querySelector('div').innerHTML = '<h2>Signed in!</h2><p>You can close this tab and return to the launcher.</p>';
      });
  } else {
    document.querySelector('div').innerHTML = '<h2>Login failed</h2><p>No token received. Please try again.</p>';
  }
</script>
</body>
</html>"#;

            let mut access_token = String::new();
            let mut refresh_token = String::new();
            let deadline = std::time::Instant::now() + SSO_TIMEOUT;

            // Handle up to 2 requests: first the callback page, then the token relay
            let mut requests_handled = 0;
            while requests_handled < 2 {
                if std::time::Instant::now() > deadline {
                    return Err("Sign in timed out (2 minutes). Please try again.".to_string());
                }

                let stream = match listener.accept() {
                    Ok((stream, _)) => stream,
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(100));
                        continue;
                    }
                    Err(e) => return Err(format!("Failed to accept: {}", e)),
                };

                // Set read timeout on the accepted connection
                let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));

                let mut stream = stream;
                let mut buf = [0u8; 8192];
                let n = stream.read(&mut buf).unwrap_or(0);
                if n == 0 {
                    continue;
                }
                let request = String::from_utf8_lossy(&buf[..n]).to_string();
                requests_handled += 1;

                if request.contains("/receive-token") {
                    // Extract token from query params
                    if let Some(query_start) = request.find("/receive-token?") {
                        let query = &request[query_start + 15..];
                        let query = query.split_whitespace().next().unwrap_or("");
                        for param in query.split('&') {
                            let mut kv = param.splitn(2, '=');
                            let key = kv.next().unwrap_or("");
                            let val = kv.next().unwrap_or("");
                            match key {
                                "token" => {
                                    access_token =
                                        urlencoding::decode(val).unwrap_or_default().to_string()
                                }
                                "refresh" => {
                                    refresh_token =
                                        urlencoding::decode(val).unwrap_or_default().to_string()
                                }
                                _ => {}
                            }
                        }
                    }
                    // No wildcard CORS — only the page we served should call this
                    let response = "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n\r\nOK";
                    let _ = stream.write_all(response.as_bytes());
                    break;
                } else {
                    // Serve the callback HTML page
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\n\r\n{}",
                        callback_html.len(),
                        callback_html
                    );
                    let _ = stream.write_all(response.as_bytes());
                }
            }

            if access_token.is_empty() {
                return Err("No token received from SSO".to_string());
            }

            Ok((access_token, refresh_token))
        })
        .await
        .map_err(|e| format!("Task failed: {}", e))?;

    let (access_token, refresh_token) = token_result?;

    // Verify the token with the SSO server
    let user = verify_token_internal(&sso_url, &access_token).await?;

    // Store tokens
    let store = get_store(&app);
    store.set("access_token", serde_json::json!(&access_token));
    store.set("refresh_token", serde_json::json!(&refresh_token));
    let _ = store.save();

    app.emit(
        "log",
        format!("Signed in as {}", user.display_name),
    )
    .map_err(|e| e.to_string())?;

    Ok(AuthState {
        logged_in: true,
        user: Some(user),
    })
}

async fn verify_token_internal(sso_url: &str, token: &str) -> Result<SSOUser, String> {
    let client = reqwest::Client::new();
    let res = client
        .post(format!("{}/api/verify", sso_url))
        .json(&serde_json::json!({ "token": token }))
        .send()
        .await
        .map_err(|e| format!("Network error: {}", e))?;

    if res.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Err("Invalid or expired token".to_string());
    }

    let body: VerifyResponse = res
        .json()
        .await
        .map_err(|e| format!("Parse error: {}", e))?;

    if body.valid {
        body.user.ok_or_else(|| "No user data".to_string())
    } else {
        Err(body
            .error
            .unwrap_or_else(|| "Verification failed".to_string()))
    }
}

#[tauri::command]
pub async fn verify_token(app: AppHandle) -> Result<AuthState, String> {
    let store = get_store(&app);
    let sso_url = store
        .get("sso_url")
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| AppSettings::default().sso_url);

    let token = store
        .get("access_token")
        .and_then(|v| v.as_str().map(|s| s.to_string()));

    match token {
        Some(t) if !t.is_empty() => match verify_token_internal(&sso_url, &t).await {
            Ok(user) => Ok(AuthState {
                logged_in: true,
                user: Some(user),
            }),
            Err(_) => {
                // Token expired, clear it
                store.delete("access_token");
                store.delete("refresh_token");
                let _ = store.save();
                Ok(AuthState {
                    logged_in: false,
                    user: None,
                })
            }
        },
        _ => Ok(AuthState {
            logged_in: false,
            user: None,
        }),
    }
}

#[tauri::command]
pub async fn get_stored_auth(app: AppHandle) -> AuthState {
    match verify_token(app).await {
        Ok(state) => state,
        Err(_) => AuthState {
            logged_in: false,
            user: None,
        },
    }
}

#[tauri::command]
pub fn logout(app: AppHandle) -> Result<(), String> {
    let store = get_store(&app);
    store.delete("access_token");
    store.delete("refresh_token");
    store.save().map_err(|e| format!("Failed to save: {}", e))
}
