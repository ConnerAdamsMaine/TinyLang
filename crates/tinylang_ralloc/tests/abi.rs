use core::ffi::c_void;

use ralloc::{ralloc_aligned_alloc, ralloc_calloc, ralloc_free, ralloc_malloc, ralloc_realloc};

/// Pins the exported C ABI symbol set of the standalone `ralloc` CDyLib: the
/// five documented malloc-family entry points, no more and no less.
#[test]
fn cdylib_exports_exactly_the_documented_malloc_family() {
    let source = include_str!("../src/ralloc.rs");
    let mut found: Vec<String> = source
        .split("extern \"C\" fn ")
        .skip(1)
        .map(|tail| {
            tail.chars()
                .take_while(|character| *character == '_' || character.is_ascii_alphanumeric())
                .collect()
        })
        .collect();
    found.sort();
    found.dedup();

    let expected = [
        "ralloc_aligned_alloc",
        "ralloc_calloc",
        "ralloc_free",
        "ralloc_malloc",
        "ralloc_realloc",
    ];
    let unexpected: Vec<&String> = found
        .iter()
        .filter(|symbol| !expected.contains(&symbol.as_str()))
        .collect();
    assert!(
        unexpected.is_empty() && found.len() == expected.len(),
        "ralloc C ABI symbol drift; unexpected: {unexpected:?}, found: {found:?}"
    );
}

#[test]
fn malloc_family_symbols_have_c_allocator_shapes() {
    let malloc_fn: unsafe extern "C" fn(usize) -> *mut c_void = ralloc_malloc;
    let aligned_alloc_fn: unsafe extern "C" fn(usize, usize) -> *mut c_void = ralloc_aligned_alloc;
    let free_fn: unsafe extern "C" fn(*mut c_void) = ralloc_free;
    let calloc_fn: unsafe extern "C" fn(usize, usize) -> *mut c_void = ralloc_calloc;
    let realloc_fn: unsafe extern "C" fn(*mut c_void, usize) -> *mut c_void = ralloc_realloc;

    let _ = (malloc_fn, aligned_alloc_fn, free_fn, calloc_fn, realloc_fn);
}
