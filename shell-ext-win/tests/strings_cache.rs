//! FR-002 / FR-005 — the menu the handler builds fits the modern Windows 11
//! menu's rules and degrades safely when the label cache is missing or hostile.
//!
//! The menu model is deliberately pure so the two limits that are invisible
//! until a real Explorer silently drops entries — 16 items per handler, no
//! nested sub-menus — are asserted here rather than discovered on a user's
//! machine.
//!
//! Hermetic: caches are written into a `tempfile::TempDir`; the real
//! `<data_local_dir>/shell-menu.json` is never read.

use shell_ext_win::handoff::Action;
use shell_ext_win::strings::{
    self, MenuCommand, MenuItem, Strings, CACHE_FILE, CACHE_SCHEMA_VERSION, MAX_CACHE_BYTES,
    MAX_MENU_ITEMS, MAX_MENU_PROFILES,
};

fn cache_json(profiles: usize) -> String {
    let profiles: Vec<String> = (0..profiles)
        .map(|i| format!(r#"{{"id":"p{i}","name":"Profile {i}"}}"#))
        .collect();
    format!(
        r#"{{"schema_version":{CACHE_SCHEMA_VERSION},"lang":"es",
            "labels":{{"root":"MaeclyPrint",
                       "print_one":"Imprimir 1 archivo",
                       "print_many":"Imprimir {{count}} archivos",
                       "print_with_profile":"Imprimir con «{{name}}»",
                       "configure":"Configurar trabajo…",
                       "queue":"Añadir a la cola",
                       "tooltip":"Imprime la selección"}},
            "profiles":[{}]}}"#,
        profiles.join(",")
    )
}

fn commands(items: &[MenuItem]) -> Vec<&MenuCommand> {
    items
        .iter()
        .filter_map(|item| match item {
            MenuItem::Command(command) => Some(command),
            MenuItem::Separator => None,
        })
        .collect()
}

/// Explorer drops everything past the sixteenth item a handler contributes,
/// without a word. Overflowing would lose "Configure print job…" — the one
/// entry that reaches the profiles that did not fit.
#[test]
fn fr_002_the_menu_never_exceeds_the_handler_item_budget() {
    let strings = strings::parse(&cache_json(50)).expect("a 50-profile cache is still valid");
    for count in [1usize, 2, 23, 10_000] {
        let menu = strings.menu(count);
        assert!(
            menu.len() <= MAX_MENU_ITEMS,
            "{} items for a selection of {count}",
            menu.len()
        );
        assert!(!matches!(menu.last(), Some(MenuItem::Separator)));
        assert!(!matches!(menu.first(), Some(MenuItem::Separator)));
    }
}

#[test]
fn fr_002_only_the_first_profiles_get_their_own_entry() {
    let strings = strings::parse(&cache_json(50)).unwrap();
    let menu = strings.menu(3);
    let profile_entries: Vec<&MenuCommand> = commands(&menu)
        .into_iter()
        .filter(|command| command.profile_id.is_some())
        .collect();

    assert_eq!(profile_entries.len(), MAX_MENU_PROFILES);
    assert_eq!(profile_entries[0].profile_id.as_deref(), Some("p0"));
    assert_eq!(profile_entries[0].title, "Imprimir con «Profile 0»");
    // Everything past the cap stays reachable through the dialog.
    assert!(commands(&menu)
        .iter()
        .any(|command| command.action == Action::Configure));
}

#[test]
fn fr_002_the_fixed_entries_carry_the_right_actions() {
    let menu = Strings::defaults().menu(4);
    let commands = commands(&menu);

    // First entry is the bold default: print now, no profile.
    assert!(commands[0].is_default);
    assert_eq!(commands[0].action, Action::PrintDefault);
    assert_eq!(commands[0].profile_id, None);
    assert_eq!(commands[0].title, "Print 4 files");

    let actions: Vec<Action> = commands.iter().map(|command| command.action).collect();
    assert!(actions.contains(&Action::Configure));
    assert!(actions.contains(&Action::Queue));
    // Exactly one entry is bold; two defaults in one menu is a shell bug
    // waiting to happen.
    assert_eq!(commands.iter().filter(|c| c.is_default).count(), 1);
}

#[test]
fn fr_002_the_count_is_pluralized_and_substituted() {
    let english = Strings::defaults();
    assert_eq!(english.print_title(1), "Print 1 file");
    assert_eq!(english.print_title(2), "Print 2 files");
    assert_eq!(english.print_title(1_234), "Print 1234 files");

    let spanish = strings::parse(&cache_json(0)).unwrap();
    assert_eq!(spanish.print_title(1), "Imprimir 1 archivo");
    assert_eq!(spanish.print_title(23), "Imprimir 23 archivos");
    // No leftover placeholder in any language.
    assert!(!spanish.print_title(23).contains("{count}"));
}

#[test]
fn fr_002_a_cache_with_no_profiles_has_no_dangling_separator() {
    let menu = strings::parse(&cache_json(0)).unwrap().menu(1);
    assert!(!matches!(menu.first(), Some(MenuItem::Separator)));
    // One separator, between the default action and the fixed entries.
    assert_eq!(
        menu.iter()
            .filter(|item| matches!(item, MenuItem::Separator))
            .count(),
        1
    );
}

#[test]
fn fr_002_an_unusable_cache_is_ignored_rather_than_trusted() {
    assert!(strings::parse("{not json").is_none());
    assert!(strings::parse("[]").is_none());
    // A future schema is refused, exactly like the handoff file: a menu drawn
    // from half-understood data is worse than the English default.
    assert!(strings::parse(r#"{"schema_version":99,"profiles":[]}"#).is_none());
    // A partial cache still works — every label has a compiled-in fallback.
    let partial = strings::parse(r#"{"schema_version":1,"labels":{"queue":"Encolar"}}"#).unwrap();
    assert_eq!(partial.labels.queue, "Encolar");
    assert_eq!(partial.labels.root, "MaeclyPrint");
}

#[test]
fn fr_002_a_missing_cache_falls_back_to_the_built_in_menu() {
    let dir = tempfile::tempdir().unwrap();
    let loaded = strings::load_from_dir(dir.path());
    assert_eq!(loaded, Strings::defaults());
    assert!(!loaded.menu(1).is_empty());
}

/// The cache is read while the user is waiting for a menu to appear. A
/// corrupted or deliberately huge file must cost a `stat`, not a read.
#[test]
fn fr_002_an_oversized_cache_is_not_read() {
    let dir = tempfile::tempdir().unwrap();
    let mut bloated = cache_json(1);
    bloated.push_str(&" ".repeat(MAX_CACHE_BYTES as usize + 1));
    std::fs::write(dir.path().join(CACHE_FILE), bloated).unwrap();

    assert_eq!(strings::load_from_dir(dir.path()), Strings::defaults());
}

#[test]
fn fr_002_a_valid_cache_on_disk_is_used() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(CACHE_FILE), cache_json(2)).unwrap();

    let loaded = strings::load_from_dir(dir.path());
    assert_eq!(loaded.lang, "es");
    assert_eq!(loaded.profiles.len(), 2);
    // default, separator, p0, p1, separator, configure, queue.
    assert_eq!(loaded.menu(1).len(), 7);
}

/// FR-005: the entry is only offered when something in the selection could
/// actually be printed, and never hidden on the strength of a partial look at
/// a large selection.
#[test]
fn fr_005_visibility_follows_the_supported_formats() {
    use shell_ext_win::supported::{is_supported_name, should_show, ProbeItem, PROBE_LIMIT};

    let file = |name: &str| ProbeItem {
        name: name.into(),
        is_folder: false,
    };

    assert!(is_supported_name("2026-01.PDF"));
    assert!(is_supported_name("notes.txt"));
    assert!(!is_supported_name("setup.msi"));

    assert!(should_show(&[file("a.msi"), file("b.pdf")], 2));
    assert!(!should_show(&[file("a.msi"), file("b.exe")], 2));

    let unprintable: Vec<ProbeItem> = (0..PROBE_LIMIT)
        .map(|i| file(&format!("{i}.exe")))
        .collect();
    assert!(should_show(&unprintable, 9_000));
}

/// FR-001 / FR-092: the cache the app writes (`src-tauri/src/shell_menu.rs`)
/// is one this reader accepts. The fixture is the app's exact output for a
/// fixed input — the app's own test asserts that — so a field renamed on either
/// side fails one of the two crates instead of silently falling back to the
/// English defaults on a user's machine.
#[test]
fn fr_001_reads_the_cache_the_app_writes() {
    let raw = include_str!("fixtures/shell-menu.es.json");
    let strings = strings::parse(raw).expect("the app-written cache parses");
    assert_eq!(strings.lang, "es");
    assert_eq!(strings.print_title(1), "Imprimir 1 archivo");
    assert_eq!(strings.print_title(3), "Imprimir 3 archivos");

    let items = strings.menu(2);
    let cmds = commands(&items);
    assert!(cmds
        .iter()
        .any(|c| c.title == "Imprimir con «Profile 0»" && c.profile_id.as_deref() == Some("p0")));
    assert!(cmds
        .iter()
        .any(|c| c.title == "Configurar el trabajo de impresión…"));
}
