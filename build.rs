extern crate bindgen;

use std::env;
use std::path::PathBuf;

use bindgen::callbacks::{ParseCallbacks, IntKind};

fn main() {
    // Tell cargo to tell rustc to link the system bzip2
    // shared library.
    println!("cargo:rustc-link-lib=ionoPi");

    // The bindgen::Builder is the main entry point
    // to bindgen, and lets you build up options for
    // the resulting bindings.
    let bindings = bindgen::Builder::default()
        // The input header we would like to generate
        // bindings for.
        .header("ionoPi.h")
        // Tell cargo to invalidate the built crate whenever any of the
        // included header files changed.
        .parse_callbacks(Box::new(Callbacks))
        // Finish the builder and generate the bindings.
        .generate()
        // Unwrap the Result and panic on failure.
        .expect("Unable to generate bindings");

    // Write the bindings to the $OUT_DIR/bindings.rs file.
    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("Couldn't write bindings!");
}

#[derive(Debug)]
struct Callbacks;

impl ParseCallbacks for Callbacks {
    fn include_file(&self, filename: &str) {
        println!("cargo:rerun-if-changed={}", filename);
    }

    fn int_macro(&self, _name: &str, _value: i64) -> Option<IntKind> {
        if _name.starts_with("TTL") {
            return Some(IntKind::I32);
        }

        if _name.starts_with("DI") {
            return Some(IntKind::I32);
        }

        if _name.starts_with("O") {
            return Some(IntKind::I32);
        }

        if _name.starts_with("INT_EDGE") {
            return Some(IntKind::I32);
        }

        let kind = match _name {
            "LED" => IntKind::I32,
            "ON" | "OFF" => IntKind::I32,
            "LOW" | "HIGH" => IntKind::I32,
            "CLOSED" | "OPEN" => IntKind::I32,
            _ => return None,
        };

        Some(kind)
    }
}