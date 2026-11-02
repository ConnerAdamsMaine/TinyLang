use std::collections::{HashMap, HashSet};
use std::num::NonZeroU32;

use super::certificate::{ChunkCertificate, PermissionSetId, VerificationCertificate};
use super::control_flow::{BlockStarts, InstructionFlow, StackDepth};
use crate::decorators::registry::DecoratorRegistry;
use crate::{BUILTINS, EnumVariantDef, Function, Instr, Op, Program, Result, StructDef, TinyLangError};

const MAX_VERIFIER_STEPS: usize = 10_000_000;
const MAX_STACK_DEPTH: i64 = 65_536;
const MAX_VERIFIER_FUNCTIONS: usize = 4_096;
const MAX_VERIFIER_TOTAL_OPS: usize = 262_144;
const MAX_VERIFIER_SLOT_COUNT: usize = 65_536;
const MAX_VERIFIER_NAMES: usize = 65_536;
const MAX_VERIFIER_STRINGS: usize = 65_536;
const MAX_VERIFIER_FIELDS: usize = 65_536;
const MAX_VERIFIER_STRUCTS: usize = 4_096;
const MAX_VERIFIER_STRUCT_FIELDS: usize = 256;
const MAX_VERIFIER_ENUM_VARIANTS: usize = 4_096;
const MAX_VERIFIER_ENUM_VARIANT_FIELDS: usize = 256;
const MAX_VERIFIER_MODULES: usize = 256;
const MAX_VERIFIER_MODULE_IMPORTS: usize = 4_096;
const MAX_VERIFIER_MODULE_EXPORTS: usize = 4_096;
const MAX_VERIFIER_TEXT_BYTES: usize = 1024 * 1024;

pub struct BytecodeVerifier;

struct VerificationContext<'a> {
    functions:         &'a [Function],
    strings:           &'a [String],
    structs:           &'a [StructDef],
    fields:            &'a [String],
    enum_variants:     &'a [EnumVariantDef],
    global_slot_count: usize,
    modules:           ModuleGraph,
}

#[derive(Debug)]
struct ModuleGraph {
    function_owners:     Vec<Option<usize>>,
    struct_owners:       Vec<Option<usize>>,
    enum_variant_owners: Vec<Option<usize>>,
    exported_functions:  HashSet<usize>,
    exported_structs:    HashSet<usize>,
    imports:             Vec<HashSet<usize>>,
}

impl ModuleGraph {
    fn build(program: &Program) -> Result<Self> {
        let mut modules_by_name = HashMap::new();
        for (module_index, module) in program.modules.iter().enumerate() {
            verify_identifier("module name", &module.name)?;
            if module.path != module.name {
                return Err(TinyLangError::compile(format!(
                    "Verifier: module {:?} has inconsistent logical path {:?}",
                    module.name, module.path
                )));
            }
            if modules_by_name.insert(module.name.as_str(), module_index).is_some() {
                return Err(TinyLangError::compile(format!("Verifier: duplicate module name {:?}", module.name)));
            }
        }

        let function_names =
            unique_named_indexes("function", program.functions.iter().map(|function| function.name.as_str()))?;
        let struct_names = unique_named_indexes("struct", program.structs.iter().map(|item| item.name.as_str()))?;
        let function_owners = program
            .functions
            .iter()
            .map(|function| owner_for_name("function", &function.name, &modules_by_name))
            .collect::<Result<Vec<_>>>()?;
        let struct_owners = program
            .structs
            .iter()
            .map(|item| owner_for_name("struct", &item.name, &modules_by_name))
            .collect::<Result<Vec<_>>>()?;
        let enum_variant_owners = program
            .enum_variants
            .iter()
            .map(|item| owner_for_name("enum", &item.enum_name, &modules_by_name))
            .collect::<Result<Vec<_>>>()?;

        let mut exported_functions = HashSet::new();
        let mut exported_structs = HashSet::new();
        let mut imports = vec![HashSet::new(); program.modules.len()];
        for (module_index, module) in program.modules.iter().enumerate() {
            let mut aliases = HashSet::new();
            for import in &module.imports {
                verify_identifier("module import alias", &import.alias)?;
                verify_identifier("module import target", &import.module)?;
                verify_identifier("resolved module import target", &import.resolved)?;
                if !aliases.insert(import.alias.as_str()) {
                    return Err(TinyLangError::compile(format!(
                        "Verifier: duplicate import alias {:?} in module {:?}",
                        import.alias, module.name
                    )));
                }
                if import.module != import.resolved {
                    return Err(TinyLangError::compile(format!(
                        "Verifier: import {:?} in module {:?} resolves inconsistently",
                        import.alias, module.name
                    )));
                }
                let target = modules_by_name.get(import.resolved.as_str()).copied().ok_or_else(|| {
                    TinyLangError::compile(format!(
                        "Verifier: import {:?} in module {:?} targets unknown module {:?}",
                        import.alias, module.name, import.resolved
                    ))
                })?;
                if target == module_index {
                    return Err(TinyLangError::compile(format!("Verifier: module {:?} imports itself", module.name)));
                }
                imports[module_index].insert(target);
            }

            let mut seen_exports = HashSet::new();
            for export in &module.exported_functions {
                verify_identifier("module function export", export)?;
                if !seen_exports.insert(export.as_str()) {
                    return Err(TinyLangError::compile(format!(
                        "Verifier: duplicate function export {:?} in module {:?}",
                        export, module.name
                    )));
                }
                let full_name = format!("{}.{}", module.name, export);
                let function_index = function_names.get(full_name.as_str()).copied().ok_or_else(|| {
                    TinyLangError::compile(format!(
                        "Verifier: module {:?} exports missing function {:?}",
                        module.name, export
                    ))
                })?;
                exported_functions.insert(function_index);
            }

            seen_exports.clear();
            for export in &module.exported_structs {
                verify_identifier("module struct export", export)?;
                if !seen_exports.insert(export.as_str()) {
                    return Err(TinyLangError::compile(format!(
                        "Verifier: duplicate struct export {:?} in module {:?}",
                        export, module.name
                    )));
                }
                let full_name = format!("{}.{}", module.name, export);
                let struct_index = struct_names.get(full_name.as_str()).copied().ok_or_else(|| {
                    TinyLangError::compile(format!(
                        "Verifier: module {:?} exports missing struct {:?}",
                        module.name, export
                    ))
                })?;
                exported_structs.insert(struct_index);
            }
        }
        reject_module_cycles(&program.modules, &imports)?;

        Ok(Self {
            function_owners,
            struct_owners,
            enum_variant_owners,
            exported_functions,
            exported_structs,
            imports,
        })
    }

    fn caller_owner(&self, caller_function: Option<usize>) -> Option<usize> {
        caller_function.and_then(|index| self.function_owners.get(index).copied().flatten())
    }

    fn can_access(&self, caller_function: Option<usize>, target_owner: Option<usize>, exported: bool) -> bool {
        let caller_owner = self.caller_owner(caller_function);
        match (caller_owner, target_owner) {
            (None, None) => true,
            (None, Some(_)) => exported,
            (Some(_), None) => false,
            (Some(caller), Some(target)) if caller == target => true,
            (Some(caller), Some(target)) => exported && self.imports[caller].contains(&target),
        }
    }

    fn can_call(&self, caller_function: Option<usize>, target: usize) -> bool {
        self.can_access(caller_function, self.function_owners[target], self.exported_functions.contains(&target))
    }

    fn can_construct(&self, caller_function: Option<usize>, target: usize) -> bool {
        self.can_access(caller_function, self.struct_owners[target], self.exported_structs.contains(&target))
    }

    fn can_construct_enum(&self, caller_function: Option<usize>, target: usize) -> bool {
        let target_owner = self.enum_variant_owners[target];
        let caller_owner = self.caller_owner(caller_function);
        (target_owner.is_none() && caller_owner.is_none()) || target_owner == caller_owner
    }
}

impl BytecodeVerifier {
    /// Verifies that `program` is a valid, safely executable bytecode program.
    ///
    /// # Errors
    ///
    /// Returns an error if the program violates any safety or structural
    /// invariant: a verification budget is exceeded, the module dependency
    /// graph cannot be built, or any chunk fails its static checks (e.g.
    /// invalid opcodes, stack/slot misuse, or capability ownership violations).
    pub fn verify(program: &Program) -> Result<()> {
        Self::verify_with_certificate(program).map(drop)
    }

    pub(crate) fn verify_with_certificate(program: &Program) -> Result<VerificationCertificate> {
        Self::verify_program_budget(program)?;
        let modules = ModuleGraph::build(program)?;
        let registry = DecoratorRegistry::default();
        for (index, function) in program.functions.iter().enumerate() {
            if function.decorators.is_empty() {
                continue;
            }
            let (capabilities, permissions) = modules.function_owners[index].map_or(
                (program.root_capabilities, &program.root_permissions),
                |owner| {
                    let module = &program.modules[owner];
                    (module.capabilities, &module.permissions)
                },
            );
            registry
                .validate_permissions(&function.decorators, capabilities, permissions)
                .map_err(|error| TinyLangError::compile(format!("Verifier: function {:?}: {error}", function.name)))?;
        }
        let context = VerificationContext {
            functions: &program.functions,
            strings: &program.strings,
            structs: &program.structs,
            fields: &program.fields,
            enum_variants: &program.enum_variants,
            global_slot_count: program.slot_count,
            modules,
        };
        let mut chunks = Vec::with_capacity(program.functions.len() + 1);
        chunks.push(Self::verify_chunk("main", &program.code, program.slot_count, &context, Op::Halt, None)?);
        for (index, function) in program.functions.iter().enumerate() {
            chunks.push(Self::verify_chunk(
                &format!("function {:?} (index {index})", function.name),
                &function.code,
                function.slot_count,
                &context,
                Op::Return,
                Some(index),
            )?);
        }
        Ok(VerificationCertificate {
            chunks:             chunks.into_boxed_slice(),
            denied_permissions: crate::ModulePermissions::none(),
        })
    }

    fn verify_program_budget(program: &Program) -> Result<()> {
        reject_over_limit("slot_count", program.slot_count, MAX_VERIFIER_SLOT_COUNT)?;
        verify_string_list("names", &program.names, MAX_VERIFIER_NAMES)?;
        verify_string_list("strings", &program.strings, MAX_VERIFIER_STRINGS)?;
        verify_string_list("fields", &program.fields, MAX_VERIFIER_FIELDS)?;
        reject_over_limit("struct count", program.structs.len(), MAX_VERIFIER_STRUCTS)?;
        reject_over_limit("module count", program.modules.len(), MAX_VERIFIER_MODULES)?;
        if program.functions.len() > MAX_VERIFIER_FUNCTIONS {
            return Err(TinyLangError::compile(format!(
                "Verifier: function count {} exceeds limit {MAX_VERIFIER_FUNCTIONS}",
                program.functions.len()
            )));
        }
        let mut total_ops = program.code.len();
        let registry = DecoratorRegistry::default();
        let mut decorator_text_bytes = 0usize;
        for function in &program.functions {
            let function_decorator_bytes = registry.validate_function(&function.name, &function.decorators)?;
            decorator_text_bytes = decorator_text_bytes
                .checked_add(function_decorator_bytes)
                .ok_or_else(|| TinyLangError::compile("Verifier: decorator text byte count overflow"))?;
            reject_over_limit("decorator text bytes", decorator_text_bytes, MAX_VERIFIER_TEXT_BYTES)?;
            reject_over_limit(
                &format!("function {:?} name bytes", function.name),
                function.name.len(),
                MAX_VERIFIER_TEXT_BYTES,
            )?;
            verify_string_list(
                &format!("function {:?} generic parameters", function.name),
                &function.generic_params,
                MAX_VERIFIER_NAMES,
            )?;
            reject_over_limit(
                &format!("function {:?} slot_count", function.name),
                function.slot_count,
                MAX_VERIFIER_SLOT_COUNT,
            )?;
            verify_string_list(&format!("function {:?} names", function.name), &function.names, MAX_VERIFIER_NAMES)?;
            if function.param_count > function.slot_count {
                return Err(TinyLangError::compile(format!(
                    "Verifier: function {:?} has {} parameter(s) but only {} slot(s)",
                    function.name, function.param_count, function.slot_count
                )));
            }
            total_ops = total_ops
                .checked_add(function.code.len())
                .ok_or_else(|| TinyLangError::compile("Verifier: total instruction count overflow"))?;
        }
        for item in &program.structs {
            reject_over_limit(&format!("struct {:?} name bytes", item.name), item.name.len(), MAX_VERIFIER_TEXT_BYTES)?;
            verify_string_list(&format!("struct {:?} fields", item.name), &item.fields, MAX_VERIFIER_STRUCT_FIELDS)?;
        }
        reject_over_limit("enum variant count", program.enum_variants.len(), MAX_VERIFIER_ENUM_VARIANTS)?;
        for item in &program.enum_variants {
            reject_over_limit(
                &format!("enum {:?} name bytes", item.enum_name),
                item.enum_name.len(),
                MAX_VERIFIER_TEXT_BYTES,
            )?;
            reject_over_limit(
                &format!("enum variant {:?} name bytes", item.variant_name),
                item.variant_name.len(),
                MAX_VERIFIER_TEXT_BYTES,
            )?;
            verify_string_list(
                &format!("enum {:?} variant {:?} fields", item.enum_name, item.variant_name),
                &item.fields,
                MAX_VERIFIER_ENUM_VARIANT_FIELDS,
            )?;
        }
        for module in &program.modules {
            reject_over_limit(
                &format!("module {:?} name bytes", module.name),
                module.name.len(),
                MAX_VERIFIER_TEXT_BYTES,
            )?;
            reject_over_limit(
                &format!("module {:?} path bytes", module.name),
                module.path.len(),
                MAX_VERIFIER_TEXT_BYTES,
            )?;
            reject_over_limit(
                &format!("module {:?} imports", module.name),
                module.imports.len(),
                MAX_VERIFIER_MODULE_IMPORTS,
            )?;
            for import in &module.imports {
                for (label, value) in [
                    ("alias", &import.alias),
                    ("path", &import.path),
                    ("module", &import.module),
                    ("resolved module", &import.resolved),
                ] {
                    reject_over_limit(
                        &format!("module {:?} import {label} bytes", module.name),
                        value.len(),
                        MAX_VERIFIER_TEXT_BYTES,
                    )?;
                }
            }
            verify_string_list(
                &format!("module {:?} function exports", module.name),
                &module.exported_functions,
                MAX_VERIFIER_MODULE_EXPORTS,
            )?;
            verify_string_list(
                &format!("module {:?} struct exports", module.name),
                &module.exported_structs,
                MAX_VERIFIER_MODULE_EXPORTS,
            )?;
        }
        if total_ops > MAX_VERIFIER_TOTAL_OPS {
            return Err(TinyLangError::compile(format!(
                "Verifier: total instruction count {total_ops} exceeds limit {MAX_VERIFIER_TOTAL_OPS}"
            )));
        }
        Ok(())
    }

    fn verify_chunk(
        chunk_name: &str,
        code: &[Instr],
        slot_count: usize,
        context: &VerificationContext<'_>,
        final_op: Op,
        caller_function: Option<usize>,
    ) -> Result<ChunkCertificate> {
        if code.last().map(|instr| instr.op) != Some(final_op) {
            let got = code.last().map_or("nothing", |instr| instr.op.name());
            return Err(TinyLangError::compile(format!(
                "Verifier: {chunk_name} must end with {}, got {got}",
                final_op.name()
            )));
        }
        let mut block_starts = BlockStarts::new(code.len())?;
        for (pc, instr) in code.iter().copied().enumerate() {
            let flow =
                Self::verify_instruction_operands(chunk_name, code, slot_count, context, caller_function, pc, instr)?;
            block_starts.record(pc, flow)?;
        }
        let mut seen = vec![None; code.len()];
        let mut todo = Vec::new();
        visit(&mut seen, &mut todo, code, 0, StackDepth::ZERO, 0, chunk_name)?;
        let mut steps: usize = 0;
        let mut max_stack_depth = 0;
        while let Some((pc, depth)) = todo.pop() {
            max_stack_depth = max_stack_depth.max(depth.0.get() - 1);
            steps += 1;
            if steps > MAX_VERIFIER_STEPS {
                return Err(TinyLangError::compile(format!(
                    "Verifier: {chunk_name} exceeded step limit ({MAX_VERIFIER_STEPS})"
                )));
            }
            let instr = code.get(pc).copied().ok_or_else(|| {
                TinyLangError::compile(format!("Verifier: internal invalid instruction {pc} in {chunk_name}"))
            })?;
            let op = instr.op;
            let arg = instr.arg;
            let arg2 = instr.arg2;
            // All immutable operands and arities, including unreachable
            // instructions, were checked by the preceding static pass.
            match op {
                Op::Jump => visit(&mut seen, &mut todo, code, arg, depth, pc, chunk_name)?,
                Op::JumpIfZero => {
                    let depth = next_depth(pc, depth, -1, chunk_name)?;
                    visit(&mut seen, &mut todo, code, arg, depth, pc, chunk_name)?;
                    visit(&mut seen, &mut todo, code, next_pc(pc)?, depth, pc, chunk_name)?;
                }
                Op::Call | Op::MakeStruct | Op::MakeEnum | Op::Builtin => {
                    visit(
                        &mut seen,
                        &mut todo,
                        code,
                        next_pc(pc)?,
                        next_depth_after_popping_to_one(pc, depth, arg2, chunk_name)?,
                        pc,
                        chunk_name,
                    )?;
                }
                Op::CallValue => {
                    let inputs = arg
                        .checked_add(1)
                        .ok_or_else(|| TinyLangError::compile("Verifier: call arity overflow"))?;
                    visit(
                        &mut seen,
                        &mut todo,
                        code,
                        next_pc(pc)?,
                        next_depth_after_popping_to_one(pc, depth, inputs, chunk_name)?,
                        pc,
                        chunk_name,
                    )?;
                }
                Op::MakeArray => {
                    visit(
                        &mut seen,
                        &mut todo,
                        code,
                        next_pc(pc)?,
                        next_depth_after_popping_to_one(pc, depth, arg, chunk_name)?,
                        pc,
                        chunk_name,
                    )?;
                }
                Op::Return => {
                    if depth.value() != 1 {
                        return Err(TinyLangError::compile(format!(
                            "Verifier: RETURN in {chunk_name} requires one value, got {depth}"
                        )));
                    }
                }
                Op::Halt => {
                    if depth.value() != 0 {
                        return Err(TinyLangError::compile(format!(
                            "Verifier: HALT in {chunk_name} requires empty stack, got {depth}"
                        )));
                    }
                }
                _ => {
                    let effect = stack_effect(op).ok_or_else(|| {
                        TinyLangError::compile(format!("Verifier: unknown opcode {op:?} at index {pc} in {chunk_name}"))
                    })?;
                    visit(
                        &mut seen,
                        &mut todo,
                        code,
                        next_pc(pc)?,
                        effect.apply(pc, depth, chunk_name)?,
                        pc,
                        chunk_name,
                    )?;
                }
            }
        }
        // Every resulting depth is visited, including terminal instructions.
        // The verifier's 65,536 bound is representable on supported targets.
        let permissions = match context.modules.caller_owner(caller_function) {
            Some(owner) => PermissionSetId::Module(owner),
            None => PermissionSetId::Root,
        };
        Ok(ChunkCertificate {
            max_stack_depth: max_stack_depth as usize,
            permissions,
            blocks: block_starts.finish(&seen),
        })
    }

    fn verify_instruction_operands(
        chunk_name: &str,
        code: &[Instr],
        slot_count: usize,
        context: &VerificationContext<'_>,
        caller_function: Option<usize>,
        pc: usize,
        instr: Instr,
    ) -> Result<InstructionFlow> {
        let op = instr.op;
        let arg = instr.arg;
        let arg2 = instr.arg2;
        if matches!(op, Op::Load | Op::Store) && checked_index(arg, slot_count).is_err() {
            return Err(TinyLangError::compile(format!(
                "Verifier: invalid slot {arg} at instruction {pc} in {chunk_name}"
            )));
        }
        if op == Op::LoadGlobal && checked_index(arg, context.global_slot_count).is_err() {
            return Err(TinyLangError::compile(format!(
                "Verifier: invalid global slot {arg} at instruction {pc} in {chunk_name}"
            )));
        }
        if op == Op::LoadGlobal && context.modules.caller_owner(caller_function).is_some() {
            return Err(TinyLangError::compile(format!(
                "Verifier: module function in {chunk_name} cannot access root global slots"
            )));
        }
        if op == Op::PushString && checked_index(arg, context.strings.len()).is_err() {
            return Err(TinyLangError::compile(format!(
                "Verifier: invalid string index {arg} at instruction {pc} in {chunk_name}"
            )));
        }
        if op == Op::PushFunction {
            let function_index = checked_index(arg, context.functions.len()).map_err(|_| {
                TinyLangError::compile(format!(
                    "Verifier: invalid function index {arg} at instruction {pc} in {chunk_name}"
                ))
            })?;
            if !context.modules.can_call(caller_function, function_index) {
                return Err(module_access_error("function", &context.functions[function_index].name, chunk_name, pc));
            }
        }
        if op == Op::CallValue && arg < 0 {
            return Err(TinyLangError::compile(format!(
                "Verifier: negative call arity {arg} at instruction {pc} in {chunk_name}"
            )));
        }
        if op == Op::CallValue && usize::try_from(arg).is_err() {
            return Err(TinyLangError::compile(format!(
                "Verifier: invalid call arity {arg} at instruction {pc} in {chunk_name}"
            )));
        }
        if matches!(op, Op::GetField | Op::SetField) && checked_index(arg, context.fields.len()).is_err() {
            return Err(TinyLangError::compile(format!(
                "Verifier: invalid field index {arg} at instruction {pc} in {chunk_name}"
            )));
        }
        let flow = InstructionFlow::parse(instr, pc, code.len(), chunk_name)?;
        if op == Op::Call {
            let function_index = checked_index(arg, context.functions.len()).map_err(|_| {
                TinyLangError::compile(format!(
                    "Verifier: invalid function index {arg} at instruction {pc} in {chunk_name}"
                ))
            })?;
            let arg_count = usize::try_from(arg2).map_err(|_| {
                TinyLangError::compile(format!(
                    "Verifier: invalid function arity {arg2} at instruction {pc} in {chunk_name}"
                ))
            })?;
            let function = &context.functions[function_index];
            if !context.modules.can_call(caller_function, function_index) {
                return Err(module_access_error("function", &function.name, chunk_name, pc));
            }
            if arg_count != function.param_count {
                return Err(TinyLangError::compile(format!(
                    "Function {:?} expects {} argument(s), got {arg2} at instruction {pc} in {chunk_name}",
                    function.name, function.param_count
                )));
            }
        }
        if op == Op::MakeArray && arg < 0 {
            return Err(TinyLangError::compile(format!(
                "Verifier: negative array arity {arg} at instruction {pc} in {chunk_name}"
            )));
        }
        if op == Op::MakeStruct {
            let struct_index = checked_index(arg, context.structs.len()).map_err(|_| {
                TinyLangError::compile(format!(
                    "Verifier: invalid struct index {arg} at instruction {pc} in {chunk_name}"
                ))
            })?;
            let field_count = usize::try_from(arg2).map_err(|_| {
                TinyLangError::compile(format!(
                    "Verifier: invalid struct arity {arg2} at instruction {pc} in {chunk_name}"
                ))
            })?;
            let struct_def = &context.structs[struct_index];
            if !context.modules.can_construct(caller_function, struct_index) {
                return Err(module_access_error("struct", &struct_def.name, chunk_name, pc));
            }
            let expected = struct_def.fields.len();
            if field_count != expected {
                return Err(TinyLangError::compile(format!(
                    "Struct {:?} expects {expected} field value(s), got {arg2} at instruction {pc} in {chunk_name}",
                    struct_def.name
                )));
            }
        }
        if op == Op::MakeEnum {
            let variant_index = checked_index(arg, context.enum_variants.len()).map_err(|_| {
                TinyLangError::compile(format!(
                    "Verifier: invalid enum variant index {arg} at instruction {pc} in {chunk_name}"
                ))
            })?;
            if !context.modules.can_construct_enum(caller_function, variant_index) {
                let item = &context.enum_variants[variant_index];
                return Err(module_access_error("enum", &item.enum_name, chunk_name, pc));
            }
            let field_count = usize::try_from(arg2).map_err(|_| {
                TinyLangError::compile(format!(
                    "Verifier: invalid enum variant arity {arg2} at instruction {pc} in {chunk_name}"
                ))
            })?;
            let variant_def = &context.enum_variants[variant_index];
            let expected = variant_def.fields.len();
            if field_count != expected {
                return Err(TinyLangError::compile(format!(
                    "Enum variant {:?}.{:?} expects {expected} field value(s), got {arg2} at instruction {pc} in {chunk_name}",
                    variant_def.enum_name, variant_def.variant_name
                )));
            }
        }
        if op == Op::Builtin {
            let builtin_index = checked_index(arg, BUILTINS.len()).map_err(|_| {
                TinyLangError::compile(format!(
                    "Verifier: invalid builtin index {arg} at instruction {pc} in {chunk_name}"
                ))
            })?;
            let builtin = BUILTINS[builtin_index];
            if arg2 < builtin.min_args as i64 || arg2 > builtin.max_args as i64 {
                return Err(TinyLangError::compile(format!(
                    "Builtin {:?} expects {}..{} argument(s), got {arg2} at instruction {pc} in {chunk_name}",
                    builtin.name, builtin.min_args, builtin.max_args
                )));
            }
        }
        Ok(flow)
    }
}

fn verify_identifier(kind: &str, value: &str) -> Result<()> {
    reject_over_limit(&format!("{kind} text bytes"), value.len(), MAX_VERIFIER_TEXT_BYTES)?;
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return Err(TinyLangError::compile(format!("Verifier: {kind} must not be empty")));
    };
    if first.is_ascii_digit()
        || !(first == '_' || first.is_alphanumeric())
        || chars.any(|ch| ch != '_' && !ch.is_alphanumeric())
    {
        return Err(TinyLangError::compile(format!("Verifier: invalid {kind} {value:?}")));
    }
    Ok(())
}

fn unique_named_indexes<'a>(kind: &str, names: impl IntoIterator<Item = &'a str>) -> Result<HashMap<&'a str, usize>> {
    let mut indexes = HashMap::new();
    for (index, name) in names.into_iter().enumerate() {
        reject_over_limit(&format!("{kind} name bytes"), name.len(), MAX_VERIFIER_TEXT_BYTES)?;
        if indexes.insert(name, index).is_some() {
            return Err(TinyLangError::compile(format!("Verifier: duplicate {kind} name {name:?}")));
        }
    }
    Ok(indexes)
}

fn owner_for_name(kind: &str, name: &str, modules_by_name: &HashMap<&str, usize>) -> Result<Option<usize>> {
    let Some((module_name, local_name)) = name.split_once('.') else {
        verify_identifier(&format!("{kind} name"), name)?;
        return Ok(None);
    };
    if local_name.contains('.') {
        return Err(TinyLangError::compile(format!("Verifier: invalid qualified {kind} name {name:?}")));
    }
    verify_identifier(&format!("{kind} module qualifier"), module_name)?;
    verify_identifier(&format!("{kind} local name"), local_name)?;
    modules_by_name.get(module_name).copied().map(Some).ok_or_else(|| {
        TinyLangError::compile(format!("Verifier: {kind} {name:?} belongs to unknown module {module_name:?}"))
    })
}

fn reject_module_cycles(modules: &[crate::ModuleDef], imports: &[HashSet<usize>]) -> Result<()> {
    fn visit_module(
        module_index: usize,
        modules: &[crate::ModuleDef],
        imports: &[HashSet<usize>],
        states: &mut [u8],
    ) -> Result<()> {
        match states[module_index] {
            2 => return Ok(()),
            1 => {
                return Err(TinyLangError::compile(format!(
                    "Verifier: cyclic module dependency involving {:?}",
                    modules[module_index].name
                )));
            }
            _ => {}
        }
        states[module_index] = 1;
        for dependency in &imports[module_index] {
            visit_module(*dependency, modules, imports, states)?;
        }
        states[module_index] = 2;
        Ok(())
    }

    let mut states = vec![0; modules.len()];
    for module_index in 0..modules.len() {
        visit_module(module_index, modules, imports, &mut states)?;
    }
    Ok(())
}

fn module_access_error(target_kind: &str, target_name: &str, chunk_name: &str, pc: usize) -> TinyLangError {
    TinyLangError::compile(format!(
        "Verifier: {target_kind} {target_name:?} is not visible from {chunk_name} at instruction {pc}"
    ))
}

fn visit(
    seen: &mut [Option<StackDepth>],
    todo: &mut Vec<(usize, StackDepth)>,
    code: &[Instr],
    pc: i64,
    depth: StackDepth,
    origin: usize,
    chunk_name: &str,
) -> Result<()> {
    let Ok(pc_usize) = usize::try_from(pc) else {
        return Err(TinyLangError::compile(format!("Verifier: instruction {origin} in {chunk_name} targets {pc}")));
    };
    if pc_usize >= code.len() {
        return Err(TinyLangError::compile(format!("Verifier: instruction {origin} in {chunk_name} targets {pc}")));
    }
    let pc = pc_usize;
    if let Some(old_depth) = seen[pc] {
        if old_depth != depth {
            return Err(TinyLangError::compile(format!(
                "Verifier: stack depth mismatch at instruction {pc} in {chunk_name}: {old_depth} vs {depth}"
            )));
        }
        return Ok(());
    }
    seen[pc] = Some(depth);
    todo.push((pc, depth));
    Ok(())
}

fn next_depth(pc: usize, depth: StackDepth, delta: i64, chunk_name: &str) -> Result<StackDepth> {
    let depth = depth.value().checked_add(delta).ok_or_else(|| {
        TinyLangError::compile(format!("Verifier: stack depth overflow at instruction {pc} in {chunk_name}"))
    })?;
    if depth < 0 {
        return Err(TinyLangError::compile(format!("Verifier: stack underflow at instruction {pc} in {chunk_name}")));
    }
    if depth > MAX_STACK_DEPTH {
        return Err(TinyLangError::compile(format!(
            "Verifier: stack depth {depth} exceeds limit in {chunk_name} at instruction {pc}"
        )));
    }
    let encoded = u32::try_from(depth + 1).ok().and_then(NonZeroU32::new).ok_or_else(|| {
        TinyLangError::compile(format!("Verifier: stack depth overflow at instruction {pc} in {chunk_name}"))
    })?;
    Ok(StackDepth(encoded))
}

fn next_depth_after_popping_to_one(pc: usize, depth: StackDepth, count: i64, chunk_name: &str) -> Result<StackDepth> {
    StackEffect {
        inputs:  count,
        outputs: 1,
    }
    .apply(pc, depth, chunk_name)
}

fn next_pc(pc: usize) -> Result<i64> {
    let next = pc
        .checked_add(1)
        .ok_or_else(|| TinyLangError::compile("Verifier: instruction index overflow"))?;
    i64::try_from(next).map_err(|_| TinyLangError::compile("Verifier: instruction index too large"))
}

fn checked_index(index: i64, len: usize) -> Result<usize> {
    if index < 0 {
        return Err(TinyLangError::compile("negative index"));
    }
    let index = usize::try_from(index).map_err(|_| TinyLangError::compile("index is too large for this platform"))?;
    if index >= len {
        return Err(TinyLangError::compile("index out of bounds"));
    }
    Ok(index)
}

fn reject_over_limit(name: &str, got: usize, max: usize) -> Result<()> {
    if got > max {
        return Err(TinyLangError::compile(format!("Verifier: {name} {got} exceeds limit {max}")));
    }
    Ok(())
}

fn verify_string_list(name: &str, values: &[String], max_count: usize) -> Result<()> {
    reject_over_limit(name, values.len(), max_count)?;
    let mut bytes = 0usize;
    for value in values {
        bytes = bytes
            .checked_add(value.len())
            .ok_or_else(|| TinyLangError::compile(format!("Verifier: {name} text overflow")))?;
        reject_over_limit(&format!("{name} text bytes"), bytes, MAX_VERIFIER_TEXT_BYTES)?;
    }
    Ok(())
}

struct StackEffect {
    inputs:  i64,
    outputs: i64,
}

impl StackEffect {
    fn apply(self, pc: usize, depth: StackDepth, chunk_name: &str) -> Result<StackDepth> {
        // A result is pushed only after all inputs have been consumed. A
        // nonnegative net depth alone would accept ADD with just one operand.
        if depth.value() < self.inputs {
            return Err(TinyLangError::compile(format!(
                "Verifier: stack underflow at instruction {pc} in {chunk_name}"
            )));
        }
        let delta = self.outputs.checked_sub(self.inputs).ok_or_else(|| {
            TinyLangError::compile(format!("Verifier: stack effect overflow at instruction {pc} in {chunk_name}"))
        })?;
        next_depth(pc, depth, delta, chunk_name)
    }
}

fn stack_effect(op: Op) -> Option<StackEffect> {
    let (inputs, outputs) = match op {
        Op::PushInt
        | Op::PushString
        | Op::PushNull
        | Op::PushBool
        | Op::PushFloat
        | Op::Load
        | Op::LoadGlobal
        | Op::PushFunction => (0, 1),
        Op::Store | Op::Pop | Op::Print => (1, 0),
        Op::Add | Op::Sub | Op::Mul | Op::Div | Op::Lt | Op::Lte | Op::Gt | Op::Gte | Op::Eq | Op::Ne | Op::Index => {
            (2, 1)
        }
        Op::Neg | Op::GetField => (1, 1),
        Op::SetIndex => (3, 0),
        Op::SetField => (2, 0),
        _ => return None,
    };
    Some(StackEffect { inputs, outputs })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn coprocessor_program() -> Program {
        let mut function =
            Function::new("calculate", 0, vec![Instr::new(Op::PushInt, 42, 0), Instr::new(Op::Return, 0, 0)], 0);
        function.decorators.push(crate::Decorator {
            path:      vec!["stdx".into(), "coprocessor".into()],
            arguments: vec![crate::DecoratorArgument::String("Arduino".into())],
        });
        Program::new(vec![Instr::new(Op::Halt, 0, 0)], 0).with_functions(vec![function])
    }

    #[test]
    fn forged_decorator_shapes_and_signatures_are_rejected() {
        let malformed = [
            crate::Decorator {
                path:      vec![],
                arguments: vec![],
            },
            crate::Decorator {
                path:      vec!["stdx".into(), "bad-name".into()],
                arguments: vec![],
            },
            crate::Decorator {
                path:      vec!["stdx".into(); 9],
                arguments: vec![],
            },
            crate::Decorator {
                path:      vec!["stdx".into(), "unregistered".into()],
                arguments: vec![],
            },
            crate::Decorator {
                path:      vec!["stdx".into(), "coprocessor".into()],
                arguments: vec![],
            },
            crate::Decorator {
                path:      vec!["stdx".into(), "coprocessor".into()],
                arguments: vec![crate::DecoratorArgument::Integer(4)],
            },
            crate::Decorator {
                path:      vec!["stdx".into(), "coprocessor".into()],
                arguments: vec![crate::DecoratorArgument::Bool(true)],
            },
            crate::Decorator {
                path:      vec!["stdx".into(), "coprocessor".into()],
                arguments: vec![crate::DecoratorArgument::Identifier("Arduino".into())],
            },
            crate::Decorator {
                path:      vec!["stdx".into(), "coprocessor".into()],
                arguments: vec![crate::DecoratorArgument::String(String::new())],
            },
            crate::Decorator {
                path:      vec!["stdx".into(), "coprocessor".into()],
                arguments: vec![crate::DecoratorArgument::String("Arduino".into()); 17],
            },
        ];
        for decorator in malformed {
            let mut program = coprocessor_program();
            program.functions[0].decorators = vec![decorator];
            assert!(matches!(BytecodeVerifier::verify(&program), Err(TinyLangError::Compile(_))));
        }
        let mut program = coprocessor_program();
        let decorator = program.functions[0].decorators[0].clone();
        program.functions[0].decorators = vec![decorator.clone(), decorator.clone()];
        assert!(BytecodeVerifier::verify(&program).is_err(), "duplicate placement is invalid");
        program.functions[0].decorators = vec![decorator; 17];
        assert!(BytecodeVerifier::verify(&program).is_err(), "declaration count is bounded");
    }

    #[test]
    fn decorator_text_is_bounded_across_the_whole_program() {
        let mut program = coprocessor_program();
        program.functions[0].decorators[0].arguments = vec![crate::DecoratorArgument::String(
            "x".repeat(MAX_VERIFIER_TEXT_BYTES / 2),
        )];
        let mut second = program.functions[0].clone();
        second.name = "second".into();
        program.functions.push(second);
        let error = BytecodeVerifier::verify(&program).expect_err("aggregate decorator text exceeds budget");
        assert!(error.to_string().contains("decorator text bytes"), "{error}");
    }

    #[test]
    fn placement_requires_owning_hardware_grant_and_detailed_permission() {
        let mut program = coprocessor_program();
        BytecodeVerifier::verify(&program).expect("root embedding grant permits placement metadata");
        program.root_capabilities = crate::ModuleCapabilities::none();
        program.root_permissions = crate::ModulePermissions::none();
        assert!(
            BytecodeVerifier::verify(&program)
                .expect_err("decorator cannot grant hardware")
                .to_string()
                .contains("hardware capability grant")
        );
        program.root_capabilities = crate::ModuleCapabilities::all();
        program.root_permissions = crate::ModulePermissions::none();
        assert!(BytecodeVerifier::verify(&program).is_err(), "capability bits do not replace detailed permission");
        program.root_capabilities = crate::ModuleCapabilities::none();
        program.root_permissions = crate::ModulePermissions::from_capabilities(crate::ModuleCapabilities::all());
        assert!(BytecodeVerifier::verify(&program).is_err(), "detailed permissions cannot exceed declared grant");

        let mut program = coprocessor_program();
        program.functions[0].name = "device.calculate".into();
        program.modules.push(crate::ModuleDef {
            name:               "device".into(),
            path:               "device".into(),
            imports:            vec![],
            exported_functions: vec!["calculate".into()],
            exported_structs:   vec![],
            capabilities:       crate::ModuleCapabilities::none(),
            permissions:        crate::ModulePermissions::none(),
        });
        assert!(BytecodeVerifier::verify(&program).is_err(), "imported function cannot inherit root hardware grant");
        let hardware = crate::ModuleCapabilities::from_names(&["hardware".into()]).expect("hardware capability");
        program.modules[0].capabilities = hardware;
        program.modules[0].permissions = crate::ModulePermissions::from_capabilities(hardware);
        BytecodeVerifier::verify(&program).expect("owning module has externally established grant");
    }

    #[test]
    fn untrusted_placement_metadata_never_grants_effective_hardware_access() {
        let mut program = coprocessor_program();
        program.policy_trusted = false;
        let verified = crate::VerifiedProgram::verify(program).expect("valid metadata remains inspectable");
        assert_eq!(verified.program().functions[0].coprocessor_target(), Some("Arduino"));
        assert!(
            !verified
                .permissions_for_chunk(1)
                .expect("function permissions")
                .hardware_access()
        );
        assert!(!verified.program().capabilities_for_function(Some(0)).hardware_access());
    }

    #[test]
    fn metadata_text_budget_preserves_count_order_and_utf8_byte_boundaries() {
        verify_string_list("names", &[], 0).expect("empty list at its count limit");
        let mut values = vec!["é".repeat(MAX_VERIFIER_TEXT_BYTES / 2)];
        verify_string_list("names", &values, 1).expect("exact UTF-8 byte budget");
        values.extend(["é".into(), "later text must not change the first crossing".into()]);
        assert_eq!(
            verify_string_list("names", &values, 2)
                .expect_err("count is checked before text")
                .to_string(),
            "Verifier: names 3 exceeds limit 2"
        );
        assert_eq!(
            verify_string_list("names", &values, 3)
                .expect_err("first cumulative byte crossing")
                .to_string(),
            "Verifier: names text bytes 1048578 exceeds limit 1048576"
        );
    }

    #[test]
    fn metadata_budget_labels_preserve_debug_escaping_and_error_order() {
        let program = Program::new(vec![Instr::new(Op::Halt, 0, 0)], 0).with_functions(vec![Function::new(
            "odd\"name",
            0,
            vec![Instr::new(Op::PushInt, 0, 0), Instr::new(Op::Return, 0, 0)],
            MAX_VERIFIER_SLOT_COUNT + 1,
        )]);
        let error = BytecodeVerifier::verify(&program).expect_err("slot budget precedes invalid identifier");
        assert!(matches!(error, TinyLangError::Compile(_)));
        assert_eq!(error.to_string(), "Verifier: function \"odd\\\"name\" slot_count 65537 exceeds limit 65536");

        let program = Program::new(vec![Instr::new(Op::Halt, 0, 0)], 0).with_structs(vec![StructDef::new(
            "odd\"name",
            vec![String::new(); MAX_VERIFIER_STRUCT_FIELDS + 1],
        )]);
        assert_eq!(
            BytecodeVerifier::verify(&program)
                .expect_err("field budget precedes invalid identifier")
                .to_string(),
            "Verifier: struct \"odd\\\"name\" fields 257 exceeds limit 256"
        );
    }

    #[test]
    fn metadata_identifier_diagnostics_preserve_qualified_roles() {
        let modules = HashMap::from([("known", 0)]);
        assert_eq!(owner_for_name("function", "root", &modules).expect("root owner"), None);
        assert_eq!(owner_for_name("function", "known.local", &modules).expect("module owner"), Some(0));
        for (name, expected) in [
            ("", "Verifier: function name must not be empty"),
            ("9bad", "Verifier: invalid function name \"9bad\""),
            ("known.", "Verifier: function local name must not be empty"),
            (".local", "Verifier: function module qualifier must not be empty"),
            ("known.bad-name", "Verifier: invalid function local name \"bad-name\""),
            ("bad-name.local", "Verifier: invalid function module qualifier \"bad-name\""),
            ("known.a.b", "Verifier: invalid qualified function name \"known.a.b\""),
            ("missing.local", "Verifier: function \"missing.local\" belongs to unknown module \"missing\""),
        ] {
            let error = owner_for_name("function", name, &modules).expect_err("invalid qualified name");
            assert!(matches!(error, TinyLangError::Compile(_)));
            assert_eq!(error.to_string(), expected);
        }
    }

    #[test]
    fn unreachable_enum_constructors_must_have_the_declared_arity() {
        let mut program = Program::new(
            vec![
                Instr::new(Op::Jump, 2, 0),
                Instr::new(Op::MakeEnum, 0, 0),
                Instr::new(Op::Halt, 0, 0),
            ],
            0,
        );
        program.enum_variants.push(EnumVariantDef {
            enum_name:    "Maybe".into(),
            variant_name: "Some".into(),
            tag:          0,
            fields:       vec!["value".into()],
        });
        assert_eq!(
            BytecodeVerifier::verify(&program)
                .expect_err("invalid unreachable constructor")
                .to_string(),
            "Enum variant \"Maybe\".\"Some\" expects 1 field value(s), got 0 at instruction 1 in main"
        );
    }

    #[test]
    fn certificates_preserve_reachable_stack_peaks_for_each_chunk() {
        let program = Program::new(
            vec![
                Instr::new(Op::Jump, 3, 0),
                Instr::new(Op::PushInt, 99, 0),
                Instr::new(Op::PushInt, 99, 0),
                Instr::new(Op::PushInt, 42, 0),
                Instr::new(Op::Print, 0, 0),
                Instr::new(Op::Halt, 0, 0),
            ],
            0,
        )
        .with_functions(vec![Function::new(
            "sum",
            0,
            vec![
                Instr::new(Op::PushInt, 1, 0),
                Instr::new(Op::PushInt, 2, 0),
                Instr::new(Op::Add, 0, 0),
                Instr::new(Op::Return, 0, 0),
            ],
            0,
        )]);
        let verified = crate::VerifiedProgram::verify(program).expect("valid program");
        assert_eq!(verified.chunk_certificate(0).expect("main certificate").max_stack_depth, 1);
        assert_eq!(
            verified
                .chunk_certificate(1)
                .expect("function certificate")
                .max_stack_depth,
            2
        );
        assert!(verified.chunk_certificate(2).is_err());
        let clone = verified.clone();
        assert!(std::ptr::eq(verified.chunk_certificate(1).unwrap(), clone.chunk_certificate(1).unwrap()));
    }

    #[test]
    fn stack_depth_limit_accepts_the_boundary_and_rejects_one_more_value() {
        for count in [65_536, 65_537] {
            let mut code = vec![Instr::new(Op::PushInt, 1, 0); count];
            code.extend([
                Instr::new(Op::MakeArray, i64::try_from(count).expect("bounded array count"), 0),
                Instr::new(Op::Pop, 0, 0),
                Instr::new(Op::Halt, 0, 0),
            ]);
            let result = BytecodeVerifier::verify(&Program::new(code, 0));
            if count == 65_536 {
                result.expect("maximum stack depth is valid");
            } else {
                assert!(
                    result
                        .expect_err("one above maximum")
                        .to_string()
                        .contains("stack depth 65537 exceeds limit")
                );
            }
        }
    }

    #[test]
    fn inconsistent_stack_merges_report_the_join_instruction() {
        let program = Program::new(
            vec![
                Instr::new(Op::PushInt, 0, 0),
                Instr::new(Op::JumpIfZero, 4, 0),
                Instr::new(Op::PushInt, 7, 0),
                Instr::new(Op::Jump, 5, 0),
                Instr::new(Op::Jump, 5, 0),
                Instr::new(Op::Print, 0, 0),
                Instr::new(Op::Halt, 0, 0),
            ],
            0,
        );
        assert_eq!(
            BytecodeVerifier::verify(&program)
                .expect_err("inconsistent merge")
                .to_string(),
            "Verifier: stack depth mismatch at instruction 5 in main: 1 vs 0",
        );
    }

    #[test]
    fn unreachable_instructions_still_require_valid_static_operands() {
        for instruction in [
            Instr::new(Op::Load, 1, 0),
            Instr::new(Op::Store, -1, 0),
            Instr::new(Op::LoadGlobal, 1, 0),
            Instr::new(Op::PushString, 0, 0),
            Instr::new(Op::PushFunction, 0, 0),
            Instr::new(Op::GetField, 0, 0),
            Instr::new(Op::SetField, 0, 0),
            Instr::new(Op::Call, 0, 0),
            Instr::new(Op::Builtin, i64::MAX, 0),
        ] {
            let program = Program::new(vec![Instr::new(Op::Jump, 2, 0), instruction, Instr::new(Op::Halt, 0, 0)], 1);
            assert!(BytecodeVerifier::verify(&program).is_err(), "{:?}", instruction.op);
        }
    }

    #[test]
    fn rejects_missing_inputs_even_when_the_net_stack_depth_is_nonnegative() {
        let operations = [
            (Op::Add, 2),
            (Op::Sub, 2),
            (Op::Mul, 2),
            (Op::Div, 2),
            (Op::Lt, 2),
            (Op::Lte, 2),
            (Op::Gt, 2),
            (Op::Gte, 2),
            (Op::Eq, 2),
            (Op::Ne, 2),
            (Op::Index, 2),
            (Op::Neg, 1),
            (Op::GetField, 1),
        ];
        for (op, required) in operations {
            let mut code = vec![Instr::new(Op::PushInt, 1, 0); required - 1];
            code.push(Instr::new(op, 0, 0));
            code.push(Instr::new(Op::Halt, 0, 0));
            let mut program = Program::new(code, 0);
            program.fields.push("field".into());
            let error = BytecodeVerifier::verify(&program).expect_err("all input operands must exist");
            assert!(error.to_string().contains("stack underflow"), "{op:?}: {error}");
        }
    }

    #[test]
    fn rejects_missing_call_and_constructor_inputs_before_the_result_is_pushed() {
        let builtin = i64::try_from(crate::builtin_index("len").expect("len builtin")).expect("builtin index");
        for instruction in [
            Instr::new(Op::Call, 0, 1),
            Instr::new(Op::CallValue, 0, 0),
            Instr::new(Op::MakeArray, 1, 0),
            Instr::new(Op::MakeStruct, 0, 1),
            Instr::new(Op::MakeEnum, 0, 1),
            Instr::new(Op::Builtin, builtin, 1),
        ] {
            let mut program = Program::new(vec![instruction, Instr::new(Op::Halt, 0, 0)], 0);
            program.functions.push(Function::new(
                "identity",
                1,
                vec![Instr::new(Op::Load, 0, 0), Instr::new(Op::Return, 0, 0)],
                1,
            ));
            program.structs.push(StructDef::new("Boxed", vec!["value".into()]));
            program.enum_variants.push(EnumVariantDef {
                enum_name:    "Maybe".into(),
                variant_name: "Some".into(),
                tag:          0,
                fields:       vec!["value".into()],
            });
            let error = BytecodeVerifier::verify(&program).expect_err("result cannot supply an input operand");
            assert!(error.to_string().contains("stack underflow"), "{:?}: {error}", instruction.op);
        }
    }
}
