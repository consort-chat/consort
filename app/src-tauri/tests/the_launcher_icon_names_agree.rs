// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The names a launcher icon needs, read off the packaging and checked against
//! each other.
//!
//! Three things have to carry the same name before a desktop will put Consort's
//! icon next to Consort's window:
//!
//! - the installed binary, because GTK derives `WM_CLASS` from `argv[0]`, which
//!   is why the binary is `consort` and not `consort-app`,
//! - `StartupWMClass` in the desktop entry, which is the string a shell matches
//!   a window's `WM_CLASS` against to decide which launcher it belongs to,
//! - the icon name, which has to be the stem of the files installed under
//!   `/usr/share/icons/hicolor/*/apps/`, because that is how a name becomes a
//!   picture.
//!
//! Every one of them is silent when it is wrong. The application builds, starts,
//! opens its window and works; the dock shows a grey square. Nothing in a build
//! or a test run has ever had an opinion about it, so the only way anybody found
//! out was by looking, on a desktop the change was not made on.
//!
//! There are three packaging paths and each names these files by hand:
//! `packaging/arch/PKGBUILD`, `packaging/aur/PKGBUILD`, and the Tauri bundler
//! for the .deb and the .rpm. Two hand-written recipes can drift from each other
//! as easily as either can drift from the bundler, so every recipe on disk is
//! read here rather than one of them.
//!
//! What the bundler does is not read off a built package, because building one
//! takes a compiler, a frontend and twenty minutes. It is encoded instead, from
//! `tauri-bundler`'s freedesktop module and confirmed against the .deb attached
//! to the v0.7.0 release: the icons are named after the binary, the desktop
//! entry is named after `productName`, and its contents come from
//! `bundle.linux.deb.desktopTemplate` when one is given. Those are the
//! assumptions here with the shortest shelf life, so each is tied to a file in
//! this repository that can be checked rather than left as a belief.
//!
//! This reads files. No desktop session, no bundler, no package manager and no
//! network, which is what lets it run in CI on every change instead of being
//! something somebody remembers to look at.

use std::path::{Path, PathBuf};

/// The desktop entry all three packaging paths are meant to install.
const DESKTOP_ENTRY: &str = "packaging/linux/consort.desktop";

/// Where a package puts icons for a name to be resolved through.
const ICON_DIRECTORY: &str = "/usr/share/icons/hicolor/";

/// Where a package puts desktop entries.
const APPLICATIONS_DIRECTORY: &str = "/usr/share/applications/";

#[test]
fn the_desktop_entry_launches_the_binary_the_packages_install() {
    // `Exec` is half of why the binary is named `consort`: it is the command a
    // launcher runs, and `argv[0]` is where GTK reads the window class it
    // reports from.
    assert_eq!(
        desktop_entry("Exec"),
        binary_name(),
        "the desktop entry runs a command the packages do not install"
    );
}

#[test]
fn the_window_class_is_the_one_gtk_derives_from_the_binary_name() {
    // GDK's default program class is `g_get_prgname()` with the first character
    // upper-cased and nothing else touched, and `g_get_prgname()` is the
    // basename of `argv[0]`. So a binary called `consort` reports
    // `WM_CLASS(STRING) = "consort", "Consort"`, and `StartupWMClass` has to be
    // one of that pair. It is the second, which is what was measured.
    assert_eq!(
        desktop_entry("StartupWMClass"),
        window_class(&binary_name()),
        "the window class in the desktop entry is not the one the window reports"
    );
}

#[test]
fn the_icon_name_is_the_one_the_bundler_will_install_under() {
    // The .deb and the .rpm are the packaging path with no file in this
    // repository naming their icons: `tauri-bundler` writes them to
    // `usr/share/icons/hicolor/<size>/apps/<main binary name>.png`. So the name
    // the desktop entry asks for has to be the binary's, or those two packages
    // are the ones showing a grey square.
    assert_eq!(
        desktop_entry("Icon"),
        binary_name(),
        "the icon name is not what the .deb and .rpm will install the icons under"
    );
}

#[test]
fn the_icon_is_asked_for_by_name_rather_than_by_path() {
    // A name goes through the icon theme, which is what picks the size a dock
    // wants and what lets somebody's theme override it. A path does neither.
    let icon = desktop_entry("Icon");

    assert!(
        !icon.contains('/') && !icon.contains('.'),
        "Icon={icon} is a path or a filename; it has to be a bare icon name"
    );
}

#[test]
fn every_recipe_installs_its_icons_under_the_name_the_desktop_entry_asks_for() {
    let wanted = desktop_entry("Icon");

    for recipe in recipes() {
        let icons = icon_installs(&recipe);
        assert!(
            !icons.is_empty(),
            "{} installs no icons at all",
            recipe.display()
        );

        for icon in icons {
            assert_eq!(
                stem(&icon.destination),
                wanted,
                "{} installs {}, which no desktop will find under Icon={wanted}",
                recipe.display(),
                icon.destination
            );
        }
    }
}

#[test]
fn every_recipe_installs_the_binary_under_the_name_gtk_will_report() {
    for recipe in recipes() {
        let installed = installs(&recipe)
            .into_iter()
            .find(|install| install.destination.starts_with("/usr/bin/"))
            .unwrap_or_else(|| panic!("{} installs no binary", recipe.display()));

        assert_eq!(
            file_name(&installed.destination),
            binary_name(),
            "{} installs the binary under a name the desktop entry does not run",
            recipe.display()
        );
        assert_eq!(
            file_name(&installed.source),
            binary_name(),
            "{} takes the binary from a path cargo does not build",
            recipe.display()
        );
    }
}

#[test]
fn every_recipe_installs_the_desktop_entry_the_bundler_is_given() {
    for recipe in recipes() {
        let installed = installs(&recipe)
            .into_iter()
            .find(|install| install.destination.starts_with(APPLICATIONS_DIRECTORY))
            .unwrap_or_else(|| panic!("{} installs no desktop entry", recipe.display()));

        assert_eq!(
            installed.source,
            DESKTOP_ENTRY,
            "{} installs a desktop entry the .deb and .rpm are not built from",
            recipe.display()
        );
        // The basename is an application's identity to anything that has no
        // `StartupWMClass` to go on, and it is the one part of the entry the
        // recipes choose rather than copy. The .deb calls it `Consort.desktop`
        // after `productName`; this is the same name in the spelling `argv[0]`
        // uses, and a recipe free to call it something else is a recipe free to
        // give Arch a different application.
        assert_eq!(
            file_name(&installed.destination),
            format!("{}.desktop", binary_name()),
            "{} installs the desktop entry under a name nothing else uses",
            recipe.display()
        );
    }
}

#[test]
fn the_recipes_do_not_drift_from_each_other() {
    // Two files installing the same things by hand. The reason every recipe is
    // read above rather than one of them is that a change made to one and not
    // the other is invisible until somebody installs the package that was
    // missed, so the comparison is also made directly.
    let mut recipes = recipes();
    let first = recipes.remove(0);
    let wanted = desktop_and_icon_installs(&first);

    for other in recipes {
        assert_eq!(
            desktop_and_icon_installs(&other),
            wanted,
            "{} and {} no longer install the same desktop entry and icons",
            first.display(),
            other.display()
        );
    }
}

#[test]
fn every_file_a_recipe_installs_is_in_the_repository() {
    for recipe in recipes() {
        for install in desktop_and_icon_installs(&recipe) {
            let source = repository().join(&install.source);
            assert!(
                source.is_file(),
                "{} installs {}, which is not in the repository",
                recipe.display(),
                install.source
            );
        }
    }
}

#[test]
fn the_deb_and_the_rpm_are_built_from_the_desktop_entry_the_recipes_install() {
    // Without this the bundler writes an entry from a template of its own, with
    // an empty `Categories`, no `Keywords` and a `StartupWMClass` of `{{exec}}`.
    // Consort shipped that way until this test: the Debian package and the Arch
    // package installed different files describing the same application.
    for bundle in ["deb", "rpm"] {
        let template = tauri_config()["bundle"]["linux"][bundle]["desktopTemplate"]
            .as_str()
            .unwrap_or_else(|| panic!("bundle.linux.{bundle}.desktopTemplate is not set"))
            .to_owned();

        let resolved = tauri_directory()
            .join(&template)
            .canonicalize()
            .unwrap_or_else(|error| panic!("{bundle} template {template}: {error}"));

        assert_eq!(
            resolved,
            repository().join(DESKTOP_ENTRY),
            "the {bundle} is built from a different desktop entry than the recipes install"
        );
    }
}

#[test]
fn the_desktop_entry_asks_the_bundler_to_substitute_nothing() {
    // It is a Handlebars template to the bundler now, and a doubled curly brace
    // anywhere in it, a comment included, is the start of a substitution. An
    // unclosed one fails the template, and the only build that reads it is the
    // release build, so without this the mistake is found on release day.
    let entry = read(DESKTOP_ENTRY);

    assert!(
        !entry.contains("{{"),
        "{DESKTOP_ENTRY} holds a template substitution, which the bundler will try to expand"
    );
}

#[test]
fn the_bundler_ships_no_icon_size_the_recipes_leave_out() {
    // The bundler installs every PNG in `bundle.icon`; the recipes name theirs
    // one line at a time. A size added to the config and not to the recipes is
    // a size Debian has and Arch does not, which is the drift this file exists
    // for, one directory along.
    let icons = tauri_config()["bundle"]["icon"]
        .as_array()
        .expect("bundle.icon is a list")
        .iter()
        .map(|icon| icon.as_str().expect("an icon path").to_owned())
        .filter(|icon| icon.ends_with(".png"))
        .map(|icon| format!("app/src-tauri/{icon}"))
        .collect::<Vec<_>>();
    assert!(!icons.is_empty(), "bundle.icon names no PNGs");

    for recipe in recipes() {
        let installed = icon_installs(&recipe)
            .into_iter()
            .map(|install| install.source)
            .collect::<Vec<_>>();

        for icon in &icons {
            assert!(
                installed.contains(icon),
                "the .deb ships {icon} and {} does not install it",
                recipe.display()
            );
        }
    }
}

#[test]
fn the_product_name_is_the_window_class_too() {
    // The bundler names the desktop entry it writes after `productName`, so the
    // .deb installs `Consort.desktop` where the recipes install
    // `consort.desktop`. That difference is only a spelling as long as this
    // holds: the basename of a desktop entry is an application's identity to a
    // shell with no `StartupWMClass` to go on, and two packages for one
    // application should not disagree about what it is called.
    assert_eq!(
        tauri_config()["productName"]
            .as_str()
            .expect("productName is set"),
        window_class(&binary_name()),
        "the .deb would install a desktop entry named after something else"
    );
}

#[test]
fn the_recipes_read_here_are_all_the_recipes_there_are() {
    // A test that reads one recipe leaves the other free to rot, which is why
    // both are read. A packaging path added later would be free in exactly the
    // same way, so the list is discovered rather than written down.
    let found = recipes()
        .iter()
        .map(|recipe| recipe.display().to_string())
        .collect::<Vec<_>>();

    assert_eq!(
        found,
        ["packaging/arch/PKGBUILD", "packaging/aur/PKGBUILD"],
        "a packaging recipe was added or moved, and the rest of this file has not seen it"
    );
}

/// What GTK reports as the class half of `WM_CLASS`.
///
/// GDK upper-cases the first character of the program name with
/// `g_ascii_toupper` and leaves the rest alone, so this is deliberately not a
/// Unicode-aware capitalisation. It is a copy of what the toolkit does.
fn window_class(binary_name: &str) -> String {
    let mut characters = binary_name.chars();

    match characters.next() {
        Some(first) => format!("{}{}", first.to_ascii_uppercase(), characters.as_str()),
        None => String::new(),
    }
}

/// The name the binary is installed under.
///
/// From `[[bin]]` rather than from the package name: they differ on purpose,
/// and the one that reaches `argv[0]` is this one.
fn binary_name() -> String {
    let manifest = read("app/src-tauri/Cargo.toml");
    let bin = section(&manifest, "[[bin]]").expect("app/src-tauri/Cargo.toml has a [[bin]]");

    value(bin, "name")
        .expect("the [[bin]] section names the binary")
        .to_owned()
}

/// One value out of the desktop entry.
///
/// Panics when the key is absent, because a key this file asks about and cannot
/// find is a packaging change nobody has looked at, not a reason to pass.
fn desktop_entry(key: &str) -> String {
    let entry = read(DESKTOP_ENTRY);

    entry
        .lines()
        .map(str::trim)
        .filter(|line| !line.starts_with('#'))
        .find_map(|line| line.strip_prefix(key)?.strip_prefix('='))
        .unwrap_or_else(|| panic!("{DESKTOP_ENTRY} has no {key}"))
        .trim()
        .to_owned()
}

/// `tauri.conf.json`, parsed.
fn tauri_config() -> serde_json::Value {
    serde_json::from_str(&read("app/src-tauri/tauri.conf.json")).expect("tauri.conf.json parses")
}

/// One file a recipe's `package()` puts somewhere.
#[derive(Debug, PartialEq, Eq)]
struct Install {
    /// Where it comes from, relative to the repository root.
    source: String,
    /// Where it lands, with the staging prefix removed.
    destination: String,
}

/// Every `PKGBUILD` under `packaging/`, in a stable order.
fn recipes() -> Vec<PathBuf> {
    let packaging = repository().join("packaging");
    let mut found = Vec::new();

    for entry in std::fs::read_dir(&packaging).expect("packaging/ is readable") {
        let directory = entry.expect("a readable entry").path();
        if directory.join("PKGBUILD").is_file() {
            let name = directory.file_name().expect("a named directory").to_owned();
            found.push(Path::new("packaging").join(name).join("PKGBUILD"));
        }
    }

    found.sort();
    found
}

/// Every file a recipe installs.
///
/// `install -Dm644 <source> "<destination>"`, which is the only shape either
/// recipe uses, with the line continuations joined first and `$pkgdir` dropped
/// from the destination.
fn installs(recipe: &Path) -> Vec<Install> {
    let text = read(recipe).replace("\\\n", " ");

    text.lines()
        .map(str::trim)
        .filter(|line| line.starts_with("install "))
        .filter_map(|line| {
            let mut words = line.split_whitespace().map(|word| word.trim_matches('\''));
            let destination = words.next_back()?.trim_matches('"');
            let source = words.next_back()?.trim_matches('"');

            Some(Install {
                source: source.to_owned(),
                destination: destination.strip_prefix("$pkgdir")?.to_owned(),
            })
        })
        .collect()
}

/// The icons a recipe installs.
fn icon_installs(recipe: &Path) -> Vec<Install> {
    installs(recipe)
        .into_iter()
        .filter(|install| install.destination.starts_with(ICON_DIRECTORY))
        .collect()
}

/// The desktop entry and the icons a recipe installs, which is everything this
/// file has an opinion about.
fn desktop_and_icon_installs(recipe: &Path) -> Vec<Install> {
    installs(recipe)
        .into_iter()
        .filter(|install| {
            install.destination.starts_with(ICON_DIRECTORY)
                || install.destination.starts_with(APPLICATIONS_DIRECTORY)
        })
        .collect()
}

/// The text of one section of a TOML file, up to the next header.
fn section<'a>(document: &'a str, header: &str) -> Option<&'a str> {
    let after = document.split_once(&format!("\n{header}\n"))?.1;

    Some(match after.split_once("\n[") {
        Some((body, _)) => body,
        None => after,
    })
}

/// One `key = "value"` out of a TOML section.
fn value<'a>(section: &'a str, key: &str) -> Option<&'a str> {
    section
        .lines()
        .map(str::trim)
        .find_map(|line| line.strip_prefix(key)?.trim_start().strip_prefix('='))
        .map(|value| value.trim().trim_matches('"'))
}

/// The last component of a path.
fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// The last component of a path, without its extension.
fn stem(path: &str) -> &str {
    let name = file_name(path);

    match name.rsplit_once('.') {
        Some((stem, _)) => stem,
        None => name,
    }
}

/// The directory holding `tauri.conf.json`, which is what the paths inside it
/// are relative to.
fn tauri_directory() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The repository root.
fn repository() -> PathBuf {
    tauri_directory()
        .join("../..")
        .canonicalize()
        .expect("the repository root is reachable from the manifest directory")
}

/// One file in the repository, by its path from the root.
fn read(relative: impl AsRef<Path>) -> String {
    let path = repository().join(relative);

    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}
