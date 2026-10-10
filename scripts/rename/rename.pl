#!/usr/bin/env perl
# Mechanical MaeclyPrint -> Pluryprint text rewrite. See scripts/rename/APPLY.md.
# Rewrites, in place and line by line, the files given on the command line.
# Order matters: the specific forms run before the generic ones.
use strict;
use warnings;

$^I = '';
while (<>) {
    # Wrong URLs in the issue templates and docs: point them at the real repo.
    s{github\.com/maecly/maeclyprint}{github.com/MAECLY/maecly-print}g;
    # Bundle identifiers (three historic spellings) -> one.
    s{com\.maecly\.maeclyprint}{com.maecly.pluryprint}g;
    s{com\.maecly\.print\b}{com.maecly.pluryprint}g;
    # Two-word spelling used in test fixture paths: those tests are about a
    # path WITH a space, so the replacement keeps one.
    s{Maecly Print}{Pluryprint App}g;
    # The ObjC selector's Rust name.
    s{print_with_maecly_print}{print_with_pluryprint}g;
    # The display name, in every CamelCase identifier too.
    s{MaeclyPrint}{Pluryprint}g;
    # Lower-case identifiers (crate, binaries, prefixes, keys), but never the
    # archived design hand-off folder `claude design/maeclyprint/`.
    s{(?<!design/)maeclyprint}{pluryprint}g;
    s{maeclyprofile}{pluryprintprofile}g;
    # Env vars and constants.
    s{MAECLYPRINT}{PLURYPRINT}g;
    # Exported profile extension (import still accepts the old one).
    s{\.maecly-profile\.toml}{.pluryprint-profile.toml}g;
    # e2e browser stub globals.
    s{__MAECLY_E2E__}{__PLURY_E2E__}g;
    s{MAECLY_E2E_STUB}{PLURY_E2E_STUB}g;
    s{maeclyE2eStub}{pluryE2eStub}g;
    s{maecly-e2e}{plury-e2e}g;
    # ipptool job name in the CUPS tracking test.
    s{maecly-track-test}{pluryprint-track-test}g;
    # Atomic-write temporary next to a Chromium `Local State` (util/chromium.rs).
    s{\.maecly-tmp\b}{.pluryprint-tmp}g;
    print;
}
