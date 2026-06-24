# Plaza

Plaza is a terminal UI for finding and installing packages. You search once and it
queries every package source on the system at the same time. On Arch that means the
official repositories and the AUR. Results are merged into one list, so a package
name shows up once even when several sources provide it. Open a package to see each
source that ships it, then install from inside Plaza. Installs run in the background,
so you can keep searching while one downloads.

Plaza is Arch-only for now (pacman and the AUR). The source backends sit behind a
trait, so apt, dnf, zypper, flatpak and snap can be added later.

## What it does

- Searches all sources at once. Packages with the same name across sources are
  merged into one row.
- Shows every repo or source that provides a package, with versions and what is
  already installed. The repo pacman installs from by default is marked.
- Installs in a background pane backed by a real terminal, so sudo prompts and AUR
  build questions work normally. A hotkey returns you to it.
- Can install from a specific repo instead of the default.
- Streams results in as each source replies, so a slow or offline source does not
  block the rest.
- Has a small options menu (press o): hide the keybinding hints, collapse all repos
  into one [official] badge, and set the search delay. Settings are saved to
  ~/.config/plaza/settings.json.
- Flags AUR packages whose PKGBUILD changed in the last seven days.

## Requirements

- Rust and Cargo to build
- pacman, for official-repo search and install
- yay, for AUR search and install
- checkupdates (from pacman-contrib), for the update count in the sidebar

## Building

```sh
cargo build --release
./target/release/plaza
```

A headless search is also available:

```sh
cargo run -- --search firefox
```

## Keys

| Key | Action |
| --- | --- |
| type | search; the bar is focused at launch |
| / | focus the search bar from anywhere |
| Esc, Enter, Down | leave the search bar for the list |
| Up/Down, j/k | move within the focused pane |
| Left/Right, h/l, Tab | move between panes |
| Enter | open a package, or install the selected source |
| backtick | open or collapse the install pane |
| Esc in the install pane | step it down: full, peek, hidden |
| Ctrl-C in a focused install | cancel that install |
| o | options |
| q | quit; during an install it switches to the install instead |

## License

GPL-3.0-or-later. See [LICENSE](LICENSE). Plaza is free software: you can
redistribute it and/or modify it under the terms of the GNU General Public License
as published by the Free Software Foundation, either version 3 of the License, or
(at your option) any later version.
