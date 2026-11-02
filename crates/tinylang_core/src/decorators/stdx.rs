use super::{Decorator, DecoratorArgument};
use crate::{ModuleCapabilities, ModuleCapability, ModulePermissions, Result, TinyLangError};

pub(super) fn validate_hardware_grant(capabilities: ModuleCapabilities, permissions: &ModulePermissions) -> Result<()> {
    if !capabilities.allows(ModuleCapability::Hardware)
        || !permissions.capabilities().allows(ModuleCapability::Hardware)
        || !permissions.hardware_access()
    {
        return Err(TinyLangError::compile(
            "Coprocessor placement requires an externally granted hardware capability grant (Hardware)",
        ));
    }
    Ok(())
}

pub(super) fn is_coprocessor(decorator: &Decorator) -> bool {
    matches!(decorator.path.as_slice(), [namespace, name] if namespace == "stdx" && name == "coprocessor")
}

pub(super) fn normalize_coprocessor(decorator: &mut Decorator) -> Result<()> {
    match decorator.arguments.as_mut_slice() {
        [DecoratorArgument::Identifier(target)] => {
            let target = std::mem::take(target);
            decorator.arguments[0] = DecoratorArgument::String(target);
        }
        [DecoratorArgument::String(_)] => {}
        [_] => {
            return Err(TinyLangError::compile("Decorator @stdx.coprocessor target must be an identifier or string"));
        }
        _ => return Err(TinyLangError::compile("Decorator @stdx.coprocessor expects exactly 1 argument")),
    }
    validate_coprocessor(decorator)
}

pub(super) fn validate_coprocessor(decorator: &Decorator) -> Result<()> {
    match decorator.arguments.as_slice() {
        [DecoratorArgument::String(target)] if !target.trim().is_empty() => Ok(()),
        [DecoratorArgument::String(_)] => {
            Err(TinyLangError::compile("Decorator @stdx.coprocessor target must not be empty"))
        }
        [_] => Err(TinyLangError::compile("Decorator @stdx.coprocessor requires a canonical string target")),
        _ => Err(TinyLangError::compile("Decorator @stdx.coprocessor expects exactly 1 argument")),
    }
}
