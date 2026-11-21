#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
//! Bus Manager installer: install / update / repair / uninstall.
//! Downloads the game from GitHub Releases (set the repo with BUSMANAGER_REPO at build time).

use std::{
    fs,
    io::{Cursor, Read},
    path::{Path, PathBuf},
    process::Command,
    sync::mpsc::{channel, Receiver, Sender},
    time::Duration,
};

use eframe::egui;

const REPO: &str = match option_env!("BUSMANAGER_REPO") {
    Some(r) => r,
    None => "YOUR_GITHUB_USER/busmanager",
};

const APP_NAME: &str = "Bus Manager";

#[cfg(windows)]
const EXE: &str = "busmanager.exe";
#[cfg(not(windows))]
const EXE: &str = "busmanager";

#[cfg(windows)]
const INSTALLER_EXE: &str = "BusManager-Setup.exe";
#[cfg(not(windows))]
const INSTALLER_EXE: &str = "BusManager-Setup";

fn os_name() -> &'static str {
    if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    }
}

fn asset_name() -> String {
    let ext = if cfg!(windows) { "zip" } else { "tar.gz" };
    format!("busmanager-{}.{ext}", os_name())
}

// --- Paths ------------------------------------------------------------------

fn base_dir() -> PathBuf {
    dirs::data_local_dir().unwrap_or_else(|| PathBuf::from(".")).join("BusManager")
}

fn app_dir() -> PathBuf {
    base_dir().join("app")
}

/// Where the game stores downloaded cities (same folder the game uses).
fn user_data_dir() -> PathBuf {
    dirs::data_dir().unwrap_or_else(|| PathBuf::from(".")).join("BusManager")
}

fn game_exe() -> PathBuf {
    app_dir().join(EXE)
}

fn version_file() -> PathBuf {
    app_dir().join("version.txt")
}

#[derive(Clone, PartialEq)]
enum Installed {
    No,
    Yes(String),
    Broken, // files partly missing: repair fixes it
}

fn installed_state() -> Installed {
    let ver = fs::read_to_string(version_file()).ok().map(|s| s.trim().to_string());
    match (game_exe().exists(), ver) {
        (true, Some(v)) => Installed::Yes(v),
        (true, None) => Installed::Yes("unknown".into()),
        (false, Some(_)) => Installed::Broken,
        (false, None) => Installed::No,
    }
}

fn parse_ver(v: &str) -> Vec<u32> {
    v.trim_start_matches('v').split('.').map(|p| p.parse().unwrap_or(0)).collect()
}

fn is_newer(latest: &str, current: &str) -> bool {
    parse_ver(latest) > parse_ver(current)
}

// --- Network ----------------------------------------------------------------

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .user_agent("busmanager-installer")
        .timeout(Duration::from_secs(300))
        .build()
}

struct Release {
    version: String,
    url: String,
}

fn latest_release() -> Result<Release, String> {
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let v: serde_json::Value = agent()
        .get(&url)
        .set("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| format!("could not reach GitHub: {e}"))?
        .into_json()
        .map_err(|e| e.to_string())?;
    let tag = v["tag_name"].as_str().ok_or("release has no version tag")?;
    let want = asset_name();
    let url = v["assets"]
        .as_array()
        .and_then(|a| a.iter().find(|x| x["name"].as_str() == Some(want.as_str())))
        .and_then(|x| x["browser_download_url"].as_str())
        .ok_or_else(|| format!("release {tag} has no download for this system ({want})"))?;
    Ok(Release { version: tag.trim_start_matches('v').to_string(), url: url.to_string() })
}

fn download(url: &str) -> Result<Vec<u8>, String> {
    let mut buf = Vec::new();
    agent()
        .get(url)
        .call()
        .map_err(|e| format!("download failed: {e}"))?
        .into_reader()
        .read_to_end(&mut buf)
        .map_err(|e| e.to_string())?;
    Ok(buf)
}

fn extract_game(archive: &[u8]) -> Result<Vec<u8>, String> {
    if cfg!(windows) {
        let mut zip = zip::ZipArchive::new(Cursor::new(archive)).map_err(|e| e.to_string())?;
        for i in 0..zip.len() {
            let mut f = zip.by_index(i).map_err(|e| e.to_string())?;
            let is_game = Path::new(f.name()).file_name().map(|n| n == EXE).unwrap_or(false);
            if is_game {
                let mut out = Vec::new();
                f.read_to_end(&mut out).map_err(|e| e.to_string())?;
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
                .and_then(|p| p.file_name().map(|n| n == EXE))
                .unwrap_or(false);
            if is_game {
                let mut out = Vec::new();
                entry.read_to_end(&mut out).map_err(|e| e.to_string())?;
                return Ok(out);
            }
        }
    }
    Err("the download did not contain the game".into())
}

// --- Shortcuts / OS integration ---------------------------------------------

#[cfg(windows)]
fn run_hidden(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let _ = cmd.status();
}

#[cfg(windows)]
fn shortcut_paths() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(d) = dirs::data_dir() {
        v.push(d.join("Microsoft/Windows/Start Menu/Programs").join(format!("{APP_NAME}.lnk")));
    }
    if let Some(d) = dirs::desktop_dir() {
        v.push(d.join(format!("{APP_NAME}.lnk")));
    }
    v
}

#[cfg(windows)]
const UNINSTALL_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\BusManager";

#[cfg(windows)]
fn create_shortcuts(version: &str) {
    for lnk in shortcut_paths() {
        let script = format!(
            "$s=(New-Object -ComObject WScript.Shell).CreateShortcut('{}');$s.TargetPath='{}';$s.WorkingDirectory='{}';$s.Save()",
            lnk.display(),
            game_exe().display(),
            app_dir().display()
        );
        run_hidden(Command::new("powershell").args(["-NoProfile", "-Command", &script]));
    }
    let setup = app_dir().join(INSTALLER_EXE);
    let entries: [(&str, String); 5] = [
        ("DisplayName", APP_NAME.to_string()),
        ("DisplayVersion", version.to_string()),
        ("Publisher", "Bus Manager".to_string()),
        ("InstallLocation", app_dir().display().to_string()),
        ("UninstallString", format!("\"{}\"", setup.display())),
    ];
    for (name, value) in entries {
        run_hidden(Command::new("reg").args(["add", UNINSTALL_KEY, "/v", name, "/d", &value, "/f"]));
    }
}

#[cfg(windows)]
fn remove_shortcuts() {
    for lnk in shortcut_paths() {
        let _ = fs::remove_file(lnk);
    }
    run_hidden(Command::new("reg").args(["delete", UNINSTALL_KEY, "/f"]));
}

#[cfg(target_os = "macos")]
fn mac_app() -> PathBuf {
    dirs::home_dir().unwrap_or_default().join("Applications").join(format!("{APP_NAME}.app"))
}

#[cfg(target_os = "macos")]
fn create_shortcuts(version: &str) {
    use std::os::unix::fs::PermissionsExt;
    let app = mac_app();
    let macos = app.join("Contents/MacOS");
    if fs::create_dir_all(&macos).is_err() {
        return;
    }
    let launcher = macos.join("BusManager");
    let _ = fs::write(&launcher, format!("#!/bin/sh\ncd \"{}\"\nexec \"{}\"\n", app_dir().display(), game_exe().display()));
    let _ = fs::set_permissions(&launcher, fs::Permissions::from_mode(0o755));
    let plist = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\"><dict>\n<key>CFBundleName</key><string>{APP_NAME}</string>\n<key>CFBundleExecutable</key><string>BusManager</string>\n<key>CFBundleIdentifier</key><string>com.busmanager.game</string>\n<key>CFBundleVersion</key><string>{version}</string>\n<key>CFBundlePackageType</key><string>APPL</string>\n</dict></plist>\n"
    );
    let _ = fs::write(app.join("Contents/Info.plist"), plist);
}

#[cfg(target_os = "macos")]
fn remove_shortcuts() {
    let _ = fs::remove_dir_all(mac_app());
}

#[cfg(all(unix, not(target_os = "macos")))]
fn desktop_file() -> PathBuf {
    dirs::data_dir().unwrap_or_default().join("applications").join("busmanager.desktop")
}

#[cfg(all(unix, not(target_os = "macos")))]
fn create_shortcuts(_version: &str) {
    let path = desktop_file();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let entry = format!(
        "[Desktop Entry]\nType=Application\nName={APP_NAME}\nComment=Build a bus network on real city maps\nExec=\"{}\"\nPath={}\nTerminal=false\nCategories=Game;Simulation;\n",
        game_exe().display(),
        app_dir().display()
    );
    let _ = fs::write(path, entry);
}

#[cfg(all(unix, not(target_os = "macos")))]
fn remove_shortcuts() {
    let _ = fs::remove_file(desktop_file());
}

// --- Jobs -------------------------------------------------------------------

#[derive(Clone, Copy)]
enum Job {
    Install,
    Update,
    Repair,
    Uninstall { purge_data: bool },
}

enum Msg {
    Log(String),
    Latest(Result<String, String>),
    Done(Result<String, String>),
}

struct Reporter {
    tx: Sender<Msg>,
    ctx: egui::Context,
}

impl Reporter {
    fn log(&self, s: impl Into<String>) {
        let _ = self.tx.send(Msg::Log(s.into()));
        self.ctx.request_repaint();
    }
    fn send(&self, m: Msg) {
        let _ = self.tx.send(m);
        self.ctx.request_repaint();
    }
}

fn install_files(r: &Reporter, label: &str) -> Result<String, String> {
    r.log("Looking up the latest version...");
    let rel = latest_release()?;
    r.log(format!("Downloading Bus Manager {} ...", rel.version));
    let archive = download(&rel.url)?;
    r.log("Unpacking...");
    let exe_bytes = extract_game(&archive)?;

    fs::create_dir_all(app_dir()).map_err(|e| format!("cannot create install folder: {e}"))?;
    let tmp = app_dir().join(format!("{EXE}.new"));
    fs::write(&tmp, exe_bytes).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&tmp, fs::Permissions::from_mode(0o755));
    }
    fs::rename(&tmp, game_exe()).map_err(|e| format!("could not replace the game (is it still running?): {e}"))?;
    fs::write(version_file(), &rel.version).map_err(|e| e.to_string())?;

    // Keep a copy of this installer next to the game so it can be used to repair/uninstall later.
    if let Ok(me) = std::env::current_exe() {
        let target = app_dir().join(INSTALLER_EXE);
        if me != target {
            let _ = fs::copy(&me, &target);
        }
    }

    r.log("Creating shortcuts...");
    create_shortcuts(&rel.version);
    Ok(format!("{label} complete: Bus Manager {}", rel.version))
}

fn uninstall(r: &Reporter, purge_data: bool) -> Result<String, String> {
    r.log("Removing shortcuts...");
    remove_shortcuts();

    r.log("Removing game files...");
    let dir = app_dir();
    let me = std::env::current_exe().ok();
    if let Ok(rd) = fs::read_dir(&dir) {
        for entry in rd.flatten() {
            let p = entry.path();
            if Some(&p) == me.as_ref() {
                continue; // a running program cannot delete itself on Windows
            }
            let _ = if p.is_dir() { fs::remove_dir_all(&p) } else { fs::remove_file(&p) };
        }
    }
    let running_from_app = me.as_ref().map(|m| m.starts_with(&dir)).unwrap_or(false);
    if running_from_app {
        #[cfg(windows)]
        {
            // Delete the rest (this installer + folder) a moment after we exit.
            let cmd = format!("ping -n 3 127.0.0.1 >nul & rmdir /s /q \"{}\"", dir.display());
            run_detached_windows(&cmd);
        }
        #[cfg(not(windows))]
        {
            let _ = fs::remove_dir_all(&dir);
        }
    } else {
        let _ = fs::remove_dir_all(&dir);
    }

    if purge_data {
        r.log("Deleting downloaded cities...");
        let _ = fs::remove_dir_all(user_data_dir());
    }
    let _ = fs::remove_dir(base_dir());
    Ok("Bus Manager was uninstalled".into())
}

#[cfg(windows)]
fn run_detached_windows(cmd: &str) {
    use std::os::windows::process::CommandExt;
    // CREATE_NO_WINDOW | DETACHED_PROCESS
    let _ = Command::new("cmd").args(["/C", cmd]).creation_flags(0x0800_0000 | 0x0000_0008).spawn();
}

fn run_job(job: Job, r: Reporter) {
    let result = match job {
        Job::Install => install_files(&r, "Installation"),
        Job::Update => install_files(&r, "Update"),
        Job::Repair => install_files(&r, "Repair"),
        Job::Uninstall { purge_data } => uninstall(&r, purge_data),
    };
    r.send(Msg::Done(result));
}

// --- GUI --------------------------------------------------------------------

struct App {
    rx: Receiver<Msg>,
    tx: Sender<Msg>,
    installed: Installed,
    latest: Option<Result<String, String>>,
    busy: bool,
    log: Vec<String>,
    purge: bool,
    confirm_uninstall: bool,
    outcome: Option<Result<String, String>>,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let (tx, rx) = channel();
        let app = App {
            rx,
            tx,
            installed: installed_state(),
            latest: None,
            busy: false,
            log: Vec::new(),
            purge: false,
            confirm_uninstall: false,
            outcome: None,
        };
        app.check_latest(cc.egui_ctx.clone());
        app
    }

    fn check_latest(&self, ctx: egui::Context) {
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let res = latest_release().map(|r| r.version);
            let _ = tx.send(Msg::Latest(res));
            ctx.request_repaint();
        });
    }

    fn start(&mut self, ctx: &egui::Context, job: Job) {
        self.busy = true;
        self.outcome = None;
        self.confirm_uninstall = false;
        self.log.clear();
        let reporter = Reporter { tx: self.tx.clone(), ctx: ctx.clone() };
        std::thread::spawn(move || run_job(job, reporter));
    }

    fn launch(&self) {
        let _ = Command::new(game_exe()).current_dir(app_dir()).spawn();
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Log(s) => self.log.push(s),
                Msg::Latest(r) => self.latest = Some(r),
                Msg::Done(r) => {
                    self.busy = false;
                    self.installed = installed_state();
                    if let Ok(text) = &r {
                        self.log.push(text.clone());
                    }
                    self.outcome = Some(r);
                    self.check_latest(ctx.clone());
                }
            }
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(6.0);
            ui.heading(format!("{APP_NAME} Setup"));
            ui.label("Install, update, repair or remove the game.");
            ui.add_space(10.0);

            let installed_text = match &self.installed {
                Installed::No => "Not installed".to_string(),
                Installed::Yes(v) => format!("Installed: version {v}"),
                Installed::Broken => "Installation is damaged, press Repair".to_string(),
            };
            let latest_text = match &self.latest {
                None => "Latest version: checking...".to_string(),
                Some(Ok(v)) => format!("Latest version: {v}"),
                Some(Err(e)) => format!("Could not check for updates: {e}"),
            };
            ui.label(installed_text);
            ui.label(latest_text);
            ui.add_space(12.0);

            let latest_ok = matches!(self.latest, Some(Ok(_)));
            let update_available = match (&self.installed, &self.latest) {
                (Installed::Yes(cur), Some(Ok(l))) => is_newer(l, cur),
                _ => false,
            };
            let enabled = !self.busy;

            ui.horizontal(|ui| match self.installed.clone() {
                Installed::No => {
                    if ui.add_enabled(enabled && latest_ok, egui::Button::new("  Install  ")).clicked() {
                        self.start(ctx, Job::Install);
                    }
                }
                Installed::Broken => {
                    if ui.add_enabled(enabled && latest_ok, egui::Button::new("  Repair  ")).clicked() {
                        self.start(ctx, Job::Repair);
                    }
                    if ui.add_enabled(enabled, egui::Button::new("  Uninstall  ")).clicked() {
                        self.confirm_uninstall = true;
                    }
                }
                Installed::Yes(_) => {
                    if ui.add_enabled(enabled, egui::Button::new("  Play  ")).clicked() {
                        self.launch();
                    }
                    if ui.add_enabled(enabled && update_available, egui::Button::new("  Update  ")).clicked() {
                        self.start(ctx, Job::Update);
                    }
                    if ui.add_enabled(enabled && latest_ok, egui::Button::new("  Repair  ")).clicked() {
                        self.start(ctx, Job::Repair);
                    }
                    if ui.add_enabled(enabled, egui::Button::new("  Uninstall  ")).clicked() {
                        self.confirm_uninstall = true;
                    }
                }
            });

            if self.confirm_uninstall && !self.busy {
                ui.add_space(10.0);
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.label("Remove Bus Manager from this computer?");
                    ui.checkbox(&mut self.purge, "Also delete downloaded cities");
                    ui.horizontal(|ui| {
                        if ui.button("Yes, uninstall").clicked() {
                            let purge = self.purge;
                            self.start(ctx, Job::Uninstall { purge_data: purge });
                        }
                        if ui.button("Cancel").clicked() {
                            self.confirm_uninstall = false;
                        }
                    });
                });
            }

            ui.add_space(12.0);
            if self.busy {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Working...");
                });
            }
            match &self.outcome {
                Some(Ok(_)) => {
                    ui.colored_label(egui::Color32::from_rgb(0x2e, 0x7d, 0x32), "Done.");
                }
                Some(Err(e)) => {
                    ui.colored_label(egui::Color32::from_rgb(0xc6, 0x28, 0x28), format!("Failed: {e}"));
                }
                None => {}
            }
            ui.add_space(6.0);
            egui::ScrollArea::vertical().stick_to_bottom(true).max_height(160.0).show(ui, |ui| {
                for line in &self.log {
                    ui.label(egui::RichText::new(line).small().weak());
                }
            });
        });
    }
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([480.0, 420.0]).with_resizable(false),
        ..Default::default()
    };
    eframe::run_native(
        &format!("{APP_NAME} Setup"),
        options,
        Box::new(|cc| Ok(Box::new(App::new(cc)))),
    )
}
