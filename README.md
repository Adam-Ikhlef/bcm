# Bus Manager (prototype)

A City Bus Manager-style game built with Rust + Bevy, running on **real OpenStreetMap data**.
Targets Windows, macOS and Ubuntu.

## Get a map

1. Download an `.osm.pbf` extract of a city (small extracts load fastest):
   - https://extract.bbbike.org (pick any city, choose "Protocolbuffer (PBF)")
   - https://download.geofabrik.de (country/region extracts, large)
2. Save it as `assets/map.osm.pbf` (or pass a path as the first argument).

## Run

```bash
cargo run --release
# or
cargo run --release -- path/to/city.osm.pbf
```

## Controls

| Input | Action |
|---|---|
| WASD / arrows | Pan |
| Mouse wheel | Zoom |
| Left click | Add the nearest bus stop to the line you are drawing |
| Enter | Finish the line (needs 2+ stops); 3 buses start driving the loop |
| Esc | Cancel the line being drawn |

## Linux build dependencies

```bash
sudo apt-get install -y libasound2-dev libudev-dev libwayland-dev libxkbcommon-dev
```

## Roadmap

- [ ] Bake roads into a mesh instead of per-frame gizmos (big cities)
- [ ] Spatial index for stop snapping
- [ ] Timetables, bus capacity, passengers per stop (use residential/POI density)
- [ ] Money, fleet purchases, line profitability
- [ ] Save/load, city chooser, UI
- [ ] Bus stops from `public_transport=platform` and bus route relations
