pub(crate) mod bytecode;

#[derive(Debug)]
pub(crate) struct SourceMap {
    filename: String,
    source:   String,
}

impl SourceMap {
    pub(crate) fn new(source: impl Into<String>, filename: impl Into<String>) -> Self {
        Self {
            filename: filename.into(),
            source:   source.into(),
        }
    }

    pub(crate) fn source(&self) -> &str {
        &self.source
    }

    pub(crate) fn filename(&self) -> &str {
        &self.filename
    }

    pub(crate) fn line_col(&self, pos: usize) -> (usize, usize) {
        let pos = pos.min(self.source.len());
        // Locate diagnostics on demand, without building a line index on
        // successful compilation or fetching it during instruction dispatch.
        // Byte slices also accept diagnostic offsets inside a UTF-8 scalar.
        let prefix = &self.source.as_bytes()[..pos];
        let line = prefix.iter().filter(|&&byte| byte == b'\n').count() + 1;
        let column = prefix
            .iter()
            .rposition(|&byte| byte == b'\n')
            .map_or(pos + 1, |newline| pos - newline);
        (line, column)
    }

    pub(crate) fn format(&self, message: impl AsRef<str>, pos: usize, end: usize) -> String {
        let pos = pos.min(self.source.len());
        let (line, column) = self.line_col(pos);
        let line_start = pos - (column - 1);
        let next_line_start = self.source.as_bytes()[pos..]
            .iter()
            .position(|&byte| byte == b'\n')
            .map_or(self.source.len(), |newline| pos + newline + 1);
        let line_text = self.source[line_start..next_line_start].trim_end_matches('\n');
        let span_end = end.max(pos + 1).min(next_line_start);
        let width = span_end.saturating_sub(pos).max(1);
        let caret = format!("{}{}", " ".repeat(column.saturating_sub(1)), "^".repeat(width));
        format!("{}:{}:{}: {}\n{}\n{}", self.filename, line, column, message.as_ref(), line_text, caret)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostics_preserve_eager_line_map_results_at_every_byte_offset() {
        for source in ["", "abc", "a\n", "\n\n", "\tλ\r\n中\nlast", "a\rb", "\"🦀\"\n"] {
            let map = SourceMap::new(source, "source.to");
            let mut starts = vec![0];
            for (index, byte) in source.bytes().enumerate() {
                if byte == b'\n' {
                    starts.push(index + 1);
                }
            }
            for input_pos in (0..=source.len() + 2).chain([usize::MAX]) {
                let pos = input_pos.min(source.len());
                let line_index = starts
                    .iter()
                    .rposition(|&start| start <= pos)
                    .expect("initial line exists");
                let column = pos - starts[line_index] + 1;
                let next_start = starts.get(line_index + 1).copied().unwrap_or(source.len());
                let line_text = source[starts[line_index]..next_start].trim_end_matches('\n');
                assert_eq!(map.line_col(input_pos), (line_index + 1, column));

                for end in [0, pos, pos + 1, source.len(), usize::MAX] {
                    let width = end.max(pos + 1).min(next_start).saturating_sub(pos).max(1);
                    let expected = format!(
                        "source.to:{}:{column}: message\n{line_text}\n{}{}",
                        line_index + 1,
                        " ".repeat(column - 1),
                        "^".repeat(width),
                    );
                    assert_eq!(
                        map.format("message", input_pos, end),
                        expected,
                        "source={source:?}, pos={input_pos}, end={end}"
                    );
                }
            }
        }
    }

    #[test]
    fn source_owner_has_no_eager_line_table() {
        assert_eq!(size_of::<SourceMap>(), 2 * size_of::<String>());
    }

    #[test]
    fn lexer_errors_preserve_utf8_and_crlf_byte_spans() {
        for (source, expected) in [
            ("print \"\\é\"", "<source>:1:9: Unknown string escape \\é\nprint \"\\é\"\n        ^^"),
            ("\r\n  $\r\n", "<source>:2:3: Unexpected character '$'\n  $\r\n  ^"),
        ] {
            let error = crate::lex_source(source).expect_err("malformed source");
            assert_eq!(error.to_string(), expected);
        }
        assert_eq!(crate::lex_source("").expect("empty source"), 1);
    }

    #[test]
    #[ignore = "diagnostic source ownership; run alone with --ignored --nocapture"]
    fn report_source_map_storage() {
        let source = "# generated source header\n".repeat(4096);
        let map = SourceMap::new(source, "source.to");
        eprintln!(
            "source map: header_bytes={}, text_capacity={}, filename_capacity={}, line_index_capacity=0",
            size_of::<SourceMap>(),
            map.source.capacity(),
            map.filename.capacity(),
        );
    }
}
