# Bus Manager (prototype)

A City Bus Manager-style game built with Rust + Bevy, on **real OpenStreetMap data**, drawn in the
OpenStreetMap standard look (green land, outlined roads, German **H** bus stop signs).
Targets Windows, macOS and Ubuntu.

## For players: the installer

Download `BusManager-Setup-<your system>` from the latest GitHub Release and run it.
It can **install**, **update**, **repair** and **uninstall** the game, and creates a Start menu /
applications-menu shortcut.

Uninstall can optionally delete the cities you downloaded.

## For developers

```bash
cargo run --release -p busmanager        # the game
cargo run --release -p busmanager-installer   # the installer
```

The installer downloads from GitHub Releases. Set your repository when building it:

```bash
BUSMANAGER_REPO=yourname/busmanager cargo build --release -p busmanager-installer
```

(The included GitHub Actions workflow does this automatically.)

### Publishing a release

1. Bump `version` in the root `Cargo.toml` (`[workspace.package]`).
2. `git tag v0.2.0 && git push --tags`
3. The workflow builds Windows, macOS and Ubuntu and attaches:
   `busmanager-<os>.zip|tar.gz` (what the installer downloads) and `BusManager-Setup-<os>`.

Installed copies show an **Update** button once a newer release exists.

### Linux build dependencies

```bash
sudo apt-get install -y libasound2-dev libudev-dev libwayland-dev libxkbcommon-dev \
  libx11-dev libxcursor-dev libxrandr-dev libxi-dev libgl1-mesa-dev
```

## Playing

Start the game, type a city name, pick the result. The map downloads from OpenStreetMap
and is saved on your computer, so it loads offline next time (listed under "Downloaded cities").
Optional: `busmanager path/to/city.osm.pbf` loads a local extract.

| Input | Action |
|---|---|
| Right / middle mouse drag, WASD, arrows | Pan |
| Mouse wheel | Zoom |
| N or "New line" | Start a line; click bus stops in order |
| Enter or "Finish" | Open the line (3 buses) |
| Esc | Cancel |
| "+ Bus" in the Lines panel | Add a bus to that line |
| Space, 1 / 2 / 3 | Pause, speed 1x / 4x / 10x |

Buildings, parks and water are drawn from OpenStreetMap too (buildings show when zoomed in), and stops near more buildings get more passengers.

Hover a stop to see its name and waiting passengers. A red dot on a stop means people are piling up.

## Roadmap

- [ ] Real demand from buildings/POIs and passenger destinations
- [ ] Timetables and bus stops from `public_transport=platform` / bus route relations
- [ ] Per-line profit graphs, bus upgrades, loans
- [ ] Save/load
