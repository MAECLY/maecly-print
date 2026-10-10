//! Embeds `res/shellext.rc` into the DLL.
//!
//! The `.rc` carries the side-by-side `<msix>` assembly manifest. Without it
//! Explorer refuses to load the extension out of the sparse (external-location)
//! package and `Package.Current` is null in the host process — the single most
//! common failure of this integration.
//!
//! Failure policy: a resource compiler is only available on a Windows host (or
//! a cross toolchain that ships `rc.exe`). On a developer's macOS box
//! `cargo check --target x86_64-pc-windows-msvc` must still type-check the
//! crate, so a missing `rc.exe` is a warning, not an error. CI sets
//! `PLURYPRINT_REQUIRE_RESOURCES=1`, which turns it back into a hard failure
//! so a shipped DLL can never be built without its manifest.

use std::env;

fn main() {
    println!("cargo:rerun-if-changed=res/shellext.rc");
    println!("cargo:rerun-if-changed=res/shellext.manifest");
    println!("cargo:rerun-if-env-changed=PLURYPRINT_REQUIRE_RESOURCES");
    println!("cargo:rerun-if-env-changed=MAECLYPRINT_REQUIRE_RESOURCES");

    // Non-Windows targets have no resource section at all; skip entirely so a
    // host-side `cargo test` never shells out to a compiler that cannot exist.
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let result = embed_resource::compile("res/shellext.rc", embed_resource::NONE);
    let strict = match env::var("PLURYPRINT_REQUIRE_RESOURCES") {
        Ok(v) => v == "1",
        // The pre-rename name still works, with a warning (ADR-0009).
        Err(_) => match env::var("MAECLYPRINT_REQUIRE_RESOURCES") {
            Ok(v) => {
                println!(
                    "cargo:warning=MAECLYPRINT_REQUIRE_RESOURCES is deprecated; \
                     set PLURYPRINT_REQUIRE_RESOURCES instead"
                );
                v == "1"
            }
            Err(_) => false,
        },
    };

    let outcome = if strict {
        // NotAttempted (no rc.exe) is also fatal here: a DLL without the
        // embedded <msix> manifest looks fine and silently never loads.
        result.manifest_required()
    } else {
        result.manifest_optional()
    };

    if let Err(err) = outcome {
        if strict {
            panic!("failed to embed res/shellext.rc: {err}");
        }
        println!(
            "cargo:warning=res/shellext.rc was not embedded ({err}). \
             The resulting DLL has no side-by-side <msix> manifest and Explorer \
             will not load it. This is expected for a type-check-only build on a \
             non-Windows host; set PLURYPRINT_REQUIRE_RESOURCES=1 to make it fatal."
        );
    }
}
