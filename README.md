<h1 align="center">Bus Manager</h1>

<p align="center">
  A bus company management game on <b>real OpenStreetMap data</b>, built with Rust and Bevy.
</p>

<p align="center">
  <a href="https://github.com/YOZTeam/BusManager/releases/latest"><img alt="Release" src="https://img.shields.io/github/v/release/YOZTeam/BusManager?include_prereleases&sort=semver&color=2e7d32"></a>
  <a href="https://github.com/YOZTeam/BusManager/actions/workflows/build.yml"><img alt="Build" src="https://img.shields.io/github/actions/workflow/status/YOZTeam/BusManager/build.yml?label=build"></a>
  <a href="https://github.com/YOZTeam/BusManager/actions/workflows/ci.yml"><img alt="CI" src="https://img.shields.io/github/actions/workflow/status/YOZTeam/BusManager/ci.yml?label=tests"></a>
  <a href="LICENSE"><img alt="License" src="https://img.shields.io/github/license/YOZTeam/BusManager"></a>
  <a href="https://github.com/YOZTeam/BusManager/releases"><img alt="Downloads" src="https://img.shields.io/github/downloads/YOZTeam/BusManager/total?color=2e7d32"></a>
  <a href="https://discord.gg/Ppx7u4g2Wt"><img alt="Discord" src="https://img.shields.io/discord/1573640143842058240?logo=discord&logoColor=white&label=discord&color=5865F2"></a>
  <a href="https://ko-fi.com/adamikhlef"><img alt="Ko-fi" src="https://img.shields.io/badge/Ko--fi-support-FF5E5B?logo=ko-fi&logoColor=white"></a>
  <img alt="Platforms" src="https://img.shields.io/badge/platforms-Windows%20%7C%20macOS%20%7C%20Linux-555">
  <img alt="Rust" src="https://img.shields.io/badge/Rust-2021-b7410e?logo=rust&logoColor=white">
  <img alt="Bevy" src="https://img.shields.io/badge/Bevy-0.16-232326">
  <a href="https://github.com/YOZTeam/BusManager/pulls"><img alt="PRs welcome" src="https://img.shields.io/badge/PRs-welcome-brightgreen"></a>
</p>

Real cities from OpenStreetMap (downloaded in game), OSM-style map with German **H** bus stops,
a 3D depot, timetables, drivers and shifts, real bus routes and player-made bus mods.
Targets Windows, macOS and Ubuntu.

> **Status:** early development. Expect missing features (saving and loading are not implemented yet).

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
BUSMANAGER_REPO=YOZTeam/BusManager cargo build --release -p busmanager-installer
```

(The included GitHub Actions workflow does this automatically.)

See [CONTRIBUTING.md](CONTRIBUTING.md), [SECURITY.md](SECURITY.md) and [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md). Issues must be in English.

## Support

Bus Manager is free. If you enjoy it you can support development on [Ko-fi](https://ko-fi.com/adamikhlef).

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
| Enter or "Finish" | Open the line (free; assigns up to 2 unassigned buses) |
| Real routes list in the planner | Adopt a real OpenStreetMap bus route (stops, name, interval) as a starting point |
| "Timetable" in the Lines panel | First/last departure, interval and bus count; shows today's departures |
| Esc | Cancel |
| "+ Bus" in the Lines panel | Add a bus to that line |
| Space, 1 / 2 / 3 | Pause, speed 1x / 4x / 10x |
| Tab or Map/Depot buttons | Switch between map and 3D depot |
| Staff button | Hire and fire drivers |

Buildings, parks and water are drawn from OpenStreetMap too (buildings show when zoomed in), and stops near more buildings get more passengers.

Hover a stop to see its name and waiting passengers. A red dot on a stop means people are piling up.

## The depot and your fleet

The Depot view shows your parked buses in 3D (16 bays). Click a bus to see its card and side panel
(Service / Sell); hover cards show fuel, condition and cleanliness. Buy new buses in the fleet shop,
then assign them to lines from the map ("+ Bus"). Buses wear out while running and return to the
depot when they need service. Selling returns 60% of the price. Depot camera: wheel zoom, right/middle drag, WASD.

## Timetables, shifts and staff

The game has a clock (1 real second = 3 game seconds; the day starts at 05:30). Each line has a timetable
(default 06:00–20:30 every 15 minutes, or the `interval` of the imported OSM route). A bus assigned to the
line leaves the depot shortly before each departure, drives to the first stop, waits for the departure
time, runs a loop and either serves the next departure or returns to the depot. Every trip needs a driver
whose shift covers the departure time: **Early** 05:00–13:00, **Late** 13:00–21:00, **Night** 21:00–05:00.
Drivers are paid daily at midnight whether they drive or not; better drivers cost more but use less fuel and
wear buses less. Hire from the candidate list in the Staff view (new candidates every morning). Trips
with no bus or no driver are shown as missed in the timetable panel. Passenger demand follows the time of
day (rush hours around 08:00 and 17:00, quiet nights).

## Real bus routes

The city download now includes OpenStreetMap `route=bus` relations. In the route planner they are listed
under "Real routes (OSM)" (search by number or name); picking one fills the stop list, which you can adjust
before pressing Finish. Cities downloaded with older versions keep working but have no real routes until
they are downloaded again (delete the city's cache file in the BusManager `cache` folder).

## Bus mods

Four categories: Minibus, Solo bus, Double-decker, Articulated (10 built-in buses; shop tabs per category). Add your own by dropping a folder into
`<user data dir>/BusManager/mods/buses/<name>/` (or `mods/buses/` next to the executable) containing
`model.glb` and `bus.json`. See `mods_example/buses/double-decker`. Model convention: origin on the ground
below the bus center, length along Z, front toward +Z, 1 unit = 1 meter. `bus.json` fields: `name`,
`capacity`, `price`, optional `category` (`minibus`/`solo`/`doubledecker`/`articulated`, guessed from size if missing), `length`, `width`, `height`, `rotation_y`, `scale`.

## Roadmap

- [x] 3D depot, fleet shop, bus mods
- [x] Route-planning screen (waypoints, stop search, live length/time)
- [x] Timetables, shifts, staff, real OSM routes
- [ ] Depot expansion and rooms, bus stop shelters, passengers' destinations
- [ ] Real demand from buildings/POIs and passenger destinations
- [ ] Timetables and bus stops from `public_transport=platform` / bus route relations
- [ ] Per-line profit graphs, bus upgrades, loans
- [ ] Save/load
