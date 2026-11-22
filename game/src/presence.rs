//! Discord Rich Presence: shows the company and what the player is doing in their Discord profile.
//!
//! A background thread owns the connection to the local Discord client. If Discord is not running
//! (or anything fails) the thread just retries later; the game never notices.
//!
//! Art assets to upload in the Discord developer portal (Rich Presence -> Art Assets):
//!   `logo`  large image (the game icon, 1024x1024)
//!   `bus`   small image (a bus symbol)

use std::{
    sync::mpsc::{channel, RecvTimeoutError, Sender},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use bevy::prelude::*;
use discord_rich_presence::{
    activity::{Activity, Assets, Button, Timestamps},
    DiscordIpc, DiscordIpcClient,
};

use crate::{
    company::Company, fleet::Fleet, ops::GameClock, save::Session, settings::Settings, AppState,
};

pub const CLIENT_ID: &str = "1099116781437735033";

#[derive(Clone, PartialEq)]
struct Status {
    details: String,
    state: String,
}

enum Msg {
    Set(Option<Status>),
}

#[derive(Resource)]
struct Presence {
    tx: Sender<Msg>,
    last: Option<Option<Status>>,
}

pub struct PresencePlugin;

impl Plugin for PresencePlugin {
    fn build(&self, app: &mut App) {
        let (tx, rx) = channel::<Msg>();
        let started = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        std::thread::spawn(move || {
            let mut client: Option<DiscordIpcClient> = None;
            let mut wanted: Option<Status> = None;
            let mut dirty = false;
            loop {
                match rx.recv_timeout(Duration::from_secs(15)) {
                    Ok(Msg::Set(s)) => {
                        wanted = s;
                        dirty = true;
                    }
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => break,
                }
                match &wanted {
                    None => {
                        if let Some(mut c) = client.take() {
                            let _ = c.close();
                        }
                        dirty = false;
                    }
                    Some(st) => {
                        if client.is_none() {
                            let mut c = DiscordIpcClient::new(CLIENT_ID);
                            if c.connect().is_ok() {
                                client = Some(c);
                                dirty = true;
                            }
                        }
                        if dirty {
                            if let Some(c) = client.as_mut() {
                                let act = Activity::new()
                                    .details(&st.details)
                                    .state(&st.state)
                                    .timestamps(Timestamps::new().start(started))
                                    .assets(
                                        Assets::new()
                                            .large_image("logo")
                                            .large_text("Bus Manager")
                                            .small_image("bus")
                                            .small_text("Bus Manager"),
                                    )
                                    .buttons(vec![Button::new(
                                        "Join the Discord",
                                        "https://discord.gg/Ppx7u4g2Wt",
                                    )]);
                                if c.set_activity(act).is_ok() {
                                    dirty = false;
                                } else {
                                    client = None; // Discord was closed: reconnect later
                                }
                            }
                        }
                    }
                }
            }
        });
        app.insert_resource(Presence { tx, last: None })
            .add_systems(Update, update_presence);
    }
}

fn update_presence(
    mut p: ResMut<Presence>,
    settings: Res<Settings>,
    state: Res<State<AppState>>,
    company: Res<Company>,
    session: Option<Res<Session>>,
    fleet: Res<Fleet>,
    clock: Option<Res<GameClock>>,
    time: Res<Time<Real>>,
    mut next_check: Local<f32>,
) {
    if time.elapsed_secs() < *next_check {
        return;
    }
    *next_check = time.elapsed_secs() + 5.0;

    let status = if !settings.discord_presence {
        None
    } else if *state.get() == AppState::Playing {
        let city = session
            .as_ref()
            .map(|s| {
                s.place
                    .display_name
                    .split(',')
                    .next()
                    .unwrap_or("")
                    .trim()
                    .to_string()
            })
            .unwrap_or_default();
        let day = clock.as_ref().map(|c| c.day() + 1).unwrap_or(1);
        let name = if company.name.trim().is_empty() {
            "a bus company".to_string()
        } else {
            company.name.trim().to_string()
        };
        Some(Status {
            details: if city.is_empty() {
                format!("Running {name}")
            } else {
                format!("Running {name} in {city}")
            },
            state: format!("Day {day} · {} buses", fleet.buses.len()),
        })
    } else {
        Some(Status {
            details: "In the main menu".into(),
            state: "Planning the next bus network".into(),
        })
    };
    if p.last.as_ref() != Some(&status) {
        let _ = p.tx.send(Msg::Set(status.clone()));
        p.last = Some(status);
    }
}
