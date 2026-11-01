use super::{
    DeclarationTarget,
    Decorator,
    DecoratorArgument,
    MAX_ARGUMENTS,
    MAX_DECORATOR_TEXT_BYTES,
    MAX_DECORATORS,
    MAX_PATH_SEGMENTS,
    stdx,
};
use crate::{ModuleCapabilities, ModulePermissions, Result, TinyOneError};

/// Definitions are compiler extensions, never executable artifact content.
/// Adding an extension here makes the same rules apply to source and artifacts.
pub(crate) struct DecoratorRegistry {
    definitions: &'static [Definition],
}

struct Definition {
    path:          &'static [&'static str],
    target:        DeclarationTarget,
    repeatability: Repeatability,
    normalize:     fn(&mut Decorator) -> Result<()>,
    validate:      fn(&Decorator) -> Result<()>,
    permissions:   fn(ModuleCapabilities, &ModulePermissions) -> Result<()>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Repeatability {
    Once,
    #[cfg(test)]
    Multiple,
}

const DEFINITIONS: &[Definition] = &[
    Definition {
        path:          &["stdx", "coprocessor"],
        target:        DeclarationTarget::Function,
        repeatability: Repeatability::Once,
        normalize:     stdx::normalize_coprocessor,
        validate:      stdx::validate_coprocessor,
        permissions:   stdx::validate_hardware_grant,
    },
    // Compiler-extension fixtures exercise the generic path without shipping
    // unrelated decorators in the production registry.
    #[cfg(test)]
    Definition {
        path:          &["tag"],
        target:        DeclarationTarget::Function,
        repeatability: Repeatability::Multiple,
        normalize:     normalize_metadata,
        validate:      accept_metadata,
        permissions:   accept_permissions,
    },
    #[cfg(test)]
    Definition {
        path:          &["test", "metadata"],
        target:        DeclarationTarget::Function,
        repeatability: Repeatability::Multiple,
        normalize:     normalize_metadata,
        validate:      accept_metadata,
        permissions:   accept_permissions,
    },
];

#[cfg(test)]
fn accept_metadata(_: &Decorator) -> Result<()> {
    Ok(())
}

#[cfg(test)]
fn normalize_metadata(_: &mut Decorator) -> Result<()> {
    Ok(())
}

#[cfg(test)]
fn accept_permissions(_: ModuleCapabilities, _: &ModulePermissions) -> Result<()> {
    Ok(())
}

impl Default for DecoratorRegistry {
    fn default() -> Self {
        Self {
            definitions: DEFINITIONS,
        }
    }
}

impl DecoratorRegistry {
    pub(crate) fn extension_namespace(import: &str) -> Option<&'static str> {
        match import {
            "stdx" => Some("stdx"),
            #[cfg(test)]
            "decorator-test-extension" => Some("test"),
            _ => None,
        }
    }

    pub(crate) fn normalize(&self, namespace: Option<&str>, mut decorator: Decorator) -> Result<Decorator> {
        verify_shape(&decorator)?;
        if decorator.path.len() > 1 {
            let alias = &decorator.path[0];
            let namespace = namespace.ok_or_else(|| {
                TinyOneError::compile(format!("Unknown decorator namespace {alias:?}; did you forget to import it?"))
            })?;
            decorator.path[0] = namespace.to_owned();
        }
        let definition = self.resolve(&decorator)?;
        (definition.normalize)(&mut decorator)?;
        Ok(decorator)
    }

    /// Validates canonical function metadata and returns its text budget charge.
    pub(crate) fn validate_function(&self, name: &str, decorators: &[Decorator]) -> Result<usize> {
        if decorators.len() > MAX_DECORATORS {
            return Err(TinyOneError::compile("Decorator count exceeds limit 16"));
        }
        let mut text_bytes = 0usize;
        for (index, decorator) in decorators.iter().enumerate() {
            text_bytes = text_bytes
                .checked_add(verify_shape(decorator)?)
                .ok_or_else(|| TinyOneError::compile("Decorator text budget overflow"))?;
            if text_bytes > MAX_DECORATOR_TEXT_BYTES {
                return Err(TinyOneError::compile("Decorator text budget exceeded"));
            }
            let definition = self.resolve(decorator)?;
            if definition.target != DeclarationTarget::Function {
                return Err(TinyOneError::compile("Decorator is not valid on a function"));
            }
            (definition.validate)(decorator)?;
            if definition.repeatability == Repeatability::Once
                && decorators[..index]
                    .iter()
                    .any(|previous| previous.path == decorator.path)
            {
                return Err(TinyOneError::compile(format!(
                    "Function {name:?} declares @{} more than once",
                    decorator.path.join(".")
                )));
            }
        }
        Ok(text_bytes)
    }

    pub(crate) fn validate_permissions(
        &self,
        decorators: &[Decorator],
        capabilities: ModuleCapabilities,
        permissions: &ModulePermissions,
    ) -> Result<()> {
        for decorator in decorators {
            (self.resolve(decorator)?.permissions)(capabilities, permissions)?;
        }
        Ok(())
    }

    fn resolve(&self, decorator: &Decorator) -> Result<&Definition> {
        self.definitions
            .iter()
            .find(|definition| {
                definition
                    .path
                    .iter()
                    .copied()
                    .eq(decorator.path.iter().map(String::as_str))
            })
            .ok_or_else(|| TinyOneError::compile(format!("Decorator @{} is not registered", decorator.path.join("."))))
    }
}

fn verify_shape(decorator: &Decorator) -> Result<usize> {
    if decorator.path.is_empty() || decorator.path.len() > MAX_PATH_SEGMENTS {
        return Err(TinyOneError::compile("Decorator path must contain 1 to 8 segments"));
    }
    if decorator.arguments.len() > MAX_ARGUMENTS {
        return Err(TinyOneError::compile("Decorator argument count exceeds limit 16"));
    }
    let mut text_bytes = 0usize;
    for segment in &decorator.path {
        add_text(&mut text_bytes, segment)?;
        verify_identifier(segment)?;
    }
    for argument in &decorator.arguments {
        match argument {
            DecoratorArgument::String(text) => add_text(&mut text_bytes, text)?,
            DecoratorArgument::Identifier(text) => {
                add_text(&mut text_bytes, text)?;
                verify_identifier(text)?;
            }
            DecoratorArgument::Integer(_) | DecoratorArgument::Bool(_) => {}
        }
    }
    Ok(text_bytes)
}

fn add_text(bytes: &mut usize, text: &str) -> Result<()> {
    *bytes = bytes
        .checked_add(text.len())
        .ok_or_else(|| TinyOneError::compile("Decorator text budget overflow"))?;
    if *bytes > MAX_DECORATOR_TEXT_BYTES {
        return Err(TinyOneError::compile("Decorator text budget exceeded"));
    }
    Ok(())
}

fn verify_identifier(text: &str) -> Result<()> {
    let mut bytes = text.bytes();
    let valid = bytes.next().is_some_and(|ch| ch == b'_' || ch.is_ascii_alphabetic())
        && bytes.all(|ch| ch == b'_' || ch.is_ascii_alphanumeric())
        && crate::syntax::lexer::keyword_kind(text).is_none();
    if !valid {
        return Err(TinyOneError::compile(format!("Invalid decorator identifier {text:?}")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Program, VerifiedProgram, compile_source_verified};

    #[test]
    fn registered_generic_decorators_attach_in_order_and_survive_the_pipeline() {
        let source = "import \"decorator-test-extension\" as ext\n@tag\n@ext.metadata(123, true, Device, \"value\")\n@tag()\nfn value() { return 1 + 2 }";
        let verified = compile_source_verified(source).expect("registered generic decorators");
        let metadata = verified.program().functions()[0].decorators();
        assert_eq!(metadata.len(), 3);
        assert_eq!(metadata[0].path(), ["tag"]);
        assert_eq!(metadata[1].path(), ["test", "metadata"]);
        assert_eq!(
            metadata[1].arguments(),
            [
                DecoratorArgument::Integer(123),
                DecoratorArgument::Bool(true),
                DecoratorArgument::Identifier("Device".into()),
                DecoratorArgument::String("value".into())
            ]
        );
        assert_eq!(metadata[2].path(), ["tag"]);
        let binary = verified.program().to_binary_artifact().expect("encode");
        let binary = VerifiedProgram::verify(Program::from_binary_artifact(&binary).expect("decode")).expect("verify");
        assert_eq!(binary.program().functions()[0].decorators(), metadata);
        let json = Program::from_artifact(verified.program().to_artifact()).expect("JSON decode");
        assert_eq!(json.functions()[0].decorators(), metadata);
    }

    #[test]
    fn fingerprints_cover_decorator_order_paths_and_every_argument_kind() {
        let source = "import \"decorator-test-extension\" as ext\n@tag\n@ext.metadata(123, true, Device, \"value\")\nfn value() { return 3 }";
        let original = compile_source_verified(source).expect("fixture").into_program();
        let fingerprint = original.fingerprint();
        let mut variants = Vec::new();
        let mut changed = original.clone();
        changed.functions[0].decorators.swap(0, 1);
        variants.push(changed);
        let mut changed = original.clone();
        changed.functions[0].decorators.pop();
        variants.push(changed);
        let mut changed = original.clone();
        changed.functions[0].decorators[0].path.push("segment".into());
        variants.push(changed);
        let mut changed = original.clone();
        changed.functions[0].decorators[0].path[0] = "other".into();
        variants.push(changed);
        for (index, argument) in [
            (0, DecoratorArgument::Integer(124)),
            (1, DecoratorArgument::Bool(false)),
            (2, DecoratorArgument::Identifier("Other".into())),
            (3, DecoratorArgument::String("changed".into())),
            (2, DecoratorArgument::String("Device".into())),
        ] {
            let mut changed = original.clone();
            changed.functions[0].decorators[1].arguments[index] = argument;
            variants.push(changed);
        }
        let mut changed = original.clone();
        changed.functions[0].decorators[1]
            .arguments
            .push(DecoratorArgument::Integer(1));
        variants.push(changed);
        for changed in variants {
            assert_ne!(changed.fingerprint(), fingerprint);
        }
    }
}
