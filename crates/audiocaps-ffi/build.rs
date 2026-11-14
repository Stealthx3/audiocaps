// SPDX-License-Identifier: LGPL-2.1-or-later
//! Sets the shared library's soname to `libaudiocaps.so.<ABI major>`.

fn main() {
    // Keep in step with AC_ABI_VERSION in src/lib.rs.
    println!("cargo:rustc-cdylib-link-arg=-Wl,-soname,libaudiocaps.so.1");
    println!("cargo:rerun-if-changed=build.rs");
}
