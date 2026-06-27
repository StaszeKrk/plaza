# Plaza

Plaza is a customizable and riceable terminal UI for finding, installing, and managing packages. You search
once and it queries every package source on the system at the same time. On Arch
that means the official repositories and the AUR. Results are merged into one list,
so a package name shows up once even when several sources provide it. A separate
Manage view lists everything installed, shows what has updates, and lets you remove
or upgrade without leaving Plaza. Actions run in a background pane backed by a real
terminal, so you can keep working while one runs.

Plaza is Arch-only for now (pacman and the AUR). The source backends sit behind a
trait, so apt, dnf, zypper, flatpak, and snap can be added later.

## What it does

Search:

- Queries all sources at once. Packages with the same name across sources are
  merged into one row.
- Shows every repo or source that provides a package, with versions and what is
  already installed. The repo pacman installs from by default is marked.
- Can install from a specific repo instead of the default.
- Streams results in as each source replies, so a slow or offline source does not
  block the rest.
- Flags AUR packages whose PKGBUILD changed in the last seven days.

Manage:

- Lists every installed package with its origin repo (or `aur`), filterable by
  typing.
- Floats packages with a pending update to the top, marked with the new version.
- Removes the selected package at a configurable depth (`-Rs` by default, also
  `-Rns` or `-R`, set in options).
- Upgrades per source or all at once. "All" chains each source in one task
  (`sudo pacman -Syu && paru -Sua`, using whichever AUR helper you have).
- Upgrades a single highlighted package with `u`, when it has a pending update.

General:

- Filters either list by repository. Press `f` for a checkbox box in the sidebar
  to show only the repos you pick (toggle one repo, all pacman repos at once, or
  the AUR). It follows the collapse-repos option. By default the box appears only
  while you are in it or while a filter is active; turn off "hide filter box when
  not in use" in options to keep it on screen at all times. The selection resets
  each launch.
- Actions run in a background pane backed by a real terminal, so sudo prompts and
  AUR build questions work normally. A hotkey returns you to it.
- Confirming an action adds it to a queue. The queue runs one task at a time,
  moves on by itself when a task succeeds, and pauses on a failure for you to
  dismiss or clear. Auto-advance does not pull you back to the pane, and queued
  items can be removed one at a time from it.
- When a running task stops at a prompt (sudo password, a pacman or AUR
  question) and you are not on the action pane, the status bar tells you it is
  waiting for input and which key opens the pane to answer.
- A small options menu (press `o`): hide the keybinding hints, collapse all repos
  into one `[official]` badge, switch the color palette and skin (see
  [Theming](#theming)), set the search delay, pick the remove depth, choose
  the AUR helper (auto, yay, or paru), and whether the filter box hides when it
  is not in use. Settings are saved to `~/.config/plaza/settings.json`.

## Requirements

- pacman, for official-repo search, install, and removal
- an AUR helper (yay or paru), for AUR installs and upgrades. AUR search itself
  needs no helper; with neither installed you can still browse AUR results.
- checkupdates (from pacman-contrib), for live update counts without root
- Rust and Cargo, to build

## Install

As a pacman package (tracked by pacman, removable with `pacman -R plaza`):

```sh
makepkg -si
```

Or build directly with Cargo:

```sh
cargo build --release
./target/release/plaza
```

A headless search is also available:

```sh
plaza --search firefox
```

## Navigation

Plaza has two modes, like a tiling layout you tab around:

- Navigate: arrow keys (or `hjkl`) move the highlighted panel. The highlight is
  shown with the theme's hover border color (amber in the default theme).
- Interact: press Enter or Space to focus the highlighted panel. Its border turns
  the theme's active accent color and the arrow keys now act inside it (move the
  selection, pick a scope, type in the search box). Press Esc to step back to
  navigate.

## Keys

| Key | Action |
| --- | --- |
| type | search (or filter, in Manage); the bar is focused at launch |
| Enter (in search) | run it and focus the results |
| arrows, hjkl | navigate mode: move the highlight. interact mode: move inside the panel |
| Enter, Space | focus the highlighted panel |
| Esc | step out of the focused panel |
| Tab | switch between the Search and Manage views |
| / | jump to the search bar from anywhere |
| f | open or close the repository filter; Space toggles a checkbox |
| Enter (on a result) | open it, then Enter on a source to install |
| r, Enter (in Manage list) | remove the selected package |
| u (in Manage list) | upgrade the selected package, if it has an update |
| h/l then Enter (on the upgrade chips) | upgrade that scope |
| backtick | open or collapse the action pane |
| j/k, d, x (in the action pane) | move within the queue, remove the selected item, or clear it |
| Ctrl-C in a focused action | cancel that action |
| o | options |
| q | quit; during an action it switches to the action instead |

Search and Manage keep separate search text, so switching views does not lose
either one.

## Theming

Plaza's look is split into two independent, swappable parts:

- a **palette**: the colors, and
- a **skin**: everything else (border style, corner radius, glyphs/icons, and the
  highlight and badge styles).

Switch either one live from the options menu (`o`): the `Palette` and `Skin` rows
cycle through the built-ins plus anything you have added, and the choice is saved
to `~/.config/plaza/settings.json`.

Built-in palettes: `plaza-dusk` (default), `gruvbox`, `nord`, `dracula`,
`tokyo-night`, `solarized-dark`, and `ansi` (which uses your terminal's own 16
colors, so it follows whatever theme the terminal is set to). Built-in skins:
`soft` (default), `sharp`, and `plain` (square borders and no Nerd Font glyphs,
for terminals without one).

To make your own, drop a `.toml` file in `~/.config/plaza/palettes/` or
`~/.config/plaza/skins/`. The file name is the theme name. Plaza loads new files
on the next launch, and edits to the file that is currently active reload live.
A palette file may set only the colors it wants to change; the rest fall back to
the default. See [docs/theming.md](docs/theming.md) for the full format and field
list.

## License

GPL-3.0-or-later. See [LICENSE](LICENSE). Plaza is free software: you can
redistribute it and/or modify it under the terms of the GNU General Public License
as published by the Free Software Foundation, either version 3 of the License, or
(at your option) any later version.
