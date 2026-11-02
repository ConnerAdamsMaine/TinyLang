//! Retained-object workloads shared by ordinary timing and allocation counts.

use tinylang::{VerifiedProgram, compile_source_verified};

#[derive(Clone, Copy, Debug)]
pub(super) enum RetainedObject {
    Cells,
    Strings,
    Structs,
    Enums,
}

pub(super) const OBJECTS: [RetainedObject; 4] = [
    RetainedObject::Cells,
    RetainedObject::Strings,
    RetainedObject::Structs,
    RetainedObject::Enums,
];

pub(super) const COUNTS: [usize; 3] = [16, 256, 4096];

impl RetainedObject {
    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::Cells => "cells",
            Self::Strings => "strings",
            Self::Structs => "structs",
            Self::Enums => "enums",
        }
    }

    pub(super) fn prepare(self, count: usize) -> (VerifiedProgram, String) {
        assert!(count <= 4096, "retained-object fixtures must stay inside the measured size range");
        let sum = count * count.saturating_sub(1) / 2;
        let (declaration, create, read, expected_sum) = match self {
            Self::Cells => ("", "alloc(i)", "load(roots[j])", sum),
            Self::Strings => ("", "\"a persistent string\"", "len(roots[j])", "a persistent string".len() * count),
            Self::Structs => {
                ("struct Held { value, stamp }", "Held(i, 7)", "roots[j].value + roots[j].stamp", sum + 7 * count)
            }
            Self::Enums => {
                ("enum Held { Empty, Item(value) }", "Held.Item(i)", "roots[j].value + roots[j].tag", sum + count)
            }
        };
        let source = format!(
            "{declaration}
let roots = vec_new()
let i = 0
while i < {count} {{
    let value = {create}
    let ignored = push(roots, value)
    i = i + 1
}}
let total = 0
let j = 0
while j < len(roots) {{
    total = total + ({read})
    j = j + 1
}}
print len(roots)
print total"
        );
        (
            compile_source_verified(&source).expect("bounded retained-object source"),
            format!("{count}\n{expected_sum}\n"),
        )
    }
}
