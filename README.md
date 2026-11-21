<p align="center">
  <a href="https://github.com/YOZTeam/BusManager/releases/latest"><img alt="Version" src="https://img.shields.io/github/v/release/YOZTeam/BusManager?label=version&color=f47f30&style=for-the-badge"></a>
  <a href="https://busmanager.yoz-team.online/"><img alt="Playing now" src="https://img.shields.io/endpoint?url=https%3A%2F%2Fbcm.adamikhlef.workers.dev%2Fbadge&label=playing%20now&style=for-the-badge&logo=steam&logoColor=white"></a>
  <a href="https://github.com/YOZTeam/BusManager/actions/workflows/build.yml"><img alt="Build" src="https://img.shields.io/github/actions/workflow/status/YOZTeam/BusManager/build.yml?branch=main&style=for-the-badge&label=build"></a>
  <a href="https://bcm.yoz-team.online/"><img alt="Docs" src="https://img.shields.io/badge/docs-website-2d3138?style=for-the-badge"></a>
  <a href="https://discord.gg/Ppx7u4g2Wt"><img alt="Discord" src="https://img.shields.io/badge/discord-join%20us-5865F2?style=for-the-badge&logo=discord&logoColor=white"></a>
  <a href="https://ko-fi.com/adamikhlef"><img alt="Ko-fi" src="https://img.shields.io/badge/ko--fi-support-29abe0?style=for-the-badge&logo=kofi&logoColor=white"></a>
  <a href="LICENSE"><img alt="License" src="https://img.shields.io/github/license/YOZTeam/BusManager?style=for-the-badge"></a>
</p>

> [!WARNING]
> **Early development. Expect bugs.** Things may be missing, broken or change between
> versions. Please report problems in [Issues](https://github.com/YOZTeam/BusManager/issues)
> or on our [Discord server](https://discord.gg/Ppx7u4g2Wt).

# Bus Manager (prototype)

A bus company management game built with Rust + Bevy, on **real OpenStreetMap data**, drawn in the
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
| N or "New line" | Open the route planner (deeper green map, Route panel on the left) |
| Click a stop / click a road | Add the stop / add a waypoint that bends the route along roads |
| Search box (click it, type, Enter) | Find a stop by name and add it |
| Backspace or "Undo" | Remove the last stop or waypoint |
| Enter or "Finish" | Open the line (free; auto-assigns up to 2 idle buses) |
| Esc | Cancel |
| "+ Bus" in the Lines panel | Add a bus to that line |
| Space, 1 / 2 / 3 | Pause, speed 1x / 4x / 10x |
| Tab or Map/Depot buttons | Switch between map and 3D depot |

Buildings, parks and water are drawn from OpenStreetMap too (buildings show when zoomed in), and stops near more buildings get more passengers.

Hover a stop to see its name and waiting passengers. A red dot on a stop means people are piling up.

## The depot and your fleet

The Depot view shows your parked buses in 3D (16 bays). Click a bus to see its card and side panel
(Service / Sell); hover cards show fuel, condition and cleanliness. Buy new buses in the fleet shop,
then assign them to lines from the map ("+ Bus"). Buses wear out while running and return to the
depot when they need service. Selling returns 60% of the price. Depot camera: wheel zoom, right/middle drag, WASD.

## Bus mods

Four categories: Minibus, Solo bus, Double-decker, Articulated (10 built-in buses; shop tabs per category). Add your own by dropping a folder into
`<user data dir>/BusManager/mods/buses/<name>/` (or `mods/buses/` next to the executable) containing
`model.glb` and `bus.json`. See `mods_example/buses/double-decker`. Model convention: origin on the ground
below the bus center, length along Z, front toward +Z, 1 unit = 1 meter. `bus.json` fields: `name`,
`capacity`, `price`, optional `category` (`minibus`/`solo`/`doubledecker`/`articulated`, guessed from size if missing), `length`, `width`, `height`, `rotation_y`, `scale`.

## Roadmap

- [x] 3D depot, fleet shop, bus mods
- [x] Route-planning screen (waypoints, stop search, live length/time)
- [ ] Shifts: buses leave the depot shortly before their timetable starts
- [ ] Real demand from buildings/POIs and passenger destinations
- [ ] Timetables and bus stops from `public_transport=platform` / bus route relations
- [ ] Per-line profit graphs, bus upgrades, loans
- [ ] Save/load
