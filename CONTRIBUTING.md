# Contributing

Thanks for helping! A few ground rules keep the project healthy.

## Language

Issues, pull requests, commit messages and code comments are in **English**.

## Before you start

- Check the roadmap in the README and the open issues.
- For anything bigger than a small fix, open an issue first so we can agree on the approach.

## Building

```bash
cargo run -p busmanager --release     # the game
cargo test --workspace                # unit tests
cargo fmt --all                       # formatting (CI checks it)
```

Linux needs a few system libraries, see the README. The first build of Bevy takes a while.

Handy test options: `BUSMANAGER_CLOCK=05:50` starts the game clock at that time, `BUSMANAGER_WEAR=120` makes buses wear out quickly.

## Art, fonts and sounds

Only submit assets you made yourself or that have a license allowing redistribution (state the license in the PR).
**Never include assets, logos, names or screenshots taken from other games.** This project is inspired by
bus-management games but must stay legally independent.

## Bus mods

Community bus models are welcome (see "Bus mods" in the README). Only glTF/GLB models and a `bus.json` are accepted, never executable code.

## Pull requests

- One topic per PR, small and focused
- Fill in the PR template, add screenshots for visible changes
- Keep the code simple; match the style of the surrounding code
