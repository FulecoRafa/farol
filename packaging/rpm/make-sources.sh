#!/bin/sh
# Builds the Source files farol.spec needs:
#   farol-<version>.tar.gz                  git archive of the tree
#   Phosphor-Fill.ttf                       the icon font the kit draws glyphs with
#   tessela-<tversion>.tar.xz               git archive of tessela (core/ builds
#                                           on its SDK, sdk/rust); the same file
#                                           tessela.spec builds from
#   adwaita-shell-<aversion>.tar.gz         git archive of adwaita-shell, for its
#                                           kit/ (the same file adwaita-shell.spec
#                                           builds from)
#   farol-<version>-vendor.tar.xz           every crate in core/Cargo.lock
#                                           (cargo vendor), so rpmbuild never
#                                           touches the network
#
#   packaging/rpm/make-sources.sh [outdir]    (default: ~/rpmbuild/SOURCES)
#   REF=v0.1.0 packaging/rpm/make-sources.sh  (default: HEAD)
#   TESSELA=~/src/tessela TESSELA_REF=v0.1.0 ADWAITA=~/src/adwaita-shell ADWAITA_REF=v0.1.0 \
#       packaging/rpm/make-sources.sh         (defaults: the sibling checkouts
#                                             ../tessela and ../adwaita-shell, HEAD)
#
# Needs git, curl, cargo and xz (e.g. `nix develop ../tessela -c packaging/rpm/make-sources.sh`).
set -eu

here=$(cd "$(dirname "$0")" && pwd)
root=$(git -C "$here" rev-parse --show-toplevel)
# A git worktree's top level is not necessarily named farol; the siblings
# default to the main checkout's.
common=$(cd "$root" && cd "$(git rev-parse --git-common-dir)/.." && pwd)
ref=${REF:-HEAD}
tessela=${TESSELA:-$common/../tessela}
tref=${TESSELA_REF:-HEAD}
adwaita=${ADWAITA:-$common/../adwaita-shell}
aref=${ADWAITA_REF:-HEAD}
spec=$here/farol.spec
version=$(sed -n 's/^Version:[[:space:]]*//p' "$spec")
tversion=$(sed -n 's/^%global tessela_version[[:space:]]*//p' "$spec")
aversion=$(sed -n 's/^%global adwaita_version[[:space:]]*//p' "$spec")
font=$(sed -n 's/^Source1:[[:space:]]*//p' "$spec")
name=farol-$version
tname=tessela-$tversion
aname=adwaita-shell-$aversion
out=${1:-$HOME/rpmbuild/SOURCES}
mkdir -p "$out"
out=$(cd "$out" && pwd)

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT INT TERM

git -C "$root" archive --format=tar.gz --prefix="$name/" -o "$out/$name.tar.gz" "$ref"
curl -fsSL -o "$out/Phosphor-Fill.ttf" "$font"
git -C "$tessela" archive --format=tar --prefix="$tname/" "$tref" > "$tmp/$tname.tar"
xz -T3 -c "$tmp/$tname.tar" > "$out/$tname.tar.xz"
git -C "$adwaita" archive --format=tar.gz --prefix="$aname/" -o "$out/$aname.tar.gz" "$aref"

# Vendor core/ with the SDK where its Cargo.toml looks for it
# (../../tessela/sdk/rust); path dependencies are not vendored.
tar -xzf "$out/$name.tar.gz" -C "$tmp"
tar -xf "$tmp/$tname.tar" -C "$tmp"
mv "$tmp/$tname" "$tmp/tessela"
(cd "$tmp/$name/core" && cargo vendor --quiet --locked --versioned-dirs cargo-vendor > /dev/null)
tar -C "$tmp/$name" --sort=name --owner=0 --group=0 --numeric-owner \
    -cf - core/cargo-vendor | xz -T3 > "$out/$name-vendor.tar.xz"

echo "$out/$name.tar.gz"
echo "$out/Phosphor-Fill.ttf"
echo "$out/$tname.tar.xz"
echo "$out/$aname.tar.gz"
echo "$out/$name-vendor.tar.xz"
