# Name of the application's binary.
name := 'cosmic_tail'
# Name of the panel applet's binary. A COSMIC applet has to be its own
# executable, because cosmic-panel launches it.
applet-name := 'cosmic_tail_applet'
# The unique ID of the application.
appid := 'com.github.frozenjava.CosmicTail'

# Path to root file system, which defaults to `/`.
rootdir := ''
# The prefix for the `/usr` directory.
prefix := '/usr'
# The location of the cargo target directory.
cargo-target-dir := env('CARGO_TARGET_DIR', 'target')

# Application's appstream metadata
appdata := appid + '.metainfo.xml'
# Application's desktop entry
desktop := appid + '.desktop'
# Panel applet's desktop entry. cosmic-panel finds applets by scanning for
# entries with `X-CosmicApplet=true`.
applet-desktop := appid + 'Applet.desktop'
# Application's icon.
icon-svg := appid + '.svg'
# The applet's icon. Named `-symbolic` because that is what makes COSMIC
# Settings recolour it to the foreground instead of drawing it as-is; see the
# stock applet entries in /usr/share/applications.
applet-icon-svg := appid + 'Applet-symbolic.svg'

# Prefix for the user-local development install. `~/.local/bin` is already on
# the panel's PATH, so the panel can launch an applet installed there.
user-prefix := env('HOME') / '.local'

# Install destinations
base-dir := absolute_path(clean(rootdir / prefix))
appdata-dst := base-dir / 'share' / 'appdata' / appdata
bin-dst := base-dir / 'bin' / name
desktop-dst := base-dir / 'share' / 'applications' / desktop
applet-bin-dst := base-dir / 'bin' / applet-name
applet-desktop-dst := base-dir / 'share' / 'applications' / applet-desktop
icons-dst := base-dir / 'share' / 'icons' / 'hicolor'
icon-svg-dst := icons-dst / 'scalable' / 'apps'

# Default recipe which runs `just build-release`
default: build-release

# Runs `cargo clean`
clean:
    cargo clean

# Removes vendored dependencies
clean-vendor:
    rm -rf .cargo vendor vendor.tar

# `cargo clean` and removes vendored dependencies
clean-dist: clean clean-vendor

# Compiles with debug profile
build-debug *args:
    cargo build --locked {{args}}

# Compiles with release profile
build-release *args: (build-debug '--release' args)

# Compiles release profile with vendored dependencies
build-vendored *args: vendor-extract (build-release '--frozen --offline' args)

# Runs a clippy check
check *args:
    cargo clippy --all-features --locked {{args}} -- -W clippy::pedantic

# Runs a clippy check with JSON message format
check-json: (check '--message-format=json')

# Run the application for testing purposes
run *args:
    env RUST_BACKTRACE=full cargo run --release --locked --bin {{name}} {{args}}

# cosmic-panel normally supplies these variables; without them the applet falls
# back to a top anchor at size S. It still will not be *in* the panel — only
# cosmic-panel can do that — but the icon and popup are sized as they would be.
[doc('Run the panel applet outside the panel, for testing purposes')]
run-applet *args:
    env RUST_BACKTRACE=full \
        COSMIC_PANEL_NAME=Panel \
        COSMIC_PANEL_ANCHOR=Bottom \
        COSMIC_PANEL_SIZE=S \
        COSMIC_PANEL_SPACING=0 \
        COSMIC_PANEL_BACKGROUND=ThemeDefault \
        cargo run --release --locked --bin {{applet-name}} {{args}}

# The binaries are symlinked rather than copied, so a later `cargo build` takes
# effect without re-running this. `current_exe` resolves symlinks, so the
# applet still finds the window binary beside it in the target directory.
[doc('Install a development build into ~/.local so the panel can launch the applet')]
dev-install: build-debug
    mkdir -p {{user-prefix}}/bin {{user-prefix}}/share/applications
    ln -sf {{ absolute_path(cargo-target-dir / 'debug' / name) }} {{user-prefix}}/bin/{{name}}
    ln -sf {{ absolute_path(cargo-target-dir / 'debug' / applet-name) }} {{user-prefix}}/bin/{{applet-name}}
    install -Dm0644 {{ 'target' / 'xdgen' / 'app.desktop' }} {{user-prefix}}/share/applications/{{desktop}}
    install -Dm0644 {{ 'target' / 'xdgen' / 'applet.desktop' }} {{user-prefix}}/share/applications/{{applet-desktop}}
    # Both desktop entries say `Icon={{appid}}`, so the file has to be named
    # after the app id and live in a theme directory to resolve at all.
    install -Dm0644 {{ 'resources' / 'icons' / 'hicolor' / 'scalable' / 'apps' / 'icon.svg' }} {{user-prefix}}/share/icons/hicolor/scalable/apps/{{icon-svg}}
    install -Dm0644 {{ 'resources' / 'icons' / 'cosmic-tail-symbolic.svg' }} {{user-prefix}}/share/icons/hicolor/scalable/apps/{{applet-icon-svg}}
    -gtk-update-icon-cache -qtf {{user-prefix}}/share/icons/hicolor 2>/dev/null
    @echo 'Installed. Add the applet from Cosmic Tail settings, or COSMIC Settings > Desktop > Panel.'

# Leaves the panel configuration alone; use the app's "Remove from panel"
# button for that.
[doc('Remove the development install from ~/.local')]
dev-uninstall:
    rm -f {{user-prefix}}/bin/{{name}} {{user-prefix}}/bin/{{applet-name}}
    rm -f {{user-prefix}}/share/icons/hicolor/scalable/apps/{{icon-svg}} {{user-prefix}}/share/icons/hicolor/scalable/apps/{{applet-icon-svg}}
    rm -f {{user-prefix}}/share/applications/{{desktop}}
    rm -f {{user-prefix}}/share/applications/{{applet-desktop}}

# cosmic-panel starts an applet once and never respawns it, so killing the
# process is not enough — verified: the icon simply stays gone. What does work
# is changing the panel's applet list, which makes it relaunch everything, so
# the example removes our entry and puts it straight back.
[doc('Rebuild and make the panel relaunch the applet with the new binary')]
dev-reload: build-debug
    cargo run -q --example reload_applet
    @echo 'The panel takes a few seconds to bring the applets back.'

# Installs files
install:
    install -Dm0755 {{ cargo-target-dir / 'release' / name }} {{bin-dst}}
    install -Dm0755 {{ cargo-target-dir / 'release' / applet-name }} {{applet-bin-dst}}
    install -Dm0644 {{ 'target' / 'xdgen' / 'app.desktop' }} {{desktop-dst}}
    install -Dm0644 {{ 'target' / 'xdgen' / 'applet.desktop' }} {{applet-desktop-dst}}
    install -Dm0644 {{ 'target' / 'xdgen' / 'app.metainfo.xml' }} {{appdata-dst}}
    install -Dm0644 {{ 'resources' / 'icons' / 'hicolor' / 'scalable' / 'apps' / 'icon.svg' }} {{ icon-svg-dst / icon-svg }}
    install -Dm0644 {{ 'resources' / 'icons' / 'cosmic-tail-symbolic.svg' }} {{ icon-svg-dst / applet-icon-svg }}

# Uninstalls installed files
uninstall:
    rm {{bin-dst}} {{applet-bin-dst}} {{desktop-dst}} {{applet-desktop-dst}}
    rm {{ icon-svg-dst / icon-svg }} {{ icon-svg-dst / applet-icon-svg }}

# Vendor dependencies locally
vendor:
    mkdir -p .cargo
    cargo vendor | head -n -1 > .cargo/config.toml
    echo 'directory = "vendor"' >> .cargo/config.toml
    tar pcf vendor.tar vendor
    rm -rf vendor

# Extracts vendored dependencies
vendor-extract:
    rm -rf vendor
    tar pxf vendor.tar

# Bump cargo version, create git commit, and create tag
tag version:
    find -type f -name Cargo.toml -exec sed -i '0,/^version/s/^version.*/version = "{{version}}"/' '{}' \; -exec git add '{}' \;
    cargo check
    cargo clean
    git add Cargo.lock
    git commit -m 'release: {{version}}'
    git commit --amend
    git tag -a {{version}} -m ''

