# Astro Strike

A retro 2D pixel-art vertical space shooter built with [Bevy](https://bevy.org).
Fight your way up one seamless scrolling map through five sectors, each held by
a warlord boss — and each wearing its own color scheme, applied to the whole
screen by a palette shader with Game Boy-style dither transitions.

## Features

- Five levels on one continuous map, stitched from randomized Tiled (`.tmx`) chunks
- Per-level color schemes via a fullscreen palette-substitution shader
  (Bayer dither-dissolve on level transitions, gray-out on defeat)
- Boss fights, aimed enemy fire, destructible buildings, tank and truck patrols,
  UFO swarms that unlock level by level
- Pickups: invincibility, repair, weapon upgrade (single → double → triple), bomb
- Difficulty scales per level (spawn rate, enemy speed, bullet speed, boss health)

## Controls

| Action                 | Key                  |
| ---------------------- | -------------------- |
| Move                   | Arrow keys / W A S D |
| Fire                   | Space                |
| Advance dialogue       | Space / Enter        |
| Start / back to title  | Space / Enter        |

## Running

```sh
cargo run --release
```

Requires the `assets/` folder next to the executable (or run via cargo from the
project root).
