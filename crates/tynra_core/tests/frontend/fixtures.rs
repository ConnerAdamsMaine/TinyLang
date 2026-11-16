//! Source shapes for the source-pipeline diagnostic; construction is untimed.

#[path = "../preparation/sources.rs"]
mod sources;

pub(super) struct FrontendFixture {
    pub(super) name: &'static str,
    pub(super) source: String,
    pub(super) output: String,
    pub(super) iterations: u64,
}

pub(super) fn fixtures() -> [FrontendFixture; 8] {
    let [print, straightline, float_loop, functions] = sources::SOURCE_CASES.map(|case| FrontendFixture {
        name: case.name,
        source: case.source.to_owned(),
        output: case.output.to_owned(),
        iterations: 2_000,
    });
    let mut declarations = String::new();
    let mut strings = String::new();
    for index in 0..256 {
        declarations.push_str(&format!("fn sample_{index}(value) {{ let copy = value return copy + {index} }}\n"));
        strings.push_str(&format!("let item_{index} = \"value_{index}\"\n"));
    }
    declarations.push_str("print sample_0(1) + sample_255(1)\n");
    strings.push_str("print item_0 print item_255\n");
    [
        print,
        straightline,
        float_loop,
        functions,
        FrontendFixture {
            name: "line_heavy_4096",
            source: format!("{}print 42\n", "# generated source header\n".repeat(4_096)),
            output: "42\n".to_owned(),
            iterations: 256,
        },
        FrontendFixture {
            name: "declarations_256",
            source: declarations,
            output: "257\n".to_owned(),
            iterations: 128,
        },
        FrontendFixture {
            name: "constant_chain_128",
            source: format!("print {}1\n", "1 + ".repeat(127)),
            output: "128\n".to_owned(),
            iterations: 256,
        },
        FrontendFixture {
            name: "string_bindings_256",
            source: strings,
            output: "value_0\nvalue_255\n".to_owned(),
            iterations: 128,
        },
    ]
}
