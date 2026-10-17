# Homebrew formula for farol (Linux only), for the same tap as tessela's, e.g.
#   brew tap FulecoRafa/tessela <git url of a tap repo holding this file>
#   brew install farol
# See README.md next to this file for how to release and test it.
class Farol < Formula
  desc "Launcher for Wayland: apps, calculator, files, clipboard, windows, timers"
  # Placeholders until the projects are published: point each `url` at a
  # release tarball (`git archive --prefix=<name>-0.1.0/ -o <name>-0.1.0.tar.gz v0.1.0`)
  # and set each `sha256` to `shasum -a 256 <name>-0.1.0.tar.gz`.
  homepage "https://github.com/FulecoRafa/farol"
  url "https://github.com/FulecoRafa/farol/archive/refs/tags/v0.1.0.tar.gz"
  sha256 "0000000000000000000000000000000000000000000000000000000000000000"
  license any_of: ["MIT", "Apache-2.0"]

  depends_on "rust" => :build
  # Wayland layer-shell surfaces, Hyprland, freedesktop desktop entries:
  # tessela (and so farol) does not run on macOS yet.
  depends_on :linux
  # The runner (tessela.rb, in the same tap).
  depends_on "tessela"

  # core/Cargo.toml takes tessela's SDK (sdk/rust, a member of tessela's
  # workspace) by path: the same tarball tessela.rb builds from.
  resource "tessela" do
    url "https://github.com/FulecoRafa/tessela/archive/refs/tags/v0.1.0.tar.gz"
    sha256 "0000000000000000000000000000000000000000000000000000000000000000"
  end

  # tessela.toml imports adwaita-shell's kit (components and theme).
  resource "adwaita-shell" do
    url "https://github.com/FulecoRafa/adwaita-shell/archive/refs/tags/v0.1.0.tar.gz"
    sha256 "0000000000000000000000000000000000000000000000000000000000000000"
  end

  def install
    (buildpath/"tessela").install resource("tessela")
    inreplace "core/Cargo.toml", "../../tessela/sdk/rust", "../tessela/sdk/rust"
    # The real binary in libexec; bin/farol below says where the runner and
    # the project are (it is not in ../share/farol from libexec/bin).
    system "cargo", "install", *std_cargo_args(root: libexec, path: "core")

    pkgshare.install "tessela.toml", "src"
    resource("adwaita-shell").stage { pkgshare.install "kit" }
    inreplace pkgshare/"tessela.toml" do |s|
      s.gsub! 'command = ["core/target/release/farol"]', "command = [\"#{opt_libexec}/bin/farol\"]"
      s.gsub! 'themes = ["../adwaita-shell/kit/theme.toml"]', "themes = [\"#{opt_pkgshare}/kit/theme.toml\"]"
      s.gsub! 'kit = "../adwaita-shell/kit"', "kit = \"#{opt_pkgshare}/kit\""
    end

    (bin/"farol").write_env_script libexec/"bin/farol",
      TESSELA:           "${TESSELA:-#{Formula["tessela"].opt_bin}/tessela}",
      FAROL_PROJECT_DIR: "${FAROL_PROJECT_DIR:-#{opt_pkgshare}}"
    doc.install "README.md"
  end

  def caveats
    <<~EOS
      farol runs host commands brew does not provide: xdg-open (xdg-utils),
      notify-send (libnotify) and, as a fallback, wl-copy (wl-clipboard).
      Install them with the system's package manager.

      Start it with `farol start`, and at login with `farol autostart on`
      (a systemd user unit bound to graphical-session.target).
      Open it with `farol toggle`, or let farol bind a key in Hyprland:
        farol hotkey super+alt+space
    EOS
  end

  test do
    ENV["FAROL_DB"] = testpath/"farol.db"
    system bin/"farol", "hotkey", "super+alt+space", ">"
    assert_match "super+alt+space", shell_output("#{bin}/farol hotkeys")
    assert_match "4", shell_output("#{bin}/farol query 2+2")

    # The project with its service, without a display.
    (testpath/"run").mkpath
    ENV["XDG_RUNTIME_DIR"] = testpath/"run"
    ENV["XDG_STATE_HOME"] = testpath/"state"
    ENV.delete "HYPRLAND_INSTANCE_SIGNATURE"
    system Formula["tessela"].opt_bin/"tessela", "run", opt_pkgshare,
           "--headless", "--seconds", "2", "--name", "farol-brew-test"
  end
end
