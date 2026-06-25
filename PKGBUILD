# Maintainer: staszek <staszekborkowski7@gmail.com>
pkgname=plaza
pkgver=0.1.0
pkgrel=2
pkgdesc="Cross-distro TUI package-manager browser (Arch: pacman + AUR)"
arch=('x86_64')
url="https://github.com/StaszeKrk/plaza"
license=('GPL-3.0-or-later')
depends=('gcc-libs')
optdepends=('yay: AUR search and per-source upgrades'
            'pacman-contrib: live update counts via checkupdates')
makedepends=('cargo')
options=('!lto')

# Builds from the local checkout (run `makepkg -si` in the repo root).
build() {
    cd "$startdir"
    export CARGO_TARGET_DIR=target
    cargo build --frozen --release
}

check() {
    cd "$startdir"
    export CARGO_TARGET_DIR=target
    cargo test --frozen --release
}

package() {
    cd "$startdir"
    install -Dm755 "target/release/$pkgname" "$pkgdir/usr/bin/$pkgname"
    install -Dm644 LICENSE "$pkgdir/usr/share/licenses/$pkgname/LICENSE"
}
