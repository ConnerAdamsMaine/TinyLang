use super::Compiler;
use crate::language::decorators::registry::DecoratorRegistry;
use crate::language::decorators::{Decorator, DecoratorArgument, MAX_ARGUMENTS, MAX_DECORATORS, MAX_PATH_SEGMENTS};
use crate::{Result, TokenKind};

impl Compiler {
    pub(super) fn has_namespace(&self, name: &str) -> bool {
        self.namespaces.contains_key(name) || self.extension_aliases.contains_key(name)
    }

    pub(super) fn decorated_declaration(&mut self) -> Result<()> {
        let start = self.current().clone();
        let registry = DecoratorRegistry::default();
        let mut decorators = Vec::new();
        while self.current().kind == TokenKind::At {
            if decorators.len() == MAX_DECORATORS {
                return Err(
                    self.error(format!("Decorator count exceeds limit {MAX_DECORATORS}"), self.current().clone())
                );
            }
            let token = self.current().clone();
            let decorator = self.decorator_syntax()?;
            let namespace = decorator
                .path
                .first()
                .and_then(|name| self.extension_aliases.get(name))
                .copied();
            decorators.push(
                registry
                    .normalize(namespace, decorator)
                    .map_err(|error| self.error(error.to_string(), token))?,
            );
        }
        let name_offset = match self.current().kind {
            TokenKind::Fn => 1,
            TokenKind::Export if self.peek_kind(1) == Some(TokenKind::Fn) => 2,
            TokenKind::Eof => {
                return Err(self.error("Dangling decorators at end of source; expected a function declaration", start));
            }
            _ => {
                return Err(
                    self.error("Decorators are only supported on function declarations", self.current().clone())
                );
            }
        };
        // The declaration parser reports a missing or malformed function name.
        let name = self
            .tokens
            .as_slice()
            .get(name_offset)
            .map_or("", |token| token.text.as_str());
        registry
            .validate_function(name, &decorators)
            .map_err(|error| self.error(error.to_string(), start.clone()))?;
        let permission_result = if let Some(filename) = &self.module_filename {
            let state = self.shared.borrow();
            let module = state
                .modules
                .get(filename)
                .ok_or_else(|| self.error("Internal module state error", start.clone()))?;
            registry.validate_permissions(&decorators, module.capabilities, &module.permissions)
        } else {
            registry.validate_permissions(&decorators, self.root_capabilities, &self.root_permissions)
        };
        permission_result.map_err(|error| self.error(error.to_string(), start))?;
        if self.current().kind == TokenKind::Export {
            self.export_declaration(decorators)
        } else {
            self.function_definition(false, decorators)
        }
    }

    fn decorator_syntax(&mut self) -> Result<Decorator> {
        self.eat(TokenKind::At)?;
        let mut path = vec![self.eat(TokenKind::Ident)?.text];
        while self.current().kind == TokenKind::Dot {
            if path.len() == MAX_PATH_SEGMENTS {
                return Err(
                    self.error(format!("Decorator path exceeds limit {MAX_PATH_SEGMENTS}"), self.current().clone())
                );
            }
            self.eat(TokenKind::Dot)?;
            path.push(self.eat(TokenKind::Ident)?.text);
        }
        let mut arguments = Vec::new();
        if self.current().kind == TokenKind::LParen {
            self.eat(TokenKind::LParen)?;
            if self.current().kind != TokenKind::RParen {
                loop {
                    if arguments.len() == MAX_ARGUMENTS {
                        return Err(self.error(
                            format!("Decorator argument count exceeds limit {MAX_ARGUMENTS}"),
                            self.current().clone(),
                        ));
                    }
                    arguments.push(self.decorator_argument()?);
                    match self.current().kind {
                        TokenKind::RParen => break,
                        TokenKind::Comma => {
                            self.eat(TokenKind::Comma)?;
                        }
                        _ => {
                            return Err(self.error(
                                "Decorator arguments must be static literals or bare identifiers; expected ',' or ')'",
                                self.current().clone(),
                            ));
                        }
                    }
                }
            }
            self.eat(TokenKind::RParen)?;
        }
        Ok(Decorator { path, arguments })
    }

    fn decorator_argument(&mut self) -> Result<DecoratorArgument> {
        let token = self.current().clone();
        match token.kind {
            TokenKind::String => Ok(DecoratorArgument::String(self.eat(TokenKind::String)?.text)),
            TokenKind::Ident => Ok(DecoratorArgument::Identifier(self.eat(TokenKind::Ident)?.text)),
            TokenKind::True => {
                self.eat(TokenKind::True)?;
                Ok(DecoratorArgument::Bool(true))
            }
            TokenKind::False => {
                self.eat(TokenKind::False)?;
                Ok(DecoratorArgument::Bool(false))
            }
            TokenKind::Int | TokenKind::Minus => {
                let text = if token.kind == TokenKind::Minus {
                    self.eat(TokenKind::Minus)?;
                    format!("-{}", self.eat(TokenKind::Int)?.text)
                } else {
                    self.eat(TokenKind::Int)?.text
                };
                text.parse::<i64>()
                    .map(DecoratorArgument::Integer)
                    .map_err(|_| self.error("Decorator integer is outside the i64 range", token))
            }
            _ => Err(self.error("Decorator arguments must be strings, integers, booleans, or bare identifiers", token)),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use super::*;
    use crate::{CompilerSharedState, ModuleCapabilities, ModulePermissions};

    fn compiler(source: &str) -> Compiler {
        Compiler::new(
            source,
            "decorators.to",
            None,
            false,
            "",
            ModulePermissions::from_capabilities(ModuleCapabilities::all()),
            Rc::new(RefCell::new(CompilerSharedState::default())),
        )
        .expect("tokenize decorator fixture")
    }

    #[test]
    fn generic_syntax_preserves_paths_argument_types_and_order() {
        let mut compiler = compiler(
            "@plain @namespace.empty() @namespace.meta(123, \"value\", true, false, Device, -9223372036854775808) fn value() {}",
        );
        let plain = compiler.decorator_syntax().expect("unqualified decorator");
        assert_eq!(plain.path, ["plain"]);
        assert!(plain.arguments.is_empty());
        let empty = compiler.decorator_syntax().expect("empty parentheses");
        assert_eq!(empty.path, ["namespace", "empty"]);
        assert!(empty.arguments.is_empty());
        let metadata = compiler.decorator_syntax().expect("all static arguments");
        assert_eq!(metadata.path, ["namespace", "meta"]);
        assert_eq!(
            metadata.arguments,
            [
                DecoratorArgument::Integer(123),
                DecoratorArgument::String("value".into()),
                DecoratorArgument::Bool(true),
                DecoratorArgument::Bool(false),
                DecoratorArgument::Identifier("Device".into()),
                DecoratorArgument::Integer(i64::MIN)
            ]
        );
        assert_eq!(compiler.current().kind, TokenKind::Fn);
    }

    #[test]
    fn static_syntax_rejects_runtime_expressions_and_overflow() {
        for source in [
            "@meta(1 + 2)",
            "@meta(call())",
            "@meta([1])",
            "@meta({})",
            "@meta(fn() {})",
            "@meta(1.5)",
            "@meta(9223372036854775808)",
            "@meta(-9223372036854775809)",
        ] {
            let error = compiler(source)
                .decorator_syntax()
                .expect_err("invalid static argument");
            assert!(error.to_string().contains("decorators.to"), "{error}");
        }
    }

    #[test]
    fn syntax_limits_are_checked_before_collecting_more_items() {
        let path = format!("@{}", ["part"; MAX_PATH_SEGMENTS + 1].join("."));
        let arguments = format!("@meta({})", ["1"; MAX_ARGUMENTS + 1].join(","));
        for source in [path, arguments] {
            assert!(
                compiler(&source)
                    .decorator_syntax()
                    .expect_err("bounded syntax")
                    .to_string()
                    .contains("exceeds limit")
            );
        }
    }

    #[test]
    fn imported_aliases_attach_canonical_metadata_to_exported_functions() {
        let program = compiler("import \"stdx\" as ext @ext.coprocessor(Device) export fn work() { return 1 }")
            .compile()
            .expect("compile exported decorated function");
        assert_eq!(program.functions[0].coprocessor_target(), Some("Device"));
        assert_eq!(program.functions[0].decorators[0].path, ["stdx", "coprocessor"]);
        assert!(program.modules.is_empty(), "compile-time namespaces do not add runtime modules");
    }

    #[test]
    fn registered_generic_decorators_preserve_declaration_order() {
        let program = compiler(
            "import \"decorator-test-extension\" as ext @tag @ext.metadata() @ext.metadata(123, \"value\", true, Device) @tag fn work() {}",
        )
        .compile()
        .expect("compile registered generic decorators");
        let decorators = &program.functions[0].decorators;
        assert_eq!(decorators.len(), 4);
        assert_eq!(decorators[0].path, ["tag"]);
        assert_eq!(decorators[1].path, ["test", "metadata"]);
        assert!(decorators[1].arguments.is_empty());
        assert_eq!(decorators[2].path, ["test", "metadata"]);
        assert_eq!(
            decorators[2].arguments,
            [
                DecoratorArgument::Integer(123),
                DecoratorArgument::String("value".into()),
                DecoratorArgument::Bool(true),
                DecoratorArgument::Identifier("Device".into())
            ],
        );
        assert_eq!(decorators[3].path, ["tag"]);
    }

    #[test]
    fn extension_import_aliases_conflict_with_namespaces_and_declarations() {
        for suffix in [
            "import \"stdx\" as ext",
            "let ext = 1",
            "fn ext() {}",
            "struct ext {}",
            "enum ext { First }",
        ] {
            let source = format!("import \"stdx\" as ext {suffix}");
            let error = compiler(&source).compile().expect_err("namespace collision");
            assert!(error.to_string().contains("namespace"), "{error}");
        }
    }

    #[test]
    fn decorators_require_compatible_declarations_and_source_located_errors() {
        for suffix in [
            "",
            "let value = 1",
            "struct Thing {}",
            "enum Thing { First }",
            "export struct Thing {}",
        ] {
            let source = format!("import \"stdx\" as ext @ext.coprocessor(Device) {suffix}");
            let error = compiler(&source).compile().expect_err("decorators only on functions");
            assert!(error.to_string().contains("decorators.to"), "{error}");
        }
        let error = compiler("import \"stdx\" as ext fn work() { @ext.coprocessor(Device) let value = 1 }")
            .compile()
            .expect_err("decorated statement is unsupported");
        assert!(error.to_string().contains("decorators.to"), "{error}");
        let error = compiler("@stdx.coprocessor(Device) fn work() {}")
            .compile()
            .expect_err("explicit import required");
        assert!(error.to_string().contains("Unknown decorator namespace"), "{error}");
    }

    #[test]
    fn decorator_count_is_bounded_before_function_compilation() {
        let source =
            format!("import \"stdx\" as ext {} fn work() {{}}", "@ext.coprocessor(Device) ".repeat(MAX_DECORATORS + 1));
        let error = compiler(&source).compile().expect_err("bounded decorator list");
        assert!(error.to_string().contains("Decorator count exceeds limit"), "{error}");
    }
}
