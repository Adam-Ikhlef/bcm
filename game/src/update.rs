//! Update checker: on every launch the game asks GitHub for the newest release (in the background)
//! and, if there is a newer one, shows its changelog with "Download" and "Remind me later".

use std::{
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
}

#[derive(Resource, Default)]
pub struct Updates {
    inbox: Option<Mutex<Receiver<Option<Release>>>>,
    /// A newer release, once the check has finished.
    pub available: Option<Release>,
    /// "Remind me later" was pressed (or the window was shown): nothing more this session.
    pub dismissed: bool,
    shown: bool,
}

#[derive(Component)]
struct UpdateModal;

pub struct UpdatePlugin;

impl Plugin for UpdatePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Updates>()
            .add_systems(Startup, start_check)
            .add_systems(
                Update,
                (poll_check, show_modal, update_actions)
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
    Some(Release {
        version: best.tag_name.trim_start_matches(['v', 'V']).to_string(),
        notes: plain_notes(best.body.as_deref().unwrap_or("")),
        url: best.html_url,
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
                    Node {
                        column_gap: Val::Px(10.0),
                        justify_content: JustifyContent::FlexEnd,
                        ..default()
                    },
                    children![
                        button(f, "Remind me later", Action::UpdateLater, BtnKind::Normal),
                        button(f, "Download", Action::UpdateDownload, BtnKind::Primary),
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
) {
    for UiAction(a) in events.read() {
        match a {
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

fn close_modal(mut commands: Commands, modal: Query<Entity, With<UpdateModal>>) {
    for e in &modal {
        commands.entity(e).despawn();
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
