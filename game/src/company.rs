//! The player's bus company: a name and a logo. Chosen when starting a new game and reused later
//! (customer reviews, social media, contracts ...).

use bevy::{
    asset::RenderAssetUsages,
    image::{CompressedImageFormats, ImageSampler, ImageType},
    input::keyboard::{Key, KeyboardInput},
    prelude::*,
};

use crate::{
    menu::{Menu, Screen},
    settings::Settings,
    style,
    ui::{button, text, Action, BtnKind, UiAction, UiFonts},
    AppState,
};

pub const NAME_MAX: usize = 28;

/// Colours behind the six built-in logo glyphs.
pub const LOGO_COLORS: [Color; 6] = [
    style::CBM_BLUE,
    style::CBM_GREEN,
    style::CBM_ORANGE,
    style::CBM_PURPLE,
    style::CBM_RED,
    style::rgb(0x1f, 0x9e, 0x9e),
];

#[derive(Resource, Clone, Default)]
pub struct Company {
    pub name: String,
    /// "d0" .. "d5" for a built-in logo, otherwise the path of the player's own image.
    pub logo: String,
    pub custom: Option<Handle<Image>>,
}

impl Company {
    pub fn default_logo(&self) -> Option<usize> {
        self.logo
            .strip_prefix('d')
            .and_then(|n| n.parse().ok())
            .filter(|n| *n < LOGO_COLORS.len())
    }
}

#[derive(Resource)]
pub struct LogoIcons(pub Vec<Handle<Image>>);

fn decode(bytes: &[u8], ext: &str) -> Result<Image, String> {
    Image::from_buffer(
        bytes,
        ImageType::Extension(ext),
        CompressedImageFormats::NONE,
        true,
        ImageSampler::linear(),
        RenderAssetUsages::default(),
    )
    .map_err(|e| e.to_string())
}

/// Reads the player's own logo (PNG or JPG, up to 4 MB).
pub fn load_custom(path: &str, images: &mut Assets<Image>) -> Result<Handle<Image>, String> {
    let path = path.trim().trim_matches('"').trim_matches('\'');
    let ext = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    let ext = match ext.as_str() {
        "png" => "png",
        "jpg" | "jpeg" => "jpg",
        _ => return Err("The logo must be a PNG or JPG file".into()),
    };
    let meta = std::fs::metadata(path).map_err(|_| "File not found".to_string())?;
    if meta.len() > 4 * 1024 * 1024 {
        return Err("The image is bigger than 4 MB".into());
    }
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let img = decode(&bytes, ext).map_err(|_| "Could not read that image".to_string())?;
    Ok(images.add(img))
}

/// Draws a logo of `size` pixels into `p`.
pub fn spawn_logo(p: &mut ChildSpawnerCommands, c: &Company, icons: &LogoIcons, size: f32) {
    if let (Some(h), None) = (&c.custom, c.default_logo()) {
        p.spawn((
            ImageNode::new(h.clone()),
            Node {
                width: Val::Px(size),
                height: Val::Px(size),
                ..default()
            },
            BorderRadius::all(Val::Px(size * 0.18)),
        ));
        return;
    }
    let i = c.default_logo().unwrap_or(0);
    p.spawn((
        Node {
            width: Val::Px(size),
            height: Val::Px(size),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        BackgroundColor(LOGO_COLORS[i]),
        BorderRadius::all(Val::Px(size * 0.2)),
        children![(
            ImageNode::new(icons.0[i].clone()).with_color(Color::WHITE),
            Node {
                width: Val::Px(size * 0.66),
                height: Val::Px(size * 0.66),
                ..default()
            },
        )],
    ));
}

#[derive(Default, PartialEq, Clone, Copy)]
enum Focus {
    #[default]
    Name,
    Path,
}

#[derive(Resource, Default)]
struct CompanyForm {
    focus: Focus,
    path: String,
    message: String,
    error: bool,
}

#[derive(Component)]
struct NameText;
#[derive(Component)]
struct PathText;
#[derive(Component)]
struct MessageText;
#[derive(Component)]
struct LogoTile(u8);
#[derive(Component)]
struct CustomTile;

pub struct CompanyPlugin;

impl Plugin for CompanyPlugin {
    fn build(&self, app: &mut App) {
        let s = Settings::load();
        let mut company = Company {
            name: s.company_name.clone(),
            logo: if s.company_logo.is_empty() {
                "d0".into()
            } else {
                s.company_logo.clone()
            },
            custom: None,
        };
        let world = app.world_mut();
        let mut images = world.resource_mut::<Assets<Image>>();
        if company.default_logo().is_none() {
            match load_custom(&company.logo, &mut images) {
                Ok(h) => company.custom = Some(h),
                Err(_) => company.logo = "d0".into(),
            }
        }
        let glyphs: [&[u8]; 6] = [
            include_bytes!("../assets/icons/logo0.png"),
            include_bytes!("../assets/icons/logo1.png"),
            include_bytes!("../assets/icons/logo2.png"),
            include_bytes!("../assets/icons/logo3.png"),
            include_bytes!("../assets/icons/logo4.png"),
            include_bytes!("../assets/icons/logo5.png"),
        ];
        let handles = glyphs
            .iter()
            .map(|b| images.add(decode(b, "png").expect("embedded logo")))
            .collect();
        world.insert_resource(LogoIcons(handles));
        world.insert_resource(company);
        app.init_resource::<CompanyForm>().add_systems(
            Update,
            (company_input, company_actions, company_render)
                .chain()
                .run_if(in_state(AppState::Menu)),
        );
    }
}

fn label(f: &UiFonts, s: &str) -> impl Bundle {
    text(f, true, 12.0, style::MUTED, s.to_string())
}

fn input_box(f: &UiFonts, marker: impl Bundle, action: Action) -> impl Bundle {
    (
        Button,
        action,
        Node {
            flex_grow: 1.0,
            padding: UiRect::axes(Val::Px(14.0), Val::Px(11.0)),
            border: UiRect::all(Val::Px(2.0)),
            ..default()
        },
        BackgroundColor(style::INPUT),
        BorderColor(style::PANEL_BORDER),
        BorderRadius::all(Val::Px(10.0)),
        crate::ui::ButtonColors {
            normal: style::INPUT,
            hover: style::INPUT,
            pressed: style::INPUT,
            border: style::PANEL_BORDER,
            toggle: false,
        },
        children![(marker, text(f, false, 17.0, style::TEXT, ""))],
    )
}

/// Builds the "Your company" screen.
pub fn company_screen(root: &mut ChildSpawnerCommands, f: &UiFonts, icons: &LogoIcons) {
    crate::menu::panel_screen(root, f, "Your company", 660.0, Screen::Main, |p| {
        p.spawn(text(
            f,
            false,
            14.0,
            style::MUTED,
            "Your name and logo will appear in customer reviews, social media, contracts and more.",
        ));
        p.spawn(label(f, "COMPANY NAME"));
        p.spawn(input_box(f, NameText, Action::CompanyField(0)));
        p.spawn(label(f, "LOGO"));
        p.spawn(Node {
            column_gap: Val::Px(10.0),
            ..default()
        })
        .with_children(|row| {
            for i in 0..LOGO_COLORS.len() {
                row.spawn((
                    Button,
                    Action::LogoPick(i as u8),
                    LogoTile(i as u8),
                    Node {
                        padding: UiRect::all(Val::Px(5.0)),
                        border: UiRect::all(Val::Px(3.0)),
                        ..default()
                    },
                    BorderColor(Color::NONE),
                    BorderRadius::all(Val::Px(14.0)),
                    crate::ui::ButtonColors {
                        normal: Color::NONE,
                        hover: style::BTN_HOVER,
                        pressed: style::BTN_PRESS,
                        border: Color::NONE,
                        toggle: false,
                    },
                    BackgroundColor(Color::NONE),
                ))
                .with_children(|t| {
                    let c = Company {
                        name: String::new(),
                        logo: format!("d{i}"),
                        custom: None,
                    };
                    spawn_logo(t, &c, icons, 64.0);
                });
            }
        });
        p.spawn(label(f, "OR USE YOUR OWN IMAGE (PNG or JPG file path)"));
        p.spawn(Node {
            column_gap: Val::Px(10.0),
            align_items: AlignItems::Center,
            ..default()
        })
        .with_children(|row| {
            row.spawn(input_box(f, PathText, Action::CompanyField(1)));
            row.spawn(button(f, "Use image", Action::LogoLoad, BtnKind::Normal));
        });
        p.spawn((MessageText, text(f, false, 13.0, style::MUTED, "")));
        p.spawn((
            Node {
                justify_content: JustifyContent::FlexEnd,
                ..default()
            },
            children![button(
                f,
                "Next: choose a city",
                Action::CompanyNext,
                BtnKind::Primary
            )],
        ));
        let _ = CustomTile;
    });
}

fn company_input(
    mut events: EventReader<KeyboardInput>,
    menu: Res<Menu>,
    mut form: ResMut<CompanyForm>,
    mut company: ResMut<Company>,
    mut ui_events: EventWriter<UiAction>,
) {
    if menu.screen != Screen::Company {
        events.clear();
        return;
    }
    for ev in events.read() {
        if !ev.state.is_pressed() {
            continue;
        }
        match &ev.logical_key {
            Key::Tab => {
                form.focus = if form.focus == Focus::Name {
                    Focus::Path
                } else {
                    Focus::Name
                };
            }
            Key::Enter => {
                ui_events.write(UiAction(if form.focus == Focus::Name {
                    Action::CompanyNext
                } else {
                    Action::LogoLoad
                }));
            }
            Key::Backspace => match form.focus {
                Focus::Name => {
                    company.name.pop();
                }
                Focus::Path => {
                    form.path.pop();
                }
            },
            Key::Space | Key::Character(_) => {
                let s = match &ev.logical_key {
                    Key::Character(s) => s.to_string(),
                    _ => " ".to_string(),
                };
                for ch in s.chars().filter(|c| !c.is_control()) {
                    match form.focus {
                        Focus::Name => {
                            if company.name.chars().count() < NAME_MAX {
                                company.name.push(ch);
                            }
                        }
                        Focus::Path => form.path.push(ch),
                    }
                }
            }
            _ => {}
        }
    }
}

fn company_actions(
    mut events: EventReader<UiAction>,
    mut form: ResMut<CompanyForm>,
    mut company: ResMut<Company>,
    mut menu: ResMut<Menu>,
    mut settings: ResMut<Settings>,
    mut images: ResMut<Assets<Image>>,
) {
    for UiAction(a) in events.read() {
        match *a {
            Action::CompanyField(i) => {
                form.focus = if i == 0 { Focus::Name } else { Focus::Path };
            }
            Action::LogoPick(i) => {
                company.logo = format!("d{i}");
                form.message.clear();
            }
            Action::LogoLoad => {
                if form.path.trim().is_empty() {
                    form.message = "Type or paste the path of a PNG or JPG file first.".into();
                    form.error = true;
                    continue;
                }
                match load_custom(&form.path, &mut images) {
                    Ok(h) => {
                        company.logo = form.path.trim().trim_matches('"').to_string();
                        company.custom = Some(h);
                        form.message = "Your logo is selected.".into();
                        form.error = false;
                    }
                    Err(e) => {
                        form.message = e;
                        form.error = true;
                    }
                }
            }
            Action::CompanyNext => {
                let name = company.name.trim().to_string();
                if name.chars().count() < 2 {
                    form.message = "Please type a company name (at least 2 letters).".into();
                    form.error = true;
                    form.focus = Focus::Name;
                    continue;
                }
                company.name = name;
                settings.company_name = company.name.clone();
                settings.company_logo = company.logo.clone();
                settings.save();
                form.message.clear();
                menu.screen = Screen::NewGame;
            }
            _ => {}
        }
    }
}

fn company_render(
    time: Res<Time<Real>>,
    company: Res<Company>,
    form: Res<CompanyForm>,
    mut name: Query<
        (&mut Text, &mut TextColor),
        (With<NameText>, Without<PathText>, Without<MessageText>),
    >,
    mut path: Query<
        (&mut Text, &mut TextColor),
        (With<PathText>, Without<NameText>, Without<MessageText>),
    >,
    mut msg: Query<
        (&mut Text, &mut TextColor),
        (With<MessageText>, Without<NameText>, Without<PathText>),
    >,
    mut tiles: Query<(&LogoTile, &mut BorderColor)>,
) {
    let on = (time.elapsed_secs() * 2.0) as i64 % 2 == 0;
    let field = |t: &mut (Mut<Text>, Mut<TextColor>), value: &str, hint: &str, focused: bool| {
        let caret = if focused && on { "|" } else { " " };
        if value.is_empty() {
            t.0 .0 = format!("{}{hint}", if focused { caret } else { "" });
            t.1 .0 = style::MUTED;
        } else {
            t.0 .0 = format!("{value}{}", if focused { caret } else { "" });
            t.1 .0 = style::TEXT;
        }
    };
    if let Ok(mut t) = name.single_mut() {
        field(
            &mut t,
            &company.name,
            "e.g. Sunrise Transit",
            form.focus == Focus::Name,
        );
    }
    if let Ok(mut t) = path.single_mut() {
        field(
            &mut t,
            &form.path,
            "e.g. C:\\Users\\me\\logo.png",
            form.focus == Focus::Path,
        );
    }
    if let Ok(mut t) = msg.single_mut() {
        t.0 .0 = form.message.clone();
        t.1 .0 = if form.error {
            style::DANGER
        } else {
            style::ACCENT
        };
    }
    for (tile, mut b) in &mut tiles {
        let sel = company.default_logo() == Some(tile.0 as usize);
        let want = if sel { style::WHITE } else { Color::NONE };
        if b.0 != want {
            b.0 = want;
        }
    }
}
