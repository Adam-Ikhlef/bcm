//! Update checker: on every launch the game asks GitHub for the newest release (in the background)
//! and, if there is a newer one, shows its changelog with "Download" and "Remind me later".

use std::{
    io::Read,
    sync::{
        mpsc::{channel, Receiver},
        Mutex,
    },
    time::Duration,
};

use bevy::{prelude::*, ui::FocusPolicy};
use serde::Deserialize;

use crate::{
    settings::Settings,
    style,
    ui::{button, text, Action, BtnKind, UiAction, UiFonts},
    AppState,
};

const DEFAULT_URL: &str = "https://api.github.com/repos/YOZTeam/BusManager/releases?per_page=10";

/// Links shown on the main menu (indexes used by `Action::OpenLink`).
pub const LINKS: [&str; 3] = [
    "https://ko-fi.com/adamikhlef",
    "https://github.com/YOZTeam/BusManager",
    "https://discord.gg/Ppx7u4g2Wt",
];

#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    pub version: String,
    pub notes: String,
    pub url: String,
    /// The download for this system, if the release has one.
    pub asset: Option<Asset>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Asset {
    pub url: String,
    /// Hex SHA-256 published by GitHub, when available.
    pub sha256: Option<String>,
    pub version: String,
}

#[derive(Resource, Default)]
pub struct Updates {
    inbox: Option<Mutex<Receiver<Option<Release>>>>,
    /// A newer release, once the check has finished.
    pub available: Option<Release>,
    /// "Remind me later" was pressed (or the window was shown): nothing more this session.
    pub dismissed: bool,
    shown: bool,
    /// Result of the running download/installation.
    installing: Option<Mutex<Receiver<Result<std::path::PathBuf, String>>>>,
}

#[derive(Component)]
struct UpdateModal;
#[derive(Component)]
struct UpdateStatus;
#[derive(Component)]
struct UpdateButtons;

pub struct UpdatePlugin;

impl Plugin for UpdatePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Updates>()
            .add_systems(Startup, (start_check, remove_old_exe))
            .add_systems(
                Update,
                (poll_check, show_modal, update_actions, poll_install)
                    .chain()
                    .run_if(in_state(AppState::Menu)),
            )
            .add_systems(OnExit(AppState::Menu), close_modal);
    }
}

#[derive(Deserialize)]
struct GhRelease {
    tag_name: String,
    #[serde(default)]
    body: Option<String>,
    html_url: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    assets: Vec<GhAsset>,
}

#[derive(Deserialize)]
struct GhAsset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    digest: Option<String>,
}

/// Names of the release files that hold the game for this system, best first:
/// `busmanager-<os>-<arch>.<ext>`, then the name used before there were several architectures.
pub fn asset_names() -> Vec<String> {
    let (os, ext) = if cfg!(windows) {
        ("windows", "zip")
    } else if cfg!(target_os = "macos") {
        ("macos", "tar.gz")
    } else {
        ("linux", "tar.gz")
    };
    let arm = cfg!(target_arch = "aarch64");
    let arch = if arm { "arm64" } else { "x64" };
    let mut names = vec![format!("busmanager-{os}-{arch}.{ext}")];
    // Old releases: Windows and Linux were x64, macOS was Apple silicon.
    if arm == (os == "macos") {
        names.push(format!("busmanager-{os}.{ext}"));
    }
    names
}

/// "v1.2.3-beta" -> [1, 2, 3]
pub fn parse_version(s: &str) -> Vec<u32> {
    s.trim()
        .trim_start_matches(['v', 'V'])
        .split(['-', '+'])
        .next()
        .unwrap_or("")
        .split('.')
        .map(|p| p.parse().unwrap_or(0))
        .collect()
}

pub fn is_newer(candidate: &str, current: &str) -> bool {
    let (mut a, mut b) = (parse_version(candidate), parse_version(current));
    let n = a.len().max(b.len());
    a.resize(n, 0);
    b.resize(n, 0);
    a > b
}

/// Turns the Markdown of a release body into short plain text for the dialog.
pub fn plain_notes(md: &str) -> String {
    let mut out = String::new();
    for line in md.lines() {
        let l = line.trim();
        if l.is_empty() {
            continue;
        }
        let l = l.trim_start_matches('#').trim();
        let l = if let Some(r) = l.strip_prefix("- ").or_else(|| l.strip_prefix("* ")) {
            format!("•  {r}")
        } else {
            l.to_string()
        };
        out.push_str(&l.replace("**", "").replace('`', ""));
        out.push('\n');
        if out.chars().count() > 1400 {
            out.push('…');
            break;
        }
    }
    out.trim().to_string()
}

fn fetch() -> Option<Release> {
    let url = std::env::var("BUSMANAGER_UPDATE_URL").unwrap_or_else(|_| DEFAULT_URL.into());
    let resp = ureq::AgentBuilder::new()
        .user_agent(concat!("busmanager/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(10))
        .build()
        .get(&url)
        .set("Accept", "application/vnd.github+json")
        .call()
        .ok()?;
    let list: Vec<GhRelease> = serde_json::from_reader(resp.into_reader()).ok()?;
    let best = list
        .into_iter()
        .filter(|r| !r.draft)
        .max_by(|a, b| parse_version(&a.tag_name).cmp(&parse_version(&b.tag_name)))?;
    if !is_newer(&best.tag_name, crate::VERSION) {
        return None;
    }
    let wanted = asset_names();
    let found = wanted
        .iter()
        .find_map(|w| best.assets.iter().find(|a| &a.name == w));
    let asset = found.map(|a| Asset {
        version: best.tag_name.trim_start_matches(['v', 'V']).to_string(),
        url: a.browser_download_url.clone(),
        sha256: a
            .digest
            .as_deref()
            .and_then(|d| d.strip_prefix("sha256:"))
            .map(|d| d.to_ascii_lowercase()),
    });
    Some(Release {
        version: best.tag_name.trim_start_matches(['v', 'V']).to_string(),
        notes: plain_notes(best.body.as_deref().unwrap_or("")),
        url: best.html_url,
        asset,
    })
}

fn start_check(mut updates: ResMut<Updates>, settings: Res<Settings>) {
    if !settings.check_updates {
        return;
    }
    let (tx, rx) = channel();
    updates.inbox = Some(Mutex::new(rx));
    // Offline, private repository, rate limit ...: all of it just means "no update shown".
    std::thread::spawn(move || {
        let _ = tx.send(fetch());
    });
}

fn poll_check(mut updates: ResMut<Updates>) {
    let got = updates
        .inbox
        .as_ref()
        .and_then(|rx| rx.lock().ok()?.try_recv().ok());
    if let Some(r) = got {
        updates.available = r;
        updates.inbox = None;
    }
}

fn show_modal(
    mut commands: Commands,
    fonts: Res<UiFonts>,
    mut updates: ResMut<Updates>,
    existing: Query<(), With<UpdateModal>>,
) {
    if updates.shown || updates.dismissed || !existing.is_empty() {
        return;
    }
    let Some(rel) = updates.available.clone() else {
        return;
    };
    updates.shown = true;
    let f = &*fonts;
    commands.spawn((
        UpdateModal,
        Interaction::default(),
        FocusPolicy::Block,
        Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)),
        GlobalZIndex(100),
        children![(
            Node {
                width: Val::Px(560.0),
                max_height: Val::Percent(86.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(12.0),
                padding: UiRect::all(Val::Px(24.0)),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(style::PANEL_SOLID),
            style::panel_shadow(),
            BorderColor(style::ACCENT),
            BorderRadius::all(Val::Px(14.0)),
            children![
                text(f, true, 26.0, style::TEXT, "Update available"),
                text(
                    f,
                    false,
                    14.0,
                    style::MUTED,
                    format!(
                        "Version {} is out. You have {}.",
                        rel.version,
                        crate::VERSION
                    )
                ),
                (UpdateStatus, text(f, false, 14.0, style::ACCENT, "")),
                text(f, true, 13.0, style::ACCENT, "WHAT'S NEW"),
                (
                    Node {
                        padding: UiRect::all(Val::Px(12.0)),
                        overflow: Overflow::clip(),
                        max_height: Val::Px(280.0),
                        ..default()
                    },
                    BackgroundColor(style::INPUT),
                    BorderRadius::all(Val::Px(8.0)),
                    children![text(
                        f,
                        false,
                        14.0,
                        style::TEXT,
                        if rel.notes.is_empty() {
                            "No changelog was provided for this release.".to_string()
                        } else {
                            rel.notes.clone()
                        }
                    )]
                ),
                (
                    UpdateButtons,
                    Node {
                        column_gap: Val::Px(10.0),
                        justify_content: JustifyContent::FlexEnd,
                        ..default()
                    },
                    children![
                        button(f, "Remind me later", Action::UpdateLater, BtnKind::Normal),
                        button(f, "Release page", Action::UpdateDownload, BtnKind::Normal),
                        button(
                            f,
                            if rel.asset.is_some() {
                                "Update now"
                            } else {
                                "Release page"
                            },
                            if rel.asset.is_some() {
                                Action::UpdateInstall
                            } else {
                                Action::UpdateDownload
                            },
                            BtnKind::Primary
                        ),
                    ]
                ),
            ]
        )],
    ));
}

fn update_actions(
    mut commands: Commands,
    mut events: EventReader<UiAction>,
    mut updates: ResMut<Updates>,
    modal: Query<Entity, With<UpdateModal>>,
    mut status: Query<&mut Text, With<UpdateStatus>>,
    mut buttons: Query<&mut Node, With<UpdateButtons>>,
) {
    for UiAction(a) in events.read() {
        match a {
            Action::UpdateInstall => {
                let Some(rel) = updates.available.clone() else {
                    continue;
                };
                let Some(asset) = rel.asset else { continue };
                if updates.installing.is_some() {
                    continue;
                }
                let (tx, rx) = channel();
                updates.installing = Some(Mutex::new(rx));
                std::thread::spawn(move || {
                    let _ = tx.send(install(&asset));
                });
                for mut t in &mut status {
                    t.0 = format!(
                        "Downloading version {} ... the game restarts when done.",
                        rel.version
                    );
                }
                for mut n in &mut buttons {
                    n.display = Display::None;
                }
            }
            Action::UpdateLater | Action::UpdateDownload => {
                if *a == Action::UpdateDownload {
                    if let Some(r) = &updates.available {
                        open_url(&r.url);
                    }
                }
                updates.dismissed = true;
                for e in &modal {
                    commands.entity(e).despawn();
                }
            }
            _ => {}
        }
    }
}

/// Waits for the installation; on success starts the new version and quits this one.
fn poll_install(
    mut updates: ResMut<Updates>,
    mut status: Query<&mut Text, With<UpdateStatus>>,
    mut buttons: Query<&mut Node, With<UpdateButtons>>,
    mut exit: EventWriter<AppExit>,
) {
    let got = updates
        .installing
        .as_ref()
        .and_then(|rx| rx.lock().ok()?.try_recv().ok());
    let Some(res) = got else { return };
    updates.installing = None;
    match res {
        Ok(exe) => {
            for mut t in &mut status {
                t.0 = "Updated. Restarting ...".into();
            }
            // `exe` is the path from before the swap: on Linux the running program's own path
            // now points at the renamed old copy.
            let _ = std::process::Command::new(exe).spawn();
            exit.write(AppExit::Success);
        }
        Err(e) => {
            for mut t in &mut status {
                t.0 = format!("Update failed: {e}");
            }
            for mut n in &mut buttons {
                n.display = Display::Flex;
            }
        }
    }
}

fn close_modal(mut commands: Commands, modal: Query<Entity, With<UpdateModal>>) {
    for e in &modal {
        commands.entity(e).despawn();
    }
}

// --- Installing -----------------------------------------------------------------

/// Only downloads from this project's releases are installed (plus localhost for tests).
fn trusted_asset_url(url: &str) -> bool {
    url.starts_with("https://github.com/YOZTeam/BusManager/releases/download/")
        || url.starts_with("http://127.0.0.1")
}

/// Largest game executable we accept from a download (1 GiB).
const MAX_GAME_BYTES: u64 = 1 << 30;

fn extract_game(archive: &[u8]) -> Result<Vec<u8>, String> {
    let exe_name = if cfg!(windows) {
        "busmanager.exe"
    } else {
        "busmanager"
    };
    let mut out = Vec::new();
    if cfg!(windows) {
        let mut zip =
            zip::ZipArchive::new(std::io::Cursor::new(archive)).map_err(|e| e.to_string())?;
        for i in 0..zip.len() {
            let mut f = zip.by_index(i).map_err(|e| e.to_string())?;
            let is_game = f.name().is_ok_and(|n| {
                std::path::Path::new(n.as_ref())
                    .file_name()
                    .is_some_and(|n| n == exe_name)
            });
            if is_game {
                // Never read more than MAX_GAME_BYTES (a hostile archive cannot exhaust memory).
                std::io::Read::read_to_end(
                    &mut std::io::Read::take(&mut f, MAX_GAME_BYTES),
                    &mut out,
                )
                .map_err(|e| e.to_string())?;
                return Ok(out);
            }
        }
    } else {
        let gz = flate2::read::GzDecoder::new(archive);
        let mut tar = tar::Archive::new(gz);
        for entry in tar.entries().map_err(|e| e.to_string())? {
            let mut entry = entry.map_err(|e| e.to_string())?;
            let is_game = entry
                .path()
                .ok()
                .and_then(|p| p.file_name().map(|n| n == exe_name))
                .unwrap_or(false);
            if is_game {
                std::io::Read::read_to_end(&mut entry, &mut out).map_err(|e| e.to_string())?;
                return Ok(out);
            }
        }
    }
    Err("the download does not contain the game".into())
}

fn sibling(exe: &std::path::Path, suffix: &str) -> std::path::PathBuf {
    let mut name = exe.file_name().unwrap_or_default().to_os_string();
    name.push(suffix);
    exe.with_file_name(name)
}

/// Replaces the program file `exe` with `bytes`. A running program can be renamed (not deleted) on
/// Windows, so the old file is moved aside first and removed on the next start.
fn swap_in(exe: &std::path::Path, bytes: &[u8]) -> Result<(), String> {
    let new = sibling(exe, ".new");
    let old = sibling(exe, ".old");
    std::fs::write(&new, bytes).map_err(|e| format!("cannot write next to the game: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&new, std::fs::Permissions::from_mode(0o755));
    }
    let _ = std::fs::remove_file(&old);
    std::fs::rename(exe, &old).map_err(|e| {
        let _ = std::fs::remove_file(&new);
        format!("cannot replace the game: {e}")
    })?;
    if let Err(e) = std::fs::rename(&new, exe) {
        let _ = std::fs::rename(&old, exe); // put the working version back
        let _ = std::fs::remove_file(&new);
        return Err(format!("cannot replace the game: {e}"));
    }
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Downloads the release, checks it and swaps it in for the running program.
fn install(asset: &Asset) -> Result<std::path::PathBuf, String> {
    use sha2::{Digest, Sha256};
    if !trusted_asset_url(&asset.url) {
        return Err("the download address is not trusted".into());
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut archive = Vec::new();
    ureq::AgentBuilder::new()
        .user_agent(concat!("busmanager/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(600))
        .build()
        .get(&asset.url)
        .call()
        .map_err(|e| format!("download failed: {e}"))?
        .into_reader()
        .take(400 * 1024 * 1024)
        .read_to_end(&mut archive)
        .map_err(|e| format!("download failed: {e}"))?;
    if let Some(want) = &asset.sha256 {
        let got = hex(&Sha256::digest(&archive));
        if &got != want {
            return Err("the download is corrupted (checksum mismatch)".into());
        }
    }
    let game = extract_game(&archive)?;
    if game.len() < 100 * 1024 {
        return Err("the downloaded program looks incomplete".into());
    }
    swap_in(&exe, &game)?;
    // Keep the installer's idea of the installed version in sync.
    if let (Some(dir), Some(rel)) = (exe.parent(), Some(asset.version.as_str())) {
        let vf = dir.join("version.txt");
        if vf.exists() {
            let _ = std::fs::write(vf, rel);
        }
    }
    Ok(exe)
}

/// Removes the previous program file left behind by an update.
fn remove_old_exe() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::fs::remove_file(sibling(&exe, ".old"));
    }
}

/// Opens the release page in the default browser (https GitHub links only).
pub fn open_url(url: &str) {
    let ok = url.starts_with("https://github.com/")
        || url.starts_with("https://ko-fi.com/")
        || url.starts_with("https://discord.gg/")
        || url.starts_with("http://127.0.0.1");
    if !ok
        || url
            .chars()
            .any(|c| c.is_whitespace() || c == '"' || c == '&' || c == '^')
    {
        return;
    }
    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("rundll32")
        .args(["url.dll,FileProtocolHandler", url])
        .spawn();
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(url).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let _ = std::process::Command::new("xdg-open").arg(url).spawn();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swap_replaces_the_program_and_keeps_old_copy() {
        let dir = std::env::temp_dir().join(format!("bm_swap_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("busmanager");
        std::fs::write(&exe, b"old").unwrap();
        swap_in(&exe, b"new version").unwrap();
        assert_eq!(std::fs::read(&exe).unwrap(), b"new version");
        assert_eq!(std::fs::read(sibling(&exe, ".old")).unwrap(), b"old");
        assert!(!sibling(&exe, ".new").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(not(windows))]
    #[test]
    fn extracts_game_from_tar_gz() {
        let mut tar_bytes = Vec::new();
        {
            let mut b = tar::Builder::new(&mut tar_bytes);
            let data = b"program bytes";
            let mut h = tar::Header::new_gnu();
            h.set_size(data.len() as u64);
            h.set_mode(0o755);
            h.set_cksum();
            b.append_data(&mut h, "busmanager", &data[..]).unwrap();
            b.finish().unwrap();
        }
        let mut gz = Vec::new();
        {
            let mut e = flate2::write::GzEncoder::new(&mut gz, flate2::Compression::fast());
            std::io::Write::write_all(&mut e, &tar_bytes).unwrap();
            e.finish().unwrap();
        }
        assert_eq!(extract_game(&gz).unwrap(), b"program bytes");
        assert!(extract_game(b"not an archive").is_err());
    }

    #[test]
    fn only_project_downloads_are_trusted() {
        assert!(trusted_asset_url(
            "https://github.com/YOZTeam/BusManager/releases/download/v1/busmanager-linux.tar.gz"
        ));
        assert!(!trusted_asset_url(
            "https://evil.example/busmanager-linux.tar.gz"
        ));
        assert!(!trusted_asset_url(
            "https://github.com/someone/else/releases/download/v1/x"
        ));
    }

    #[test]
    fn asset_names_say_system_and_architecture() {
        let n = asset_names();
        assert!(
            n[0].starts_with("busmanager-") && (n[0].contains("-x64.") || n[0].contains("-arm64."))
        );
        assert!(n.len() <= 2);
    }

    #[test]
    fn versions() {
        assert!(is_newer("v0.4.0", "0.3.0"));
        assert!(is_newer("1.0", "0.9.9"));
        assert!(!is_newer("0.3.0", "0.3.0"));
        assert!(!is_newer("v0.2.9", "0.3.0"));
        assert!(is_newer("0.3.1-beta", "0.3.0"));
    }

    #[test]
    fn notes() {
        let n = plain_notes("## New\n\n- **Menu** redesign\n- `fix` bug");
        assert_eq!(n, "New\n•  Menu redesign\n•  fix bug");
    }
}
