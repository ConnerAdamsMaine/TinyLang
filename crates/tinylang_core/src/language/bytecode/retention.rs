//! Allocation extents reachable from immutable bytecode metadata.
//!
//! Count capacities, not live lengths, and count nested allocations separately
//! from the headers already present in their owning vector. Shared owners are
//! charged in full; these are retention-policy charges, not exclusive RSS.

use super::certificate::VerificationCertificate;
use super::program::{EnumVariantDef, Function, ModuleDef, ModuleImportDef, Program, StructDef};
use crate::{Decorator, DecoratorArgument};

pub(super) fn string_table_bytes(strings: &Vec<String>) -> usize {
    strings
        .iter()
        .fold(strings.capacity().saturating_mul(size_of::<String>()), |bytes, string| {
            bytes.saturating_add(string.capacity())
        })
}

impl Program {
    pub(super) fn allocation_bytes(&self) -> usize {
        // Exhaustive destructuring makes a new program field require a storage
        // decision here, including explicitly allocation-free scalar metadata.
        let Self {
            code,
            source: _,
            slot_count: _,
            names,
            functions,
            strings,
            structs,
            fields,
            modules,
            enum_variants,
            root_capabilities: _,
            root_permissions,
            policy_trusted: _,
            vm_settings: _,
        } = self;
        [
            code.capacity().saturating_mul(size_of::<crate::Instr>()),
            self.source_allocation_bytes(),
            string_table_bytes(names),
            table_bytes(functions, Function::allocation_bytes),
            string_table_bytes(strings),
            table_bytes(structs, StructDef::allocation_bytes),
            string_table_bytes(fields),
            table_bytes(modules, ModuleDef::allocation_bytes),
            table_bytes(enum_variants, EnumVariantDef::allocation_bytes),
            root_permissions.allocation_bytes(),
        ]
        .into_iter()
        .fold(0, usize::saturating_add)
    }
}

// Unlike slice consumers, retention accounting needs the reserved capacity.
fn table_bytes<T>(table: &Vec<T>, allocation_bytes: impl Fn(&T) -> usize) -> usize {
    table
        .iter()
        .fold(table.capacity().saturating_mul(size_of::<T>()), |bytes, entry| {
            bytes.saturating_add(allocation_bytes(entry))
        })
}

impl Function {
    fn allocation_bytes(&self) -> usize {
        let Self {
            name,
            generic_params,
            decorators,
            param_count: _,
            code,
            source: _,
            slot_count: _,
            names,
        } = self;
        name.capacity()
            .saturating_add(string_table_bytes(generic_params))
            .saturating_add(table_bytes(decorators, decorator_allocation_bytes))
            .saturating_add(code.capacity().saturating_mul(size_of::<crate::Instr>()))
            .saturating_add(string_table_bytes(names))
    }
}

fn decorator_allocation_bytes(decorator: &Decorator) -> usize {
    let Decorator { path, arguments } = decorator;
    string_table_bytes(path).saturating_add(table_bytes(arguments, |argument| match argument {
        DecoratorArgument::String(value) | DecoratorArgument::Identifier(value) => value.capacity(),
        DecoratorArgument::Integer(_) | DecoratorArgument::Bool(_) => 0,
    }))
}

impl StructDef {
    fn allocation_bytes(&self) -> usize {
        let Self { name, fields } = self;
        name.capacity().saturating_add(string_table_bytes(fields))
    }
}

impl EnumVariantDef {
    fn allocation_bytes(&self) -> usize {
        let Self {
            enum_name,
            variant_name,
            tag: _,
            fields,
        } = self;
        enum_name
            .capacity()
            .saturating_add(variant_name.capacity())
            .saturating_add(string_table_bytes(fields))
    }
}

impl ModuleDef {
    fn allocation_bytes(&self) -> usize {
        let Self {
            name,
            path,
            imports,
            exported_functions,
            exported_structs,
            capabilities: _,
            permissions,
        } = self;
        name.capacity()
            .saturating_add(path.capacity())
            .saturating_add(table_bytes(imports, ModuleImportDef::allocation_bytes))
            .saturating_add(string_table_bytes(exported_functions))
            .saturating_add(string_table_bytes(exported_structs))
            .saturating_add(permissions.allocation_bytes())
    }
}

impl ModuleImportDef {
    fn allocation_bytes(&self) -> usize {
        let Self {
            alias,
            path,
            module,
            resolved,
        } = self;
        alias
            .capacity()
            .saturating_add(path.capacity())
            .saturating_add(module.capacity())
            .saturating_add(resolved.capacity())
    }
}

impl VerificationCertificate {
    pub(super) fn allocation_bytes(&self) -> usize {
        let Self {
            chunks,
            denied_permissions,
        } = self;
        chunks
            .iter()
            .fold(size_of_val(&**chunks).saturating_add(denied_permissions.allocation_bytes()), |bytes, chunk| {
                bytes.saturating_add(size_of_val(&*chunk.blocks))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Instr, ModuleCapabilities, ModulePermissions, Op, VerifiedProgram};

    fn reserved_string() -> String {
        let mut text = String::with_capacity(4096);
        text.push('x');
        text
    }

    fn reserved_names() -> Vec<String> {
        let mut names = Vec::with_capacity(4);
        names.push(reserved_string());
        names
    }

    #[test]
    fn decorator_retention_charges_spare_tables_and_text_capacity() {
        let mut arguments = Vec::with_capacity(5);
        arguments.push(DecoratorArgument::String(reserved_string()));
        arguments.push(DecoratorArgument::Identifier(reserved_string()));
        arguments.push(DecoratorArgument::Integer(1));
        arguments.push(DecoratorArgument::Bool(true));
        let decorator = Decorator {
            path: reserved_names(),
            arguments,
        };
        let expected = string_table_bytes(&decorator.path)
            + decorator.arguments.capacity() * size_of::<DecoratorArgument>()
            + 2 * 4096;
        assert_eq!(decorator_allocation_bytes(&decorator), expected);
        let mut function = Function::new("x", 0, Vec::new(), 0);
        let before = function.allocation_bytes();
        function.decorators = Vec::with_capacity(3);
        function.decorators.push(decorator);
        assert_eq!(function.allocation_bytes(), before + 3 * size_of::<Decorator>() + expected);
    }

    #[test]
    fn nested_metadata_charges_spare_tables_and_spare_strings() {
        let strings = reserved_names();
        let string_bytes = strings.capacity() * size_of::<String>() + strings[0].capacity();
        assert_eq!(string_table_bytes(&strings), string_bytes);
        let mut program = Program::new(Vec::with_capacity(7), 0);
        program.code.push(Instr::new(Op::Halt, 0, 0));
        program.names = reserved_names();
        program.strings = reserved_names();
        program.fields = reserved_names();
        let mut expected = program.code.capacity() * size_of::<Instr>() + 3 * string_bytes;

        let mut function = Function::new(reserved_string(), 0, Vec::with_capacity(8), 0);
        function.generic_params = reserved_names();
        function.names = reserved_names();
        let function_bytes =
            function.name.capacity() + 2 * string_bytes + function.code.capacity() * size_of::<Instr>();
        program.functions = Vec::with_capacity(3);
        program.functions.push(function);
        expected += program.functions.capacity() * size_of::<Function>() + function_bytes;

        program.structs = Vec::with_capacity(2);
        program.structs.push(StructDef {
            name: reserved_string(),
            fields: reserved_names(),
        });
        expected +=
            program.structs.capacity() * size_of::<StructDef>() + program.structs[0].name.capacity() + string_bytes;

        program.enum_variants = Vec::with_capacity(3);
        program.enum_variants.push(EnumVariantDef {
            enum_name: reserved_string(),
            variant_name: reserved_string(),
            tag: 0,
            fields: reserved_names(),
        });
        expected += program.enum_variants.capacity() * size_of::<EnumVariantDef>()
            + program.enum_variants[0].enum_name.capacity()
            + program.enum_variants[0].variant_name.capacity()
            + string_bytes;

        let permissions = ModulePermissions::from_signed_manifest(
            false,
            false,
            reserved_names(),
            false,
            false,
            false,
            false,
            false,
            false,
            false,
            false,
            false,
        );
        assert_eq!(permissions.allocation_bytes(), string_bytes);
        let mut imports = Vec::with_capacity(5);
        imports.push(ModuleImportDef {
            alias: reserved_string(),
            path: reserved_string(),
            module: reserved_string(),
            resolved: reserved_string(),
        });
        let import_bytes = imports.capacity() * size_of::<ModuleImportDef>() + 4 * imports[0].alias.capacity();
        program.modules = Vec::with_capacity(4);
        program.modules.push(ModuleDef {
            name: reserved_string(),
            path: reserved_string(),
            imports,
            exported_functions: reserved_names(),
            exported_structs: reserved_names(),
            capabilities: ModuleCapabilities::all(),
            permissions,
        });
        expected += program.modules.capacity() * size_of::<ModuleDef>()
            + program.modules[0].name.capacity()
            + program.modules[0].path.capacity()
            + import_bytes
            + 3 * string_bytes;
        program.root_permissions = ModulePermissions::from_signed_manifest(
            false,
            false,
            reserved_names(),
            false,
            false,
            false,
            false,
            false,
            false,
            false,
            false,
            false,
        );
        expected += string_bytes;
        // This fixture isolates allocation accounting, not semantic metadata
        // validity. Every nested owned buffer has deliberately unused capacity.
        assert_eq!(program.allocation_bytes(), expected);
    }

    #[test]
    fn shared_verified_charge_tracks_lazy_fingerprint_without_rescanning_metadata() {
        let mut code = Vec::with_capacity(1024);
        code.push(Instr::new(Op::Halt, 0, 0));
        let program = VerifiedProgram::verify(Program::new(code, 0)).expect("fixture");
        let before = program.retained_bytes();
        let clone = program.clone();
        assert_eq!(clone.retained_bytes(), before);
        let fingerprint_len = clone.fingerprint().len();
        assert!(program.retained_bytes() >= before + fingerprint_len);
        assert_eq!(clone.retained_bytes(), program.retained_bytes());
        assert!(before >= 1024 * size_of::<Instr>());
    }
}
