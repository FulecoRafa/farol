# farol: a launcher on tessela. Builds the core (core/, Rust, on tessela's
# SDK) offline from vendored crates and installs it as /usr/bin/farol (the
# CLI, and with no arguments the service the runner starts), the project to
# /usr/share/farol with adwaita-shell's kit beside it and tessela.toml
# pointing at both, and a systemd user unit running `farol start`.
#
#   packaging/rpm/make-sources.sh ~/rpmbuild/SOURCES
#   rpmbuild -ba packaging/rpm/farol.spec

%bcond check 1

# The tessela source core/ builds against (its sdk/rust) and the
# adwaita-shell source the kit comes from; make-sources.sh reads these lines.
%global tessela_version 0.1.0
%global adwaita_version 0.1.0

Name:           farol
Version:        0.1.0
Release:        1%{?dist}
Summary:        Launcher for Wayland built with tessela

# farol and adwaita-shell's kit are MIT OR Apache-2.0; the bundled Phosphor
# icon font is MIT. farol statically links the vendored crates; their
# licenses, from %%cargo_license_summary:
SourceLicense:  (MIT OR Apache-2.0) AND MIT
License:        (MIT OR Apache-2.0) AND MIT AND %{shrink:
                ((MIT OR Apache-2.0) AND Unicode-3.0) AND
                (Apache-2.0 OR MIT) AND
                (Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT) AND
                MIT AND
                (MIT OR Apache-2.0) AND
                MPL-2.0 AND
                (Unlicense OR MIT) AND
                Zlib
                }
# Provisional: the project has no public home yet.
URL:            https://github.com/FulecoRafa/farol
Source0:        %{name}-%{version}.tar.gz
# The kit's Icon component draws icons with the "Phosphor-Fill" font family.
Source1:        https://unpkg.com/@phosphor-icons/web@2.1.1/src/fill/Phosphor-Fill.ttf
# core/Cargo.toml takes tessela's SDK from ../../tessela/sdk/rust.
Source2:        tessela-%{tessela_version}.tar.xz
# tessela.toml imports adwaita-shell's kit (components and theme).
Source3:        adwaita-shell-%{adwaita_version}.tar.gz
Source4:        %{name}-%{version}-vendor.tar.xz

ExclusiveArch:  %{rust_arches}

BuildRequires:  cargo-rpm-macros >= 26
BuildRequires:  rust >= 1.85
BuildRequires:  cargo
# rusqlite builds the bundled SQLite (C).
BuildRequires:  gcc
BuildRequires:  systemd-rpm-macros

# The runner: `farol start` runs /usr/bin/tessela, beside /usr/bin/farol.
Requires:       tessela
# The SDK is tessela's own crate, compiled into farol.
Provides:       bundled(crate(tessela-service)) = %{tessela_version}
Provides:       bundled(adwaita-shell-kit) = %{adwaita_version}
# Commands farol runs. File dependencies, so whichever package provides the
# command satisfies it: opening files, folders and URLs; the notification
# when a timer or an alarm ends.
Requires:       /usr/bin/xdg-open
Requires:       /usr/bin/notify-send
%{?systemd_requires}
# Copying when farol's own clipboard (Wayland data-control) is not running.
Recommends:     wl-clipboard
# Windows, workspaces and shortcuts talk to Hyprland; the rest runs on any
# wlroots compositor with layer-shell.
Recommends:     hyprland
# The theme's text font is "CaskaydiaCove Nerd Font", which Fedora does not
# ship; fontconfig falls back to another monospace font without it.
Recommends:     cascadia-mono-fonts

%description
farol is a launcher written as a tessela project: one resident process
that sleeps while closed, a search core in Rust and a TSX surface. It finds
apps, does math and unit conversions (fend), emoji, files in the home
directory, clipboard history, open windows, timers and alarms, web
searches, bookmarks, quicklinks and aliases, and runs external plugins.

Start it with `farol start` (or with the session:
  systemctl --user enable --now farol.service)
and open it with `farol toggle`, e.g. from a compositor keybinding, or
`farol hotkey super+alt+space` (bound in Hyprland when farol starts).

%prep
%autosetup -n %{name}-%{version} -a4
# The SDK where core/Cargo.toml looks for it.
tar -xf %{SOURCE2} -C ..
rm -rf ../tessela
mv ../tessela-%{tessela_version} ../tessela
# The kit, from adwaita-shell.
tar -xzf %{SOURCE3} adwaita-shell-%{adwaita_version}/kit
rm -rf kit
mv adwaita-shell-%{adwaita_version}/kit kit
rm -r adwaita-shell-%{adwaita_version}
cd core
%cargo_prep -v cargo-vendor

%build
cd core
%{cargo_build}
%{cargo_license_summary}
%{cargo_license} > ../LICENSE.dependencies
%cargo_vendor_manifest
# The SDK is a path dependency, listed with its build path, which the
# bundled(crate()) generator cannot parse; it has its own Provides above.
sed -i '/ (\//d' cargo-vendor.txt
mv cargo-vendor.txt ..

%install
install -Dpm0755 core/target/rpm/farol %{buildroot}%{_bindir}/farol

share=%{buildroot}%{_datadir}/%{name}
install -d $share
cp -a tessela.toml src kit $share/
# The service is /usr/bin/farol, the kit the copy above.
sed -i \
    -e 's|^command = \["core/target/release/farol"\]$|command = ["%{_bindir}/farol"]|' \
    -e 's|^themes = \["../adwaita-shell/kit/theme.toml"\]$|themes = ["%{_datadir}/%{name}/kit/theme.toml"]|' \
    -e 's|^kit = "../adwaita-shell/kit"$|kit = "%{_datadir}/%{name}/kit"|' \
    $share/tessela.toml

install -Dpm0644 %{SOURCE1} %{buildroot}%{_datadir}/fonts/%{name}/Phosphor-Fill.ttf

install -d %{buildroot}%{_userunitdir}
cat > %{buildroot}%{_userunitdir}/%{name}.service << 'EOF'
[Unit]
Description=farol launcher (tessela)
PartOf=graphical-session.target
After=graphical-session.target

[Service]
ExecStart=%{_bindir}/farol start
Restart=on-failure
RestartSec=2

[Install]
WantedBy=graphical-session.target
EOF

%check
toml=%{buildroot}%{_datadir}/%{name}/tessela.toml
grep -q '^command = \["%{_bindir}/farol"\]$' $toml
grep -q '^kit = "%{_datadir}/%{name}/kit"$' $toml
grep -q '^themes = \["%{_datadir}/%{name}/kit/theme.toml"\]$' $toml
# Nothing left pointing outside the package (`! grep` would not stop sh -e).
if grep -q '\.\./' $toml; then exit 1; fi
%if %{with check}
cd core
%{cargo_test}
%endif

%post
%systemd_user_post %{name}.service

%preun
%systemd_user_preun %{name}.service

%files
%license LICENSE-MIT LICENSE-APACHE
%license LICENSE.dependencies
%license cargo-vendor.txt
%doc README.md
%{_bindir}/farol
%{_datadir}/%{name}/
%{_datadir}/fonts/%{name}/
%{_userunitdir}/%{name}.service

%changelog
* Wed Oct 14 2026 Fuleco <fuleco.ra.pha@gmail.com> - 0.1.0-1
- First package
