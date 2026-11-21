## Summary



## Type of Change

- [ ] Bug fix
- [ ] New gameplay / simulation feature (e.g., demand, timetables, economy, save/load)
- [ ] OpenStreetMap parsing / map rendering / UI change
- [ ] Installer (`busmanager-installer`) change (install, update, repair, uninstall)
- [ ] Dependency update (`Cargo.toml` / `Cargo.lock`)
- [ ] CI / release workflow / documentation update

## How to Test



## Checklist

- [ ] `cargo check --workspace` (or `cargo build --release -p busmanager` / `-p busmanager-installer`) builds cleanly without new warnings.
- [ ] Tested with OpenStreetMap city download and/or a local `.osm.pbf` extract (if map/simulation logic was touched).
- [ ] Tested `BUSMANAGER_REPO=...` build / release asset expectations (if `busmanager-installer` was touched).
- [ ] Verified on target platform(s) where applicable (Windows / macOS / Ubuntu).
- [ ] `README.md` or roadmap checkboxes updated if user-facing behavior or controls changed.
