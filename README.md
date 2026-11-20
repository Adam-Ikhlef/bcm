# Bus Manager (prototype)

A City Bus Manager-style game built with Rust + Bevy, running on **real OpenStreetMap data**.
Targets Windows, macOS and Ubuntu.

## Run

```bash
cargo run --release
```

In the menu, type a city name, press Enter, pick the right result (Up/Down + Enter).
The game downloads the road network and bus stops from OpenStreetMap (Nominatim + Overpass API)
and starts. Downloads are cached in `cache/`, so the same city loads instantly next time.
No `.pbf` files and no recompiling needed.

Optional: skip the menu with a local extract: `cargo run --release -- path/to/city.osm.pbf`

> Big cities (millions of people) download a lot of data and take longer. Start with a mid-size
> city or town. Only cities that exist as an administrative area in OpenStreetMap are listed.

## How to play

| Input | Action |
|---|---|
| WASD / arrows | Pan |
| Mouse wheel | Zoom |
| Left click | Add the nearest bus stop to the line you are drawing |
| Enter | Open the line (needs 2+ stops, costs 3 buses) |
| B | Buy one more bus for the latest line |
| Esc | Cancel the line being drawn |
| 1 / 2 / 3 | Game speed x1 / x4 / x10 |
| Space | Pause |

Passengers appear at stops (busier areas, judged by road density, generate more). Stops grow and
turn red as people wait. Buses pick them up, you earn fares, buses cost money to buy and run.
Connect busy stops with efficient loops to stay profitable.

## Linux build dependencies

```bash
sudo apt-get install -y libasound2-dev libudev-dev libwayland-dev libxkbcommon-dev
```

## Roadmap

- [ ] Bake roads into a mesh instead of per-frame gizmos (big cities)
- [ ] Real demand from buildings/POIs and passenger destinations
- [ ] Timetables and bus stops from `public_transport=platform` / bus route relations
- [ ] Per-line profit stats, bus upgrades, loans
- [ ] Save/load, in-game city chooser improvements, proper UI
