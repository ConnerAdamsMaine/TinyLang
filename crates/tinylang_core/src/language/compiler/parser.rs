mod decorators;

use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;

use crate::language::decorators::Decorator;
use crate::source::bytecode::{BytecodeSource, SourceOffset};
use crate::{
    BUILTINS, EnumVariantDef, Function, Instr, Lexer, ModuleCapabilities, ModuleDef, ModuleImportDef, ModuleInfo,
    ModulePermissions, Op, Program, Resolver, Result, SharedState, SourceMap, StructDef, SymbolTable, TinyLangError,
    Token, TokenKind, VmSettings, builtin_index, default_import_alias, module_name_from_import, unique_module_name,
};

#[derive(Debug)]
struct LoopContext {
    start: usize,
    breaks: Vec<usize>,
}

#[derive(Clone, Copy)]
enum ReadSlot {
    Local(usize),
    Global(usize),
    Capture(usize),
}

#[derive(Clone, Copy)]
struct CaptureSource {
    slot: ReadSlot,
}

struct Capture {
    name: String,
    source: CaptureSource,
    /// The captured value is a shared environment cell rather than a plain value.
    boxed: bool,
}

struct EnclosingCaptureContext {
    captures: Vec<Capture>,
    capture_slots: HashMap<String, usize>,
    outer_symbols: Option<SymbolTable>,
    outer_slots: HashMap<String, usize>,
    outer_globals: HashMap<String, usize>,
    outer_function: bool,
    outer_boxed: HashSet<usize>,
}

/// Generated closure functions use this prefix; user declarations may not.
pub(crate) const CLOSURE_NAME_PREFIX: &str = "__tiny_closure_";

fn placeholder_function(name: &str) -> Function {
    Function {
        name: name.to_string(),
        decorators: Vec::new(),
        generic_params: Vec::new(),
        param_count: 0,
        code: Vec::new(),
        source: None,
        slot_count: 0,
        names: Vec::new(),
    }
}

fn capture_slot_operand(index: usize) -> Result<i64> {
    let index = i64::try_from(index).map_err(|_| TinyLangError::compile("Closure capture index is too large"))?;
    index
        .checked_add(1)
        .and_then(i64::checked_neg)
        .ok_or_else(|| TinyLangError::compile("Closure capture index is too large"))
}

fn remap_capture_slots(code: &mut [Instr], capture_count: usize) -> Result<()> {
    let shift =
        i64::try_from(capture_count).map_err(|_| TinyLangError::compile("Closure capture count is too large"))?;
    for instruction in code {
        if !matches!(instruction.op, Op::Load | Op::Store) {
            continue;
        }
        if instruction.arg < 0 {
            let capture = instruction
                .arg
                .checked_neg()
                .and_then(|index| index.checked_sub(1))
                .and_then(|index| usize::try_from(index).ok())
                .ok_or_else(|| TinyLangError::compile("Invalid internal closure capture slot"))?;
            instruction.arg =
                i64::try_from(capture).map_err(|_| TinyLangError::compile("Closure capture index is too large"))?;
        } else {
            instruction.arg = instruction
                .arg
                .checked_add(shift)
                .ok_or_else(|| TinyLangError::compile("Closure local slot is too large"))?;
        }
    }
    Ok(())
}

pub(crate) struct Compiler {
    // Own the lexer's allocation, but only expose/consume its unparsed suffix.
    tokens: std::vec::IntoIter<Token>,
    eof_token: Token,
    source_map: Arc<SourceMap>,
    source_offsets: Vec<SourceOffset>,
    last_source_offset: SourceOffset,
    last_token_end: usize,
    filename: String,
    resolver: Option<Resolver>,
    module_mode: bool,
    module_filename: Option<String>,
    module_qualified_name: Option<String>,
    module_imports: Vec<ModuleImportDef>,
    namespaces: HashMap<String, ModuleInfo>,
    extension_aliases: HashMap<String, &'static str>,
    accept_imports: bool,
    symbols: SymbolTable,
    code: Vec<Instr>,
    shared: SharedState,
    next_closure_id: usize,
    local_function_indexes: HashMap<String, usize>,
    local_struct_indexes: HashMap<String, usize>,
    local_enum_names: HashSet<String>,
    function_globals: HashMap<String, usize>,
    captures: Vec<Capture>,
    capture_slots: HashMap<String, usize>,
    capture_outer_symbols: Option<SymbolTable>,
    capture_outer_slots: HashMap<String, usize>,
    capture_outer_globals: HashMap<String, usize>,
    capture_outer_function: bool,
    enclosing_captures: Vec<EnclosingCaptureContext>,
    loops: Vec<LoopContext>,
    /// Names in the current function that need shared cells (assigned and referenced by a nested closure).
    boxed_names: HashSet<String>,
    /// Local slots in the current function that hold shared cells.
    boxed_slots: HashSet<usize>,
    /// Boxed local slots of the function that encloses the closure being compiled.
    capture_outer_boxed: HashSet<usize>,
    in_function: bool,
    unsafe_depth: usize,
    root_capabilities: ModuleCapabilities,
    root_permissions: ModulePermissions,
    vm_settings: VmSettings,
}

impl Compiler {
    pub(crate) fn new(
        source: impl Into<String>,
        filename: impl Into<String>,
        resolver: Option<Resolver>,
        module_mode: bool,
        module_name: impl Into<String>,
        module_permissions: ModulePermissions,
        shared: SharedState,
    ) -> Result<Self> {
        let source = source.into();
        let filename = filename.into();
        let source_len = source.len();
        let source_map = Arc::new(SourceMap::new(source, filename.clone()));
        let tokens = Lexer::new(&source_map).tokenize()?;
        let mut module_filename = None;
        let mut module_qualified_name = None;
        if module_mode {
            let name = {
                let mut state = shared.borrow_mut();
                if let Some(info) = state.modules.get(&filename) {
                    info.name.clone()
                } else {
                    let base_name = module_name.into();
                    let name = unique_module_name(&mut state, &base_name, &filename);
                    state.modules.insert(
                        filename.clone(),
                        ModuleInfo {
                            name: name.clone(),
                            capabilities: module_permissions.capabilities(),
                            permissions: module_permissions,
                            function_exports: HashMap::new(),
                            struct_exports: HashMap::new(),
                            all_functions: HashSet::new(),
                            all_structs: HashSet::new(),
                            imports: Vec::new(),
                            finalized: false,
                        },
                    );
                    name
                }
            };
            module_filename = Some(filename.clone());
            module_qualified_name = Some(name);
        }
        Ok(Self {
            tokens: tokens.into_iter(),
            eof_token: Token {
                kind: TokenKind::Eof,
                text: String::new(),
                pos: source_len,
                end: source_len,
            },
            source_map,
            source_offsets: Vec::new(),
            last_source_offset: SourceOffset(source_len),
            last_token_end: 0,
            filename,
            resolver,
            module_mode,
            module_filename,
            module_qualified_name,
            module_imports: Vec::new(),
            namespaces: HashMap::new(),
            extension_aliases: HashMap::new(),
            accept_imports: true,
            symbols: SymbolTable::new(),
            code: Vec::new(),
            shared,
            next_closure_id: 0,
            local_function_indexes: HashMap::new(),
            local_struct_indexes: HashMap::new(),
            local_enum_names: HashSet::new(),
            function_globals: HashMap::new(),
            captures: Vec::new(),
            capture_slots: HashMap::new(),
            capture_outer_symbols: None,
            capture_outer_slots: HashMap::new(),
            capture_outer_globals: HashMap::new(),
            capture_outer_function: false,
            enclosing_captures: Vec::new(),
            boxed_names: HashSet::new(),
            boxed_slots: HashSet::new(),
            capture_outer_boxed: HashSet::new(),
            loops: Vec::new(),
            in_function: false,
            unsafe_depth: 0,
            root_capabilities: ModuleCapabilities::all(),
            root_permissions: ModulePermissions::from_capabilities(ModuleCapabilities::all()),
            vm_settings: VmSettings::default(),
        })
    }

    /// Applies the project policy to the root compilation unit. Nested module
    /// compilers only contribute definitions to the shared state; only the
    /// final owner assembles a Program.
    pub(crate) fn with_runtime_policy(
        mut self,
        root_capabilities: ModuleCapabilities,
        vm_settings: VmSettings,
    ) -> Self {
        self.root_capabilities = root_capabilities;
        self.root_permissions = ModulePermissions::from_capabilities(root_capabilities);
        self.vm_settings = vm_settings;
        self
    }

    pub(crate) fn compile(mut self) -> Result<Program> {
        self.compile_unit()?;
        self.emit_at(Op::Halt, 0, 0, self.eof_token.pos);
        // Imported units have finished and dropped their shared-state handles.
        // Finalization transfers ownership; it must not drain a live compiler
        // or silently fall back to cloning the complete program tables.
        let state = Rc::try_unwrap(self.shared)
            .map_err(|_| TinyLangError::compile("Internal compiler state still has active owners"))?
            .into_inner();
        // Preserve the old snapshot's compact extents without cloning its
        // elements. Moving growable builders directly retained excess capacity
        // in every function's local-name table and in the final shared tables.
        Ok(Program {
            code: self.code.into_boxed_slice().into_vec(),
            source: Some(BytecodeSource::new(self.source_map, self.source_offsets)),
            slot_count: self.symbols.slot_count(),
            names: self.symbols.names.into_boxed_slice().into_vec(),
            functions: state.functions.into_boxed_slice().into_vec(),
            strings: state.strings.into_boxed_slice().into_vec(),
            structs: state.structs.into_boxed_slice().into_vec(),
            fields: state.fields.into_boxed_slice().into_vec(),
            modules: state.module_defs.into_boxed_slice().into_vec(),
            enum_variants: state.enum_variants.into_boxed_slice().into_vec(),
            root_capabilities: self.root_capabilities,
            root_permissions: self.root_permissions,
            policy_trusted: true,
            vm_settings: self.vm_settings,
        })
    }

    /// Compile one source unit into its owned code and the session's shared
    /// definitions. Imported units have no executable main chunk to assemble.
    fn compile_unit(&mut self) -> Result<()> {
        while self.current().kind != TokenKind::Eof {
            match self.current().kind {
                TokenKind::At => {
                    self.accept_imports = false;
                    self.decorated_declaration()?;
                }
                TokenKind::Import => self.import_statement()?,
                TokenKind::Export => {
                    self.accept_imports = false;
                    self.export_declaration(Vec::new())?;
                }
                TokenKind::Struct => {
                    self.accept_imports = false;
                    self.struct_definition(false)?;
                }
                TokenKind::Enum => {
                    self.accept_imports = false;
                    self.enum_definition(false)?;
                }
                TokenKind::Fn => {
                    self.accept_imports = false;
                    self.function_definition(false, Vec::new())?;
                }
                _ => {
                    if self.module_mode {
                        return Err(self.error(
                            "Imported modules may only contain import, struct, and fn declarations",
                            self.current().clone(),
                        ));
                    }
                    self.accept_imports = false;
                    self.statement()?;
                }
            }
        }
        self.finalize_module()
    }

    fn export_declaration(&mut self, decorators: Vec<Decorator>) -> Result<()> {
        let token = self.eat(TokenKind::Export)?;
        match self.current().kind {
            TokenKind::Struct => self.struct_definition(true),
            TokenKind::Fn => self.function_definition(true, decorators),
            _ => Err(self.error("Expected function or struct declaration after export", token)),
        }
    }

    fn finalize_module(&mut self) -> Result<()> {
        let Some(filename) = self.module_filename.clone() else {
            return Ok(());
        };
        let mut state = self.shared.borrow_mut();
        let def = {
            let Some(info) = state.modules.get_mut(&filename) else {
                return Err(TinyLangError::compile("Internal module state error"));
            };
            if info.finalized {
                return Ok(());
            }
            info.imports.clone_from(&self.module_imports);
            let mut exported_functions = info.function_exports.keys().cloned().collect::<Vec<_>>();
            let mut exported_structs = info.struct_exports.keys().cloned().collect::<Vec<_>>();
            exported_functions.sort();
            exported_structs.sort();
            info.finalized = true;
            ModuleDef {
                name: info.name.clone(),
                path: info.name.clone(),
                imports: info.imports.clone(),
                exported_functions,
                exported_structs,
                capabilities: info.capabilities,
                permissions: info.permissions.clone(),
            }
        };
        state.module_defs.push(def);
        Ok(())
    }

    fn statement(&mut self) -> Result<()> {
        match self.current().kind {
            TokenKind::At => {
                Err(self
                    .error("Decorators are only allowed on top level function declarations", self.current().clone()))
            }
            TokenKind::Let => self.let_statement(),
            TokenKind::Print => self.print_statement(),
            TokenKind::While => self.while_statement(),
            TokenKind::If => self.if_statement(),
            TokenKind::Break => self.break_statement(),
            TokenKind::Continue => self.continue_statement(),
            TokenKind::Unsafe if self.peek_kind(1) == Some(TokenKind::LBrace) => self.unsafe_block_statement(),
            TokenKind::Ident if self.peek_kind(1) == Some(TokenKind::Equal) => self.assignment_statement(),
            TokenKind::Return => self.return_statement(),
            TokenKind::Set => self.set_statement(),
            TokenKind::Fn => Err(self.error(
                "Function definitions are only allowed at top level and before executable statements",
                self.current().clone(),
            )),
            TokenKind::Import | TokenKind::Struct | TokenKind::Export => Err(self.error(
                "Imports, exports, and struct definitions are only allowed at top level before statements",
                self.current().clone(),
            )),
            _ if self.is_expression_start() => self.expression_statement(),
            _ => Err(self.error("Expected statement", self.current().clone())),
        }
    }

    fn expression_statement(&mut self) -> Result<()> {
        self.expression()?;
        self.emit(Op::Pop, 0, 0);
        Ok(())
    }

    fn unsafe_block_statement(&mut self) -> Result<()> {
        self.eat(TokenKind::Unsafe)?;
        self.unsafe_depth += 1;
        let result = self.block();
        self.unsafe_depth -= 1;
        result
    }

    fn let_statement(&mut self) -> Result<()> {
        self.eat(TokenKind::Let)?;
        let name_token = self.eat(TokenKind::Ident)?;
        if self.has_namespace(&name_token.text) {
            return Err(self.error_at(
                format!("Variable {:?} conflicts with an imported namespace", name_token.text),
                name_token.pos,
            ));
        }
        self.skip_optional_type_annotation(TokenKind::Colon)?;
        self.eat(TokenKind::Equal)?;
        self.expression()?;
        let slot = self.symbols.define_current(&name_token.text).ok_or_else(|| {
            self.error(format!("Variable {:?} is already defined in this scope", name_token.text), name_token.clone())
        })?;
        if self.in_function && self.boxed_names.contains(&name_token.text) {
            self.boxed_slots.insert(slot);
            self.emit_at(Op::MakeCell, 0, 0, name_token.pos);
        } else {
            self.boxed_slots.remove(&slot);
        }
        self.emit_at(Op::Store, slot as i64, 0, name_token.pos);
        Ok(())
    }

    /// Consumes and discards an optional type annotation introduced by
    /// `marker` (`Colon` for `let`/parameter annotations, `Arrow` for
    /// function return-type annotations). `TinyLang` has no static type
    /// checker (dynamically typed by design — see `docs/v2-roadmap.md`);
    /// annotations are syntax-only documentation with no effect on compiled
    /// bytecode.
    fn skip_optional_type_annotation(&mut self, marker: TokenKind) -> Result<()> {
        if self.current().kind == marker {
            self.eat(marker)?;
            self.eat(TokenKind::Ident)?;
        }
        Ok(())
    }

    fn assignment_statement(&mut self) -> Result<()> {
        let name_token = self.eat(TokenKind::Ident)?;
        if self.has_namespace(&name_token.text) {
            return Err(self.error(format!("Cannot assign to import namespace {:?}", name_token.text), name_token));
        }
        let slot = self.get_assignment_slot(&name_token)?;
        self.eat(TokenKind::Equal)?;
        self.expression()?;
        self.emit_store_slot(slot, name_token.pos)?;
        Ok(())
    }

    fn print_statement(&mut self) -> Result<()> {
        let token = self.eat(TokenKind::Print)?;
        self.expression()?;
        self.emit_at(Op::Print, 0, 0, token.pos);
        Ok(())
    }

    fn while_statement(&mut self) -> Result<()> {
        self.eat(TokenKind::While)?;
        let loop_start = self.code.len();
        self.expression()?;
        let exit_jump = self.emit_placeholder(Op::JumpIfZero);
        self.loops.push(LoopContext {
            start: loop_start,
            breaks: Vec::new(),
        });
        let result = self.block();
        result?;
        let loop_context = self
            .loops
            .pop()
            .ok_or_else(|| TinyLangError::compile("Internal loop state error"))?;
        self.emit(Op::Jump, loop_start as i64, 0);
        let loop_end = self.code.len() as i64;
        self.patch(exit_jump, loop_end)?;
        for break_jump in loop_context.breaks {
            self.patch(break_jump, loop_end)?;
        }
        Ok(())
    }

    fn if_statement(&mut self) -> Result<()> {
        self.eat(TokenKind::If)?;
        self.expression()?;
        let false_jump = self.emit_placeholder(Op::JumpIfZero);
        self.block()?;
        if self.current().kind == TokenKind::Else {
            let end_jump = self.emit_placeholder(Op::Jump);
            self.patch(false_jump, self.code.len() as i64)?;
            self.eat(TokenKind::Else)?;
            if self.current().kind == TokenKind::If {
                self.if_statement()?;
            } else {
                self.block()?;
            }
            self.patch(end_jump, self.code.len() as i64)?;
        } else {
            self.patch(false_jump, self.code.len() as i64)?;
        }
        Ok(())
    }

    fn break_statement(&mut self) -> Result<()> {
        let token = self.eat(TokenKind::Break)?;
        if self.loops.is_empty() {
            return Err(self.error("Break outside loop", token));
        }
        let break_jump = self.emit_placeholder(Op::Jump);
        let Some(loop_context) = self.loops.last_mut() else {
            return Err(TinyLangError::compile("Internal loop state error"));
        };
        loop_context.breaks.push(break_jump);
        Ok(())
    }

    fn continue_statement(&mut self) -> Result<()> {
        let token = self.eat(TokenKind::Continue)?;
        let loop_start = self
            .loops
            .last()
            .map(|item| item.start)
            .ok_or_else(|| self.error("Continue outside loop", token))?;
        self.emit(Op::Jump, loop_start as i64, 0);
        Ok(())
    }

    fn return_statement(&mut self) -> Result<()> {
        if !self.in_function {
            return Err(self.error("Return outside function", self.current().clone()));
        }
        let token = self.eat(TokenKind::Return)?;
        self.expression()?;
        self.emit_at(Op::Return, 0, 0, token.pos);
        Ok(())
    }

    fn set_statement(&mut self) -> Result<()> {
        self.eat(TokenKind::Set)?;
        let name_token = self.eat(TokenKind::Ident)?;
        self.emit_load_name(&name_token)?;

        if self.current().kind == TokenKind::LBracket {
            let token = self.eat(TokenKind::LBracket)?;
            self.expression()?;
            self.eat(TokenKind::RBracket)?;
            self.eat(TokenKind::Equal)?;
            self.expression()?;
            self.emit_at(Op::SetIndex, 0, 0, token.pos);
            return Ok(());
        }

        if self.current().kind == TokenKind::Dot {
            self.eat(TokenKind::Dot)?;
            let field = self.eat(TokenKind::Ident)?;
            let field_index = self.intern_field(&field.text);
            self.eat(TokenKind::Equal)?;
            self.expression()?;
            self.emit_at(Op::SetField, field_index as i64, 0, field.pos);
            return Ok(());
        }

        Err(self.error("Expected indexed or field assignment target after set", self.current().clone()))
    }

    fn import_statement(&mut self) -> Result<()> {
        let token = self.current().clone();
        if !self.accept_imports {
            return Err(self.error("Imports must appear before declarations and statements", token));
        }
        self.eat(TokenKind::Import)?;
        let path_token = self.eat(TokenKind::String)?;
        let (alias, alias_pos) = if self.current().kind == TokenKind::As {
            self.eat(TokenKind::As)?;
            let alias_token = self.eat(TokenKind::Ident)?;
            (alias_token.text.clone(), alias_token.pos)
        } else {
            (default_import_alias(&path_token.text), path_token.pos)
        };
        if self.has_namespace(&alias) || self.symbols.contains(&alias) {
            return Err(self.error_at(format!("Import namespace {alias:?} is already defined"), alias_pos));
        }
        if builtin_index(&alias).is_some() {
            return Err(self.error_at(format!("Import namespace {alias:?} conflicts with a builtin"), alias_pos));
        }

        if let Some(namespace) =
            crate::language::decorators::registry::DecoratorRegistry::extension_namespace(&path_token.text)
        {
            self.extension_aliases.insert(alias, namespace);
            return Ok(());
        }
        let resolver = self
            .resolver
            .as_ref()
            .ok_or_else(|| self.error("Imports require compiling from a source file", path_token.clone()))?;
        let resolved_module = resolver.borrow_mut().resolve_import(&self.filename, &path_token.text)?;
        let module_filename = resolved_module.filename;

        let needs_load = {
            let state = self.shared.borrow();
            state.modules.get(&module_filename).is_none_or(|info| !info.finalized)
        };
        if needs_load {
            {
                let mut state = self.shared.borrow_mut();
                if state.loading_modules.contains(&module_filename) {
                    return Err(self.error_at(format!("Import cycle involving {module_filename}"), path_token.pos));
                }
                state.loading_modules.insert(module_filename.clone());
            }
            let compile_result = (|| {
                let mut compiler = Compiler::new(
                    resolved_module.source,
                    module_filename.clone(),
                    self.resolver.clone(),
                    true,
                    module_name_from_import(&path_token.text, &module_filename),
                    resolved_module.permissions,
                    Rc::clone(&self.shared),
                )?;
                compiler.compile_unit()
            })();
            self.shared.borrow_mut().loading_modules.remove(&module_filename);
            compile_result?;
        }

        let info = self
            .shared
            .borrow()
            .modules
            .get(&module_filename)
            .cloned()
            .ok_or_else(|| TinyLangError::compile("Internal import state error"))?;
        if let Some(resolver) = &self.resolver {
            resolver.borrow_mut().record_module_name(&module_filename, &info.name);
        }
        self.namespaces.insert(alias.clone(), info.clone());
        self.module_imports.push(ModuleImportDef {
            alias,
            path: path_token.text,
            module: info.name.clone(),
            resolved: info.name,
        });
        Ok(())
    }

    fn struct_definition(&mut self, exported: bool) -> Result<()> {
        self.eat(TokenKind::Struct)?;
        let name_token = self.eat(TokenKind::Ident)?;
        let name = name_token.text.clone();
        if self.has_namespace(&name) {
            return Err(self.error(format!("Struct {name:?} conflicts with an imported namespace"), name_token));
        }
        if self.local_struct_indexes.contains_key(&name) {
            return Err(self.error(format!("Struct {name:?} is already defined"), name_token));
        }
        if self.local_function_indexes.contains_key(&name) || builtin_index(&name).is_some() {
            return Err(self.error(format!("Struct {name:?} conflicts with an existing callable"), name_token));
        }
        let mut fields = Vec::new();
        let mut seen = HashSet::new();
        self.eat(TokenKind::LBrace)?;
        if self.current().kind != TokenKind::RBrace {
            loop {
                let field_token = self.eat(TokenKind::Ident)?;
                if !seen.insert(field_token.text.clone()) {
                    return Err(self.error(format!("Duplicate struct field {:?}", field_token.text), field_token));
                }
                self.intern_field(&field_token.text);
                fields.push(field_token.text);
                if self.current().kind != TokenKind::Comma {
                    break;
                }
                self.eat(TokenKind::Comma)?;
            }
        }
        self.eat(TokenKind::RBrace)?;

        let full_name = self.qualified_declaration_name(&name);
        let struct_index = {
            let mut state = self.shared.borrow_mut();
            if state.struct_indexes.contains_key(&full_name) {
                return Err(self.error(
                    format!("Struct {full_name:?} is already defined"),
                    Token {
                        kind: TokenKind::Ident,
                        text: name,
                        pos: 0,
                        end: 0,
                    },
                ));
            }
            let index = state.structs.len();
            state.struct_indexes.insert(full_name.clone(), index);
            state.structs.push(StructDef {
                name: full_name,
                fields: fields.into_boxed_slice().into_vec(),
            });
            index
        };
        self.local_struct_indexes.insert(name.clone(), struct_index);
        if let Some(filename) = &self.module_filename {
            let mut state = self.shared.borrow_mut();
            let Some(info) = state.modules.get_mut(filename) else {
                return Err(TinyLangError::compile("Internal module state error"));
            };
            info.all_structs.insert(name.clone());
            if exported {
                info.struct_exports.insert(name, struct_index);
            }
        }
        Ok(())
    }

    /// Parses `enum Name { Variant, Variant(field, field), ... }`.
    ///
    /// Enum declarations are file-local only (no cross-module export, unlike
    /// `struct`) — `exported` is accepted for call-site symmetry with
    /// `struct_definition`/`function_definition` but currently unused.
    fn enum_definition(&mut self, exported: bool) -> Result<()> {
        let _ = exported;
        self.eat(TokenKind::Enum)?;
        let name_token = self.eat(TokenKind::Ident)?;
        let name = name_token.text.clone();
        if self.has_namespace(&name) {
            return Err(self.error(format!("Enum {name:?} conflicts with an imported namespace"), name_token));
        }
        if self.local_enum_names.contains(&name) {
            return Err(self.error(format!("Enum {name:?} is already defined"), name_token));
        }
        if self.local_struct_indexes.contains_key(&name)
            || self.local_function_indexes.contains_key(&name)
            || builtin_index(&name).is_some()
        {
            return Err(self.error(format!("Enum {name:?} conflicts with an existing callable"), name_token));
        }

        let full_name = self.qualified_declaration_name(&name);
        self.eat(TokenKind::LBrace)?;
        let mut seen_variants = HashSet::new();
        let mut tag: u32 = 0;
        if self.current().kind != TokenKind::RBrace {
            loop {
                let variant_token = self.eat(TokenKind::Ident)?;
                if !seen_variants.insert(variant_token.text.clone()) {
                    return Err(self.error(format!("Duplicate enum variant {:?}", variant_token.text), variant_token));
                }

                let mut fields = Vec::new();
                if self.current().kind == TokenKind::LParen {
                    self.eat(TokenKind::LParen)?;
                    let mut seen_fields = HashSet::new();
                    if self.current().kind != TokenKind::RParen {
                        loop {
                            let field_token = self.eat(TokenKind::Ident)?;
                            if field_token.text == "tag" {
                                return Err(self.error("Enum variant field \"tag\" is reserved", field_token));
                            }
                            if !seen_fields.insert(field_token.text.clone()) {
                                return Err(self.error(
                                    format!("Duplicate enum variant field {:?}", field_token.text),
                                    field_token,
                                ));
                            }
                            self.intern_field(&field_token.text);
                            fields.push(field_token.text);
                            if self.current().kind != TokenKind::Comma {
                                break;
                            }
                            self.eat(TokenKind::Comma)?;
                        }
                    }
                    self.eat(TokenKind::RParen)?;
                }

                {
                    let mut state = self.shared.borrow_mut();
                    let variant_index = state.enum_variants.len();
                    state
                        .enum_variant_indexes
                        .insert((full_name.clone(), variant_token.text.clone()), variant_index);
                    state.enum_variants.push(EnumVariantDef {
                        enum_name: full_name.clone(),
                        variant_name: variant_token.text,
                        tag,
                        fields: fields.into_boxed_slice().into_vec(),
                    });
                }
                tag += 1;

                if self.current().kind != TokenKind::Comma {
                    break;
                }
                self.eat(TokenKind::Comma)?;
            }
        }
        self.eat(TokenKind::RBrace)?;

        self.local_enum_names.insert(name);
        Ok(())
    }

    fn function_definition(&mut self, exported: bool, decorators: Vec<Decorator>) -> Result<()> {
        self.eat(TokenKind::Fn)?;
        let name_token = self.eat(TokenKind::Ident)?;
        let name = name_token.text.clone();
        if name.starts_with(CLOSURE_NAME_PREFIX) {
            return Err(self.error(
                format!("Function name {name:?} uses the reserved prefix {CLOSURE_NAME_PREFIX:?}"),
                name_token,
            ));
        }
        if self.has_namespace(&name) {
            return Err(self.error(format!("Function {name:?} conflicts with an imported namespace"), name_token));
        }
        if self.local_function_indexes.contains_key(&name) {
            return Err(self.error(format!("Function {name:?} is already defined"), name_token));
        }
        if self.local_struct_indexes.contains_key(&name) || builtin_index(&name).is_some() {
            return Err(self.error(format!("Function {name:?} conflicts with an existing callable"), name_token));
        }
        let full_name = self.qualified_declaration_name(&name);
        let generic_params = self.generic_parameter_list()?;
        let function_index = {
            let mut state = self.shared.borrow_mut();
            if state.function_indexes.contains_key(&full_name) {
                return Err(self.error(
                    format!("Function {full_name:?} is already defined"),
                    Token {
                        kind: TokenKind::Ident,
                        text: name.clone(),
                        pos: 0,
                        end: 0,
                    },
                ));
            }
            let index = state.functions.len();
            state.function_indexes.insert(full_name.clone(), index);
            state.functions.push(placeholder_function(&full_name));
            index
        };
        self.local_function_indexes.insert(name.clone(), function_index);
        if let Some(filename) = &self.module_filename {
            let mut state = self.shared.borrow_mut();
            let Some(info) = state.modules.get_mut(filename) else {
                return Err(TinyLangError::compile("Internal module state error"));
            };
            info.all_functions.insert(name.clone());
            if exported {
                info.function_exports.insert(name.clone(), function_index);
            }
        }

        let mut function_symbols = SymbolTable::new();
        self.eat(TokenKind::LParen)?;
        let mut param_count = 0usize;
        if self.current().kind != TokenKind::RParen {
            loop {
                let param = self.eat(TokenKind::Ident)?;
                self.skip_optional_type_annotation(TokenKind::Colon)?;
                let Some(slot) = function_symbols.define_current(&param.text) else {
                    return Err(self.error(format!("Duplicate parameter {:?}", param.text), param));
                };
                debug_assert_eq!(slot, param_count);
                param_count += 1;
                if self.current().kind != TokenKind::Comma {
                    break;
                }
                self.eat(TokenKind::Comma)?;
            }
        }
        self.eat(TokenKind::RParen)?;
        self.skip_optional_type_annotation(TokenKind::Arrow)?;

        let global_slots = self.symbols.top_level_slots();
        let previous_symbols = std::mem::replace(&mut self.symbols, function_symbols);
        let previous_code = std::mem::take(&mut self.code);
        let previous_source_offsets = std::mem::take(&mut self.source_offsets);
        let previous_in_function = self.in_function;
        let previous_function_globals = std::mem::replace(&mut self.function_globals, global_slots);
        let previous_loops = std::mem::take(&mut self.loops);
        let previous_unsafe_depth = std::mem::replace(&mut self.unsafe_depth, 0);
        let previous_boxed_slots = std::mem::take(&mut self.boxed_slots);
        let prescanned_boxed_names = self.prescan_boxed_names();
        let previous_boxed_names = std::mem::replace(&mut self.boxed_names, prescanned_boxed_names);
        self.in_function = true;
        let result = (|| {
            self.box_parameters();
            self.block()?;
            self.emit(Op::PushInt, 0, 0);
            self.emit(Op::Return, 0, 0);
            Ok(Function {
                name: full_name,
                decorators,
                generic_params: generic_params.into_boxed_slice().into_vec(),
                param_count,
                code: std::mem::take(&mut self.code).into_boxed_slice().into_vec(),
                source: Some(BytecodeSource::new(
                    Arc::clone(&self.source_map),
                    std::mem::take(&mut self.source_offsets),
                )),
                slot_count: self.symbols.slot_count(),
                names: std::mem::take(&mut self.symbols.names).into_boxed_slice().into_vec(),
            })
        })();
        let function_symbols = std::mem::replace(&mut self.symbols, previous_symbols);
        self.code = previous_code;
        self.source_offsets = previous_source_offsets;
        self.in_function = previous_in_function;
        self.function_globals = previous_function_globals;
        self.loops = previous_loops;
        self.unsafe_depth = previous_unsafe_depth;
        self.boxed_slots = previous_boxed_slots;
        self.boxed_names = previous_boxed_names;
        drop(function_symbols);
        let function = result?;
        self.shared.borrow_mut().functions[function_index] = function;
        Ok(())
    }

    fn closure_literal(&mut self) -> Result<()> {
        let fn_token = self.eat(TokenKind::Fn)?;
        // Closure identities use a reserved prefix that user declarations cannot
        // spell (see `function_definition`), so they can never collide. The
        // slot is reserved up front so nested definitions keep stable indexes.
        let (full_name, function_index) = loop {
            let closure_name = format!("{CLOSURE_NAME_PREFIX}{}", self.next_closure_id);
            self.next_closure_id = self
                .next_closure_id
                .checked_add(1)
                .ok_or_else(|| self.error_at("Too many closure literals", fn_token.pos))?;
            let full_name = self.qualified_declaration_name(&closure_name);
            let mut state = self.shared.borrow_mut();
            if state.function_indexes.contains_key(&full_name) {
                continue;
            }
            let index = state.functions.len();
            state.function_indexes.insert(full_name.clone(), index);
            state.functions.push(placeholder_function(&full_name));
            break (full_name, index);
        };

        let mut function_symbols = SymbolTable::new();
        self.eat(TokenKind::LParen)?;
        let mut param_count = 0usize;
        if self.current().kind != TokenKind::RParen {
            loop {
                let param = self.eat(TokenKind::Ident)?;
                self.skip_optional_type_annotation(TokenKind::Colon)?;
                let Some(slot) = function_symbols.define_current(&param.text) else {
                    return Err(self.error(format!("Duplicate parameter {:?}", param.text), param));
                };
                debug_assert_eq!(slot, param_count);
                param_count += 1;
                if self.current().kind != TokenKind::Comma {
                    break;
                }
                self.eat(TokenKind::Comma)?;
            }
        }
        self.eat(TokenKind::RParen)?;
        self.skip_optional_type_annotation(TokenKind::Arrow)?;

        let global_slots = if self.in_function {
            self.function_globals.clone()
        } else {
            self.symbols.top_level_slots()
        };
        let outer_symbols = self.symbols.clone();
        let outer_capture_slots = self.capture_slots.clone();
        let outer_is_function = self.in_function;
        let restore_parent_capture_context = self.capture_outer_symbols.is_some();
        if restore_parent_capture_context {
            self.enclosing_captures.push(EnclosingCaptureContext {
                captures: std::mem::take(&mut self.captures),
                capture_slots: std::mem::take(&mut self.capture_slots),
                outer_symbols: self.capture_outer_symbols.take(),
                outer_slots: std::mem::take(&mut self.capture_outer_slots),
                outer_globals: std::mem::take(&mut self.capture_outer_globals),
                outer_function: std::mem::replace(&mut self.capture_outer_function, false),
                outer_boxed: std::mem::take(&mut self.capture_outer_boxed),
            });
        }
        let previous_capture_outer_boxed = std::mem::replace(&mut self.capture_outer_boxed, self.boxed_slots.clone());
        let previous_boxed_slots = std::mem::take(&mut self.boxed_slots);
        let prescanned_boxed_names = self.prescan_boxed_names();
        let previous_boxed_names = std::mem::replace(&mut self.boxed_names, prescanned_boxed_names);
        let previous_symbols = std::mem::replace(&mut self.symbols, function_symbols);
        let previous_code = std::mem::take(&mut self.code);
        let previous_source_offsets = std::mem::take(&mut self.source_offsets);
        let previous_in_function = std::mem::replace(&mut self.in_function, true);
        let previous_function_globals = std::mem::replace(&mut self.function_globals, global_slots.clone());
        let previous_captures = std::mem::take(&mut self.captures);
        let previous_capture_slots = std::mem::take(&mut self.capture_slots);
        let previous_capture_outer_symbols = self.capture_outer_symbols.replace(outer_symbols);
        let previous_capture_outer_slots = std::mem::replace(&mut self.capture_outer_slots, outer_capture_slots);
        let previous_capture_outer_globals = std::mem::replace(&mut self.capture_outer_globals, global_slots);
        let previous_capture_outer_function = std::mem::replace(&mut self.capture_outer_function, outer_is_function);
        let previous_loops = std::mem::take(&mut self.loops);
        let previous_unsafe_depth = std::mem::replace(&mut self.unsafe_depth, 0);

        let result = (|| {
            self.box_parameters();
            self.block()?;
            self.emit(Op::PushInt, 0, 0);
            self.emit(Op::Return, 0, 0);

            let local_slot_count = self.symbols.slot_count();
            let captures = std::mem::take(&mut self.captures);
            let capture_count = captures.len();
            remap_capture_slots(&mut self.code, capture_count)?;
            let param_count = param_count
                .checked_add(capture_count)
                .ok_or_else(|| self.error_at("Closure parameter count overflow", fn_token.pos))?;
            let slot_count = local_slot_count
                .checked_add(capture_count)
                .ok_or_else(|| self.error_at("Closure slot count overflow", fn_token.pos))?;
            let names = captures
                .iter()
                .map(|capture| capture.name.clone())
                .chain(std::mem::take(&mut self.symbols.names))
                .collect::<Vec<_>>();

            Ok((
                Function {
                    name: full_name.clone(),
                    decorators: Vec::new(),
                    generic_params: Vec::new(),
                    param_count,
                    code: std::mem::take(&mut self.code).into_boxed_slice().into_vec(),
                    source: Some(BytecodeSource::new(
                        Arc::clone(&self.source_map),
                        std::mem::take(&mut self.source_offsets),
                    )),
                    slot_count,
                    names,
                },
                captures,
            ))
        })();

        self.symbols = previous_symbols;
        self.code = previous_code;
        self.source_offsets = previous_source_offsets;
        self.in_function = previous_in_function;
        self.function_globals = previous_function_globals;
        self.captures = previous_captures;
        self.capture_slots = previous_capture_slots;
        self.capture_outer_symbols = previous_capture_outer_symbols;
        self.capture_outer_slots = previous_capture_outer_slots;
        self.capture_outer_globals = previous_capture_outer_globals;
        self.capture_outer_function = previous_capture_outer_function;
        self.capture_outer_boxed = previous_capture_outer_boxed;
        self.boxed_slots = previous_boxed_slots;
        self.boxed_names = previous_boxed_names;
        self.loops = previous_loops;
        self.unsafe_depth = previous_unsafe_depth;
        if restore_parent_capture_context {
            let Some(parent) = self.enclosing_captures.pop() else {
                return Err(TinyLangError::compile("Internal closure capture context error"));
            };
            self.captures = parent.captures;
            self.capture_slots = parent.capture_slots;
            self.capture_outer_symbols = parent.outer_symbols;
            self.capture_outer_slots = parent.outer_slots;
            self.capture_outer_globals = parent.outer_globals;
            self.capture_outer_function = parent.outer_function;
            self.capture_outer_boxed = parent.outer_boxed;
        }

        let (function, captures) = result?;
        self.shared.borrow_mut().functions[function_index] = function;
        self.emit_closure_value(function_index, &captures, fn_token.pos)
    }

    fn emit_closure_value(&mut self, function_index: usize, captures: &[Capture], pos: usize) -> Result<()> {
        for capture in captures {
            match capture.source.slot {
                ReadSlot::Local(slot) => self.emit_at(Op::Load, slot as i64, 0, pos),
                ReadSlot::Global(slot) => self.emit_at(Op::LoadGlobal, slot as i64, 0, pos),
                ReadSlot::Capture(index) => {
                    self.emit_at(Op::Load, capture_slot_operand(index)?, 0, pos);
                }
            }
        }
        let capture_count =
            i64::try_from(captures.len()).map_err(|_| self.error_at("Too many closure captures", pos))?;
        let function_index = i64::try_from(function_index).map_err(|_| self.error_at("Too many functions", pos))?;
        self.emit_at(Op::MakeClosure, function_index, capture_count, pos);
        Ok(())
    }

    /// Parses the v2 declaration form `fn identity<T>(value: T) -> T`.
    /// `TinyLang` is dynamically typed, so generic parameters are erased from
    /// execution; the names remain in `Function` metadata for tooling and
    /// artifact consumers.
    fn generic_parameter_list(&mut self) -> Result<Vec<String>> {
        if self.current().kind != TokenKind::Lt {
            return Ok(Vec::new());
        }
        self.eat(TokenKind::Lt)?;
        let mut params = Vec::new();
        loop {
            let token = self.eat(TokenKind::Ident)?;
            if params.contains(&token.text) {
                return Err(self.error(format!("Duplicate generic parameter {:?}", token.text), token));
            }
            params.push(token.text);
            if self.current().kind != TokenKind::Comma {
                break;
            }
            self.eat(TokenKind::Comma)?;
        }
        self.eat(TokenKind::Gt)?;
        Ok(params)
    }

    fn qualified_declaration_name(&self, name: &str) -> String {
        match &self.module_qualified_name {
            Some(module_name) => format!("{module_name}.{name}"),
            None => name.to_string(),
        }
    }

    fn block(&mut self) -> Result<()> {
        self.eat(TokenKind::LBrace)?;
        self.symbols.enter_scope();
        let result = (|| {
            while self.current().kind != TokenKind::RBrace {
                if self.current().kind == TokenKind::Eof {
                    return Err(self.error("Unterminated block", self.current().clone()));
                }
                self.statement()?;
            }
            self.eat(TokenKind::RBrace)?;
            Ok(())
        })();
        self.symbols.exit_scope()?;
        result
    }

    fn expression(&mut self) -> Result<()> {
        self.logical_or()
    }

    fn logical_or(&mut self) -> Result<()> {
        self.logical_and()?;
        while self.current().kind == TokenKind::PipePipe {
            self.eat(TokenKind::PipePipe)?;
            let rhs_jump = self.emit_placeholder(Op::JumpIfZero);
            self.emit(Op::PushInt, 1, 0);
            let end_jump = self.emit_placeholder(Op::Jump);
            self.patch(rhs_jump, self.code.len() as i64)?;
            self.logical_and()?;
            let false_jump = self.emit_placeholder(Op::JumpIfZero);
            self.emit(Op::PushInt, 1, 0);
            let rhs_end_jump = self.emit_placeholder(Op::Jump);
            self.patch(false_jump, self.code.len() as i64)?;
            self.emit(Op::PushInt, 0, 0);
            self.patch(rhs_end_jump, self.code.len() as i64)?;
            self.patch(end_jump, self.code.len() as i64)?;
        }
        Ok(())
    }

    fn logical_and(&mut self) -> Result<()> {
        self.comparison()?;
        while self.current().kind == TokenKind::AmpAmp {
            self.eat(TokenKind::AmpAmp)?;
            let lhs_false_jump = self.emit_placeholder(Op::JumpIfZero);
            self.comparison()?;
            let rhs_false_jump = self.emit_placeholder(Op::JumpIfZero);
            self.emit(Op::PushInt, 1, 0);
            let end_jump = self.emit_placeholder(Op::Jump);
            self.patch(lhs_false_jump, self.code.len() as i64)?;
            self.patch(rhs_false_jump, self.code.len() as i64)?;
            self.emit(Op::PushInt, 0, 0);
            self.patch(end_jump, self.code.len() as i64)?;
        }
        Ok(())
    }

    fn comparison(&mut self) -> Result<()> {
        self.binary_level(Self::additive, comparison_op)
    }

    fn additive(&mut self) -> Result<()> {
        self.binary_level(Self::term, additive_op)
    }

    fn term(&mut self) -> Result<()> {
        self.binary_level(Self::factor, term_op)
    }

    fn binary_level(
        &mut self,
        parse_operand: fn(&mut Self) -> Result<()>,
        op_lookup: fn(TokenKind) -> Option<Op>,
    ) -> Result<()> {
        parse_operand(self)?;
        while let Some(op) = op_lookup(self.current().kind) {
            let kind = self.current().kind;
            let token = self.eat(kind)?;
            parse_operand(self)?;
            self.emit_at(op, 0, 0, token.pos);
        }
        Ok(())
    }

    fn factor(&mut self) -> Result<()> {
        self.primary()?;
        loop {
            match self.current().kind {
                TokenKind::LBracket => {
                    let token = self.eat(TokenKind::LBracket)?;
                    self.expression()?;
                    self.eat(TokenKind::RBracket)?;
                    self.emit_at(Op::Index, 0, 0, token.pos);
                }
                TokenKind::Dot => {
                    self.eat(TokenKind::Dot)?;
                    let field = self.eat(TokenKind::Ident)?.text;
                    let field_index = self.intern_field(&field);
                    self.emit(Op::GetField, field_index as i64, 0);
                }
                // A `(` on a later line starts a new statement, not a call.
                TokenKind::LParen if !self.paren_starts_new_line() => {
                    let token = self.eat(TokenKind::LParen)?;
                    let argc = self.argument_list()?;
                    self.emit_at(Op::CallValue, argc as i64, 0, token.pos);
                }
                _ => break,
            }
        }
        Ok(())
    }

    fn primary(&mut self) -> Result<()> {
        match self.current().kind {
            TokenKind::Int => {
                let token = self.eat(TokenKind::Int)?;
                let value = token
                    .text
                    .parse::<i64>()
                    .map_err(|_| self.error(format!("Integer literal {:?} is out of range", token.text), token))?;
                self.emit(Op::PushInt, value, 0);
            }
            TokenKind::Float => {
                let token = self.eat(TokenKind::Float)?;
                let value = token
                    .text
                    .parse::<f64>()
                    .map_err(|_| self.error(format!("Float literal {:?} is out of range", token.text), token))?;
                // Floats have no dedicated constant pool; the bit pattern is
                // carried directly in the instruction's i64 arg (matching
                // the JSON artifact format's existing raw-bits convention
                // for floats) and reinterpreted by PushFloat at execution.
                self.emit(Op::PushFloat, value.to_bits() as i64, 0);
            }
            TokenKind::String => {
                let token = self.eat(TokenKind::String)?;
                let index = self.intern_string(&token.text);
                self.emit(Op::PushString, index as i64, 0);
            }
            TokenKind::Null => {
                self.eat(TokenKind::Null)?;
                self.emit(Op::PushNull, 0, 0);
            }
            TokenKind::True => {
                self.eat(TokenKind::True)?;
                self.emit(Op::PushBool, 1, 0);
            }
            TokenKind::False => {
                self.eat(TokenKind::False)?;
                self.emit(Op::PushBool, 0, 0);
            }
            TokenKind::Fn => self.closure_literal()?,
            TokenKind::LBracket => {
                let token = self.eat(TokenKind::LBracket)?;
                let mut count = 0i64;
                if self.current().kind != TokenKind::RBracket {
                    loop {
                        self.expression()?;
                        count += 1;
                        if self.current().kind != TokenKind::Comma {
                            break;
                        }
                        self.eat(TokenKind::Comma)?;
                    }
                }
                self.eat(TokenKind::RBracket)?;
                self.emit_at(Op::MakeArray, count, 0, token.pos);
            }
            TokenKind::Ident => {
                if self.is_qualified_call() {
                    let namespace = self.eat(TokenKind::Ident)?;
                    self.eat(TokenKind::Dot)?;
                    let member = self.eat(TokenKind::Ident)?;
                    self.qualified_call_expression(&namespace, &member)?;
                } else {
                    let name = self.eat(TokenKind::Ident)?;
                    if self.current().kind == TokenKind::LParen {
                        self.call_expression(&name)?;
                    } else if self.local_function_indexes.contains_key(&name.text) {
                        self.emit(Op::PushFunction, self.local_function_indexes[&name.text] as i64, 0);
                    } else {
                        self.emit_load_name(&name)?;
                    }
                }
            }
            TokenKind::LParen => {
                self.eat(TokenKind::LParen)?;
                self.expression()?;
                self.eat(TokenKind::RParen)?;
            }
            TokenKind::Minus => {
                let token = self.eat(TokenKind::Minus)?;
                self.factor()?;
                self.emit_at(Op::Neg, 0, 0, token.pos);
            }
            TokenKind::Bang => {
                self.eat(TokenKind::Bang)?;
                self.factor()?;
                let true_jump = self.emit_placeholder(Op::JumpIfZero);
                self.emit(Op::PushInt, 0, 0);
                let end_jump = self.emit_placeholder(Op::Jump);
                self.patch(true_jump, self.code.len() as i64)?;
                self.emit(Op::PushInt, 1, 0);
                self.patch(end_jump, self.code.len() as i64)?;
            }
            TokenKind::Unsafe => {
                self.eat(TokenKind::Unsafe)?;
                self.unsafe_depth += 1;
                let result = self.factor();
                self.unsafe_depth -= 1;
                result?;
            }
            _ => return Err(self.error("Expected expression", self.current().clone())),
        }
        Ok(())
    }

    fn is_qualified_call(&self) -> bool {
        self.current().kind == TokenKind::Ident
            && self.peek_kind(1) == Some(TokenKind::Dot)
            && self.peek_kind(2) == Some(TokenKind::Ident)
            && self.peek_kind(3) == Some(TokenKind::LParen)
    }

    fn is_expression_start(&self) -> bool {
        matches!(
            self.current().kind,
            TokenKind::Int
                | TokenKind::Float
                | TokenKind::String
                | TokenKind::Null
                | TokenKind::True
                | TokenKind::False
                | TokenKind::Ident
                | TokenKind::LBracket
                | TokenKind::LParen
                | TokenKind::Minus
                | TokenKind::Bang
                | TokenKind::Unsafe
                | TokenKind::Fn
        )
    }

    fn call_expression(&mut self, name: &Token) -> Result<()> {
        if let Some(struct_index) = self.local_struct_indexes.get(&name.text).copied() {
            return self.constructor_call(&name.text, struct_index, name.pos);
        }
        // Callees resolve through the same lexical chain as reads: locals, then
        // enclosing captures, then top-level variables, then builtins and named
        // functions. User bindings therefore shadow builtin names.
        let is_value = self.symbols.get(&name.text).is_some()
            || self.capture_slot(name).is_some()
            || (self.in_function
                && self.function_globals.contains_key(&name.text)
                && !self.local_function_indexes.contains_key(&name.text));
        if is_value {
            self.emit_load_name(name)?;
            self.eat(TokenKind::LParen)?;
            let arg_count = self.argument_list()?;
            self.emit_at(Op::CallValue, arg_count as i64, 0, name.pos);
            return Ok(());
        }
        if let Some(builtin_index) = builtin_index(&name.text) {
            return self.builtin_call(&name.text, builtin_index, name.pos);
        }
        let function_index = self
            .local_function_indexes
            .get(&name.text)
            .copied()
            .ok_or_else(|| self.error_at(format!("Undefined function or constructor {:?}", name.text), name.pos))?;
        self.eat(TokenKind::LParen)?;
        let arg_count = self.argument_list()?;
        self.emit_at(Op::Call, function_index as i64, arg_count as i64, name.pos);
        Ok(())
    }

    fn qualified_call_expression(&mut self, namespace: &Token, member: &Token) -> Result<()> {
        if self.local_enum_names.contains(&namespace.text) {
            return self.enum_constructor_call(namespace, member);
        }
        let info =
            self.namespaces.get(&namespace.text).cloned().ok_or_else(|| {
                self.error_at(format!("Unknown module namespace {:?}", namespace.text), namespace.pos)
            })?;
        if let Some(struct_index) = info.struct_exports.get(&member.text).copied() {
            return self.constructor_call(&format!("{}.{}", namespace.text, member.text), struct_index, member.pos);
        }
        if let Some(function_index) = info.function_exports.get(&member.text).copied() {
            self.eat(TokenKind::LParen)?;
            let arg_count = self.argument_list()?;
            self.emit_at(Op::Call, function_index as i64, arg_count as i64, member.pos);
            return Ok(());
        }
        if info.all_functions.contains(&member.text) || info.all_structs.contains(&member.text) {
            return Err(
                self.error_at(format!("Module member {}.{} is not exported", namespace.text, member.text), member.pos)
            );
        }
        Err(self.error_at(format!("Module {:?} has no exported member {:?}", namespace.text, member.text), member.pos))
    }

    fn constructor_call(&mut self, name: &str, struct_index: usize, pos: usize) -> Result<()> {
        let field_count = self.shared.borrow().structs[struct_index].fields.len();
        self.eat(TokenKind::LParen)?;
        let arg_count = self.argument_list()?;
        if arg_count != field_count {
            return Err(self.error_at(
                format!("Struct {name:?} expects {field_count} field value(s), got {arg_count}"),
                self.current().pos,
            ));
        }
        self.emit_at(Op::MakeStruct, struct_index as i64, arg_count as i64, pos);
        Ok(())
    }

    fn enum_constructor_call(&mut self, namespace: &Token, member: &Token) -> Result<()> {
        let full_name = self.qualified_declaration_name(&namespace.text);
        let variant_index = {
            let state = self.shared.borrow();
            state
                .enum_variant_indexes
                .get(&(full_name, member.text.clone()))
                .copied()
        }
        .ok_or_else(|| {
            self.error_at(format!("Enum {:?} has no variant {:?}", namespace.text, member.text), member.pos)
        })?;
        let field_count = self.shared.borrow().enum_variants[variant_index].fields.len();
        self.eat(TokenKind::LParen)?;
        let arg_count = self.argument_list()?;
        if arg_count != field_count {
            return Err(self.error_at(
                format!(
                    "Enum variant {}.{} expects {field_count} field value(s), got {arg_count}",
                    namespace.text, member.text
                ),
                self.current().pos,
            ));
        }
        self.emit_at(Op::MakeEnum, variant_index as i64, arg_count as i64, member.pos);
        Ok(())
    }

    fn builtin_call(&mut self, name: &str, builtin_index: usize, pos: usize) -> Result<()> {
        let builtin = BUILTINS[builtin_index];
        self.eat(TokenKind::LParen)?;
        let arg_count = self.argument_list()?;
        if arg_count < builtin.min_args || arg_count > builtin.max_args {
            let expected = if builtin.min_args == builtin.max_args {
                builtin.min_args.to_string()
            } else {
                format!("{}..{}", builtin.min_args, builtin.max_args)
            };
            return Err(self.error_at(
                format!("Builtin {name:?} expects {expected} argument(s), got {arg_count}"),
                self.current().pos,
            ));
        }
        if builtin.requires_unsafe && self.unsafe_depth == 0 {
            return Err(self.error_at(format!("Builtin {name:?} requires unsafe syntax"), pos));
        }
        self.emit_at(Op::Builtin, builtin_index as i64, arg_count as i64, pos);
        Ok(())
    }

    fn argument_list(&mut self) -> Result<usize> {
        let mut count = 0usize;
        if self.current().kind != TokenKind::RParen {
            loop {
                self.expression()?;
                count += 1;
                if self.current().kind != TokenKind::Comma {
                    break;
                }
                self.eat(TokenKind::Comma)?;
            }
        }
        self.eat(TokenKind::RParen)?;
        Ok(count)
    }

    fn current(&self) -> &Token {
        self.tokens.as_slice().first().unwrap_or(&self.eof_token)
    }

    fn peek_kind(&self, offset: usize) -> Option<TokenKind> {
        self.tokens.as_slice().get(offset).map(|token| token.kind)
    }

    fn eat(&mut self, kind: TokenKind) -> Result<Token> {
        let token = self.current();
        if token.kind != kind {
            return Err(self.error(format!("Expected {}, got {}", kind.name(), token.kind.name()), token.clone()));
        }
        let token = self.tokens.next().unwrap_or_else(|| self.eof_token.clone());
        self.last_source_offset = SourceOffset(token.pos);
        self.last_token_end = token.end;
        Ok(token)
    }

    /// Postfix `(` must appear on the same line as the callee, since the grammar has no statement terminators.
    fn paren_starts_new_line(&self) -> bool {
        let pos = self.current().pos;
        let start = self.last_token_end.min(pos);
        self.source_map
            .source()
            .get(start..pos)
            .is_some_and(|text| text.contains('\n'))
    }

    fn emit(&mut self, op: Op, arg: i64, arg2: i64) {
        self.code.push(Instr::new(op, arg, arg2));
        self.source_offsets.push(self.last_source_offset);
    }

    fn emit_at(&mut self, op: Op, arg: i64, arg2: i64, pos: usize) {
        self.code.push(Instr::new(op, arg, arg2));
        self.source_offsets.push(SourceOffset(pos));
    }

    fn emit_placeholder(&mut self, op: Op) -> usize {
        let index = self.code.len();
        self.emit(op, -1, 0);
        index
    }

    fn patch(&mut self, index: usize, arg: i64) -> Result<()> {
        let Some(instr) = self.code.get_mut(index) else {
            return Err(TinyLangError::compile("Internal compiler patch target error"));
        };
        instr.arg = arg;
        Ok(())
    }

    fn get_assignment_slot(&mut self, token: &Token) -> Result<ReadSlot> {
        if let Some(slot) = self.symbols.get(&token.text) {
            return Ok(ReadSlot::Local(slot));
        }
        if let Some(slot) = self.capture_slot(token) {
            return Ok(ReadSlot::Capture(slot));
        }
        if self.in_function && self.function_globals.contains_key(&token.text) {
            return Err(self.error(
                format!("Cannot assign to top-level variable {:?} inside a function", token.text),
                token.clone(),
            ));
        }
        Err(self.error(format!("Undefined variable {:?}", token.text), token.clone()))
    }

    fn get_read_slot(&mut self, token: &Token) -> Result<ReadSlot> {
        if let Some(slot) = self.symbols.get(&token.text) {
            return Ok(ReadSlot::Local(slot));
        }
        if let Some(slot) = self.capture_slot(token) {
            return Ok(ReadSlot::Capture(slot));
        }
        if self.in_function
            && let Some(slot) = self.function_globals.get(&token.text).copied()
        {
            return Ok(ReadSlot::Global(slot));
        }
        Err(self.error(format!("Undefined variable {:?}", token.text), token.clone()))
    }

    fn read_slot_is_boxed(&self, slot: ReadSlot) -> bool {
        match slot {
            ReadSlot::Local(slot) => self.boxed_slots.contains(&slot),
            ReadSlot::Global(_) => false,
            ReadSlot::Capture(index) => self.captures.get(index).is_some_and(|capture| capture.boxed),
        }
    }

    fn emit_load_name(&mut self, token: &Token) -> Result<()> {
        let slot = self.get_read_slot(token)?;
        match slot {
            ReadSlot::Local(slot) => self.emit_at(Op::Load, slot as i64, 0, token.pos),
            ReadSlot::Global(slot) => self.emit_at(Op::LoadGlobal, slot as i64, 0, token.pos),
            ReadSlot::Capture(index) => self.emit_at(Op::Load, capture_slot_operand(index)?, 0, token.pos),
        }
        if self.read_slot_is_boxed(slot) {
            self.emit_at(Op::LoadCell, 0, 0, token.pos);
        }
        Ok(())
    }

    fn emit_store_slot(&mut self, slot: ReadSlot, pos: usize) -> Result<()> {
        let operand = match slot {
            ReadSlot::Local(slot) => i64::try_from(slot).map_err(|_| self.error_at("Local slot is too large", pos))?,
            ReadSlot::Global(_) => return Err(self.error_at("Cannot assign to a global variable", pos)),
            ReadSlot::Capture(index) => capture_slot_operand(index)?,
        };
        if self.read_slot_is_boxed(slot) {
            self.emit_at(Op::Load, operand, 0, pos);
            self.emit_at(Op::StoreCell, 0, 0, pos);
        } else {
            self.emit_at(Op::Store, operand, 0, pos);
        }
        Ok(())
    }

    /// Wraps a freshly defined local in a shared cell when a nested closure mutates or observes mutation of it.
    fn box_parameters(&mut self) {
        let names: Vec<(String, usize)> = self
            .boxed_names
            .iter()
            .filter_map(|name| self.symbols.get(name).map(|slot| (name.clone(), slot)))
            .collect();
        for (_, slot) in names {
            if self.boxed_slots.insert(slot) {
                self.emit(Op::Load, slot as i64, 0);
                self.emit(Op::MakeCell, 0, 0);
                self.emit(Op::Store, slot as i64, 0);
            }
        }
    }

    /// Conservatively finds names in the upcoming `{ ... }` body that are both assigned and referenced
    /// inside a nested closure. Those bindings need shared cells; everything else is captured by value.
    fn prescan_boxed_names(&self) -> HashSet<String> {
        let mut assigned = HashSet::new();
        let mut nested = HashSet::new();
        let mut depth = 0usize;
        // Brace depths at which nested function bodies started.
        let mut nested_bodies: Vec<usize> = Vec::new();
        let mut pending_fn = false;
        let mut previous: Option<TokenKind> = None;
        let tokens = self.tokens.as_slice();
        for (index, token) in tokens.iter().enumerate() {
            match token.kind {
                TokenKind::Fn => pending_fn = true,
                TokenKind::LBrace => {
                    depth += 1;
                    if pending_fn {
                        nested_bodies.push(depth);
                        pending_fn = false;
                    }
                }
                TokenKind::RBrace => {
                    if nested_bodies.last() == Some(&depth) {
                        nested_bodies.pop();
                    }
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        break;
                    }
                }
                TokenKind::Ident => {
                    let next = tokens.get(index + 1).map(|token| token.kind);
                    if next == Some(TokenKind::Equal) && previous != Some(TokenKind::Let) {
                        assigned.insert(token.text.clone());
                    }
                    if !nested_bodies.is_empty() {
                        nested.insert(token.text.clone());
                    }
                }
                TokenKind::Eof => break,
                _ => {}
            }
            previous = Some(token.kind);
        }
        assigned.intersection(&nested).cloned().collect()
    }

    fn capture_source_boxed(&self, context_index: Option<usize>, source: ReadSlot) -> bool {
        // `context_index` identifies the saved context whose captures/outer state describe `source`;
        // `None` means the closure currently being compiled.
        match (context_index, source) {
            (None, ReadSlot::Local(slot)) => self.capture_outer_boxed.contains(&slot),
            (Some(index), ReadSlot::Local(slot)) => self
                .enclosing_captures
                .get(index)
                .is_some_and(|context| context.outer_boxed.contains(&slot)),
            (None, ReadSlot::Capture(slot)) => self
                .enclosing_captures
                .last()
                .and_then(|context| context.captures.get(slot))
                .is_some_and(|capture| capture.boxed),
            (Some(index), ReadSlot::Capture(slot)) => index
                .checked_sub(1)
                .and_then(|parent| self.enclosing_captures.get(parent))
                .and_then(|context| context.captures.get(slot))
                .is_some_and(|capture| capture.boxed),
            (_, ReadSlot::Global(_)) => false,
        }
    }

    fn capture_slot(&mut self, token: &Token) -> Option<usize> {
        if let Some(slot) = self.capture_slots.get(&token.text).copied() {
            return Some(slot);
        }

        let local = self
            .capture_outer_symbols
            .as_ref()
            .and_then(|symbols| symbols.get(&token.text))
            .filter(|_| self.capture_outer_function || !self.capture_outer_globals.contains_key(&token.text))
            .map(ReadSlot::Local);
        let source = if let Some(source) = local {
            source
        } else if let Some(slot) = self.capture_outer_slots.get(&token.text).copied() {
            ReadSlot::Capture(slot)
        } else {
            let parent = self.enclosing_captures.len().checked_sub(1)?;
            if !self.capture_outer_function {
                return None;
            }
            ReadSlot::Capture(self.ensure_enclosing_capture(parent, &token.text)?)
        };
        let boxed = self.capture_source_boxed(None, source);
        let index = self.captures.len();
        self.capture_slots.insert(token.text.clone(), index);
        self.captures.push(Capture {
            name: token.text.clone(),
            source: CaptureSource { slot: source },
            boxed,
        });
        Some(index)
    }

    fn ensure_enclosing_capture(&mut self, context_index: usize, name: &str) -> Option<usize> {
        if let Some(slot) = self
            .enclosing_captures
            .get(context_index)?
            .capture_slots
            .get(name)
            .copied()
        {
            return Some(slot);
        }

        let direct_source = {
            let context = self.enclosing_captures.get(context_index)?;
            let local = context
                .outer_symbols
                .as_ref()
                .and_then(|symbols| symbols.get(name))
                .filter(|_| context.outer_function || !context.outer_globals.contains_key(name))
                .map(ReadSlot::Local);
            local.or_else(|| context.outer_slots.get(name).copied().map(ReadSlot::Capture))
        };
        let source = if let Some(source) = direct_source {
            source
        } else {
            let context = self.enclosing_captures.get(context_index)?;
            if !context.outer_function || context_index == 0 {
                return None;
            }
            ReadSlot::Capture(self.ensure_enclosing_capture(context_index - 1, name)?)
        };

        let boxed = self.capture_source_boxed(Some(context_index), source);
        let context = self.enclosing_captures.get_mut(context_index)?;
        let slot = context.captures.len();
        context.capture_slots.insert(name.to_string(), slot);
        context.captures.push(Capture {
            name: name.to_string(),
            source: CaptureSource { slot: source },
            boxed,
        });
        Some(slot)
    }

    fn intern_string(&self, text: &str) -> usize {
        let mut state = self.shared.borrow_mut();
        if let Some(index) = state.string_indexes.get(text) {
            return *index;
        }
        let index = state.strings.len();
        state.string_indexes.insert(text.to_string(), index);
        state.strings.push(text.to_string());
        index
    }

    fn intern_field(&self, text: &str) -> usize {
        let mut state = self.shared.borrow_mut();
        if let Some(index) = state.field_indexes.get(text) {
            return *index;
        }
        let index = state.fields.len();
        state.field_indexes.insert(text.to_string(), index);
        state.fields.push(text.to_string());
        index
    }

    fn error(&self, message: impl AsRef<str>, token: Token) -> TinyLangError {
        TinyLangError::compile(self.source_map.format(message, token.pos, token.end))
    }

    fn error_at(&self, message: impl AsRef<str>, pos: usize) -> TinyLangError {
        TinyLangError::compile(self.source_map.format(message, pos, pos + 1))
    }
}

fn comparison_op(kind: TokenKind) -> Option<Op> {
    Some(match kind {
        TokenKind::Lt => Op::Lt,
        TokenKind::Lte => Op::Lte,
        TokenKind::Gt => Op::Gt,
        TokenKind::Gte => Op::Gte,
        TokenKind::EqEq => Op::Eq,
        TokenKind::BangEqual => Op::Ne,
        _ => return None,
    })
}

fn additive_op(kind: TokenKind) -> Option<Op> {
    Some(match kind {
        TokenKind::Plus => Op::Add,
        TokenKind::Minus => Op::Sub,
        _ => return None,
    })
}

fn term_op(kind: TokenKind) -> Option<Op> {
    Some(match kind {
        TokenKind::Star => Op::Mul,
        TokenKind::Slash => Op::Div,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::fmt::Write as _;
    use std::mem::{size_of, size_of_val};

    use super::*;
    use crate::CompilerSharedState;

    fn compiler(source: &str) -> Compiler {
        Compiler::new(
            source,
            "tokens.to",
            None,
            false,
            "",
            ModulePermissions::from_capabilities(ModuleCapabilities::all()),
            Rc::new(RefCell::new(CompilerSharedState::default())),
        )
        .expect("lex compiler fixture")
    }

    #[test]
    fn program_finishing_moves_contents_and_compacts_completed_buffers() {
        let mut compiler = compiler(
            "struct Pair { left, right }\nenum Choice { None, Some(value) }\n\
             fn identity<T>(value: T) -> T { return value }\nlet root = \"payload\"",
        );
        compiler.struct_definition(false).expect("parse struct");
        compiler.enum_definition(false).expect("parse enum");
        compiler.function_definition(false, Vec::new()).expect("parse function");
        compiler.let_statement().expect("parse root variable");
        let mut module = Compiler::new(
            "export fn answer() { return 42 }",
            "library.to",
            None,
            true,
            "library",
            ModulePermissions::from_capabilities(ModuleCapabilities::none()),
            Rc::clone(&compiler.shared),
        )
        .expect("construct module");
        module.export_declaration(Vec::new()).expect("parse module export");
        module.finalize_module().expect("finish module metadata");
        drop(module);

        let names = compiler.symbols.names[0].as_ptr();
        let (functions, strings, structs, fields, modules, variants, function_code, function_names) = {
            let state = compiler.shared.borrow();
            (
                state.functions[0].name.as_ptr(),
                state.strings[0].as_ptr(),
                state.structs[0].name.as_ptr(),
                state.fields[0].as_ptr(),
                state.module_defs[0].name.as_ptr(),
                state.enum_variants[0].enum_name.as_ptr(),
                state.functions[0].code.as_ptr(),
                state.functions[0].names.as_ptr(),
            )
        };
        let program = compiler.compile().expect("finish program");
        assert_eq!(program.names[0].as_ptr(), names);
        assert_eq!(program.functions[0].name.as_ptr(), functions);
        assert_eq!(program.strings[0].as_ptr(), strings);
        assert_eq!(program.structs[0].name.as_ptr(), structs);
        assert_eq!(program.fields[0].as_ptr(), fields);
        assert_eq!(program.modules[0].name.as_ptr(), modules);
        assert_eq!(program.enum_variants[0].enum_name.as_ptr(), variants);
        assert_eq!(program.functions[0].code.as_ptr(), function_code);
        assert_eq!(program.functions[0].names.as_ptr(), function_names);
        assert_eq!(program.functions[0].generic_params, ["T"]);
        assert_eq!(program.names, ["root"]);
        assert_eq!(program.strings, ["payload"]);
        assert_eq!(program.modules[0].exported_functions, ["answer"]);
        for (length, capacity) in [
            (program.code.len(), program.code.capacity()),
            (program.names.len(), program.names.capacity()),
            (program.functions.len(), program.functions.capacity()),
            (program.strings.len(), program.strings.capacity()),
            (program.structs.len(), program.structs.capacity()),
            (program.fields.len(), program.fields.capacity()),
            (program.modules.len(), program.modules.capacity()),
            (program.enum_variants.len(), program.enum_variants.capacity()),
            (program.functions[0].code.len(), program.functions[0].code.capacity()),
            (program.functions[0].names.len(), program.functions[0].names.capacity()),
            (program.functions[0].generic_params.len(), program.functions[0].generic_params.capacity()),
            (program.structs[0].fields.len(), program.structs[0].fields.capacity()),
            (program.enum_variants[1].fields.len(), program.enum_variants[1].fields.capacity()),
        ] {
            assert_eq!(capacity, length, "finished buffer retains no spare capacity");
        }
        crate::BytecodeVerifier::verify(&program).expect("transferred metadata remains valid");
    }

    #[test]
    fn program_finishing_rejects_live_shared_owners_without_draining_them() {
        let compiler = compiler("fn retained() { return 1 }");
        let shared = Rc::clone(&compiler.shared);
        let error = compiler.compile().expect_err("another compiler still owns the session");
        assert_eq!(error.to_string(), "Internal compiler state still has active owners");
        assert_eq!(shared.borrow().functions.len(), 1);
        assert_eq!(shared.borrow().functions[0].name, "retained");
    }

    #[test]
    fn imported_unit_contributes_definitions_without_a_main_buffer() {
        let shared = Rc::new(RefCell::new(CompilerSharedState::default()));
        let mut module = Compiler::new(
            "fn hidden() { return 41 }\nexport fn answer() { return hidden() + 1 }",
            "library.to",
            None,
            true,
            "library",
            ModulePermissions::from_capabilities(ModuleCapabilities::none()),
            Rc::clone(&shared),
        )
        .expect("construct imported unit");
        module.compile_unit().expect("contribute module definitions");
        assert_eq!(module.code.capacity(), 0);
        assert_eq!(shared.borrow().functions.len(), 2);
        assert_eq!(shared.borrow().module_defs.len(), 1);
        assert_eq!(shared.borrow().module_defs[0].exported_functions, ["answer"]);
        drop(module);

        let root = Compiler::new(
            "print 42",
            "main.to",
            None,
            false,
            "",
            ModulePermissions::from_capabilities(ModuleCapabilities::all()),
            shared,
        )
        .expect("construct final owner");
        let program = root.compile().expect("finish shared session");
        assert_eq!(program.functions.len(), 2);
        assert_eq!(program.modules.len(), 1);
        crate::BytecodeVerifier::verify(&program).expect("contributed definitions remain valid");
    }

    #[test]
    fn function_body_error_restores_enclosing_compiler_buffers() {
        let mut compiler = compiler("fn broken(value) { let inside = value return");
        assert_eq!(compiler.symbols.define_current("outer"), Some(0));
        compiler.emit(Op::PushInt, 7, 0);
        compiler.emit(Op::Store, 0, 0);
        compiler.function_globals.insert("previous".to_string(), 3);
        let code = compiler.code.as_ptr();
        let names = compiler.symbols.names.as_ptr();

        let error = compiler
            .function_definition(false, Vec::new())
            .expect_err("missing return expression");
        assert!(error.to_string().contains("Expected expression"));
        assert_eq!(compiler.code.as_ptr(), code);
        assert_eq!(compiler.code, [Instr::new(Op::PushInt, 7, 0), Instr::new(Op::Store, 0, 0)]);
        assert_eq!(compiler.symbols.names.as_ptr(), names);
        assert_eq!(compiler.symbols.names, ["outer"]);
        assert_eq!(compiler.symbols.get("outer"), Some(0));
        assert_eq!(compiler.function_globals.get("previous"), Some(&3));
        assert!(!compiler.in_function);
    }

    #[test]
    #[ignore = "diagnostic compiler output storage; run alone with --ignored --nocapture"]
    fn report_compiler_output_storage() {
        for count in [1, 32, 256] {
            let mut source = String::new();
            for index in 0..count {
                writeln!(source, "fn sample_{index}(value) {{ let copy = value return copy + {index} }}")
                    .expect("write declaration fixture");
            }
            source.push_str("print sample_0(1)\n");
            for optimized in [false, true] {
                let verified = if optimized {
                    crate::compile_source_verified(&source)
                } else {
                    crate::compile_source_unoptimized_verified(&source)
                }
                .expect("compile storage fixture");
                let program = verified.program();
                let code_live: usize = program
                    .functions
                    .iter()
                    .map(|function| size_of_val(&*function.code))
                    .sum();
                let code_reserved: usize = program
                    .functions
                    .iter()
                    .map(|function| function.code.capacity() * size_of::<Instr>())
                    .sum();
                let names_reserved: usize = program
                    .functions
                    .iter()
                    .map(|function| function.names.capacity() * size_of::<String>())
                    .sum();
                eprintln!(
                    "functions={count} optimized={optimized}: verified_bytes={} function_code_live={code_live} \
                     function_code_reserved={code_reserved} function_name_headers_reserved={names_reserved}",
                    verified.retained_bytes(),
                );
            }
        }
    }

    #[test]
    fn compiler_retains_incoming_source_without_copying_it() {
        let mut source = String::with_capacity(1024);
        source.push_str("print 42\n");
        let allocation = source.as_ptr();
        let compiler = Compiler::new(
            source,
            "owned.to",
            None,
            false,
            "",
            ModulePermissions::from_capabilities(ModuleCapabilities::all()),
            Rc::new(RefCell::new(CompilerSharedState::default())),
        )
        .expect("lex owned source");
        assert_eq!(compiler.source_map.source().as_ptr(), allocation);
        assert_eq!(compiler.source_map.source(), "print 42\n");
        assert_eq!(compiler.compile().expect("compile owned source").slot_count, 0);
    }

    #[test]
    fn consuming_token_moves_its_text_allocation() {
        let mut compiler = compiler("original_identifier next_identifier");
        let allocation = compiler.current().text.as_ptr();
        let token = compiler.eat(TokenKind::Ident).expect("consume identifier");

        assert_eq!(token.text, "original_identifier");
        assert_eq!(token.text.as_ptr(), allocation);
        assert_eq!((token.pos, token.end), (0, 19));
        assert_eq!(compiler.current().text, "next_identifier");
    }

    #[test]
    fn mismatched_token_preserves_cursor_and_diagnostic() {
        let mut compiler = compiler("# tokens\nalpha");
        let allocation = compiler.current().text.as_ptr();
        let error = compiler
            .eat(TokenKind::Int)
            .expect_err("identifier is not an integer token");

        assert_eq!(error.to_string(), "tokens.to:2:1: Expected INT, got IDENT\nalpha\n^^^^^");
        assert_eq!(compiler.current().text.as_ptr(), allocation);
        assert_eq!(compiler.peek_kind(0), Some(TokenKind::Ident));
        assert_eq!(compiler.peek_kind(1), Some(TokenKind::Eof));
        assert_eq!(compiler.eat(TokenKind::Ident).expect("retry correct kind").text, "alpha");
    }

    #[test]
    fn lookahead_tracks_remaining_tokens_and_end_of_input() {
        let mut compiler = compiler("skip ns.call(1) item.field");
        assert!(!compiler.is_qualified_call());
        compiler.eat(TokenKind::Ident).expect("consume prefix");
        assert!(compiler.is_qualified_call());
        assert_eq!(compiler.peek_kind(2), Some(TokenKind::Ident));

        for kind in [
            TokenKind::Ident,
            TokenKind::Dot,
            TokenKind::Ident,
            TokenKind::LParen,
            TokenKind::Int,
            TokenKind::RParen,
        ] {
            compiler.eat(kind).expect("consume qualified call");
        }
        assert_eq!(compiler.current().text, "item");
        assert!(!compiler.is_qualified_call());
        for kind in [TokenKind::Ident, TokenKind::Dot, TokenKind::Ident, TokenKind::Eof] {
            compiler.eat(kind).expect("consume field and explicit EOF");
        }

        assert_eq!(compiler.current().kind, TokenKind::Eof);
        assert_eq!((compiler.current().pos, compiler.current().end), (26, 26));
        assert_eq!(compiler.peek_kind(0), None);
        assert_eq!(compiler.peek_kind(3), None);
        assert!(!compiler.is_qualified_call());
        let error = compiler.eat(TokenKind::Int).expect_err("integer missing at EOF");
        assert_eq!(
            error.to_string(),
            "tokens.to:1:27: Expected INT, got EOF\nskip ns.call(1) item.field\n                          ^"
        );
        assert_eq!(compiler.eat(TokenKind::Eof).expect("stable EOF sentinel").kind, TokenKind::Eof);
    }

    #[test]
    fn primary_errors_retain_consumed_literal_and_source_span() {
        let mut compiler = compiler("\n999999999999999999999999\n");
        let error = compiler.primary().expect_err("oversized integer literal");
        assert_eq!(
            error.to_string(),
            "tokens.to:2:1: Integer literal \"999999999999999999999999\" is out of range\n999999999999999999999999\n^^^^^^^^^^^^^^^^^^^^^^^^"
        );
        assert_eq!(compiler.current().kind, TokenKind::Eof);
    }

    #[test]
    #[ignore = "diagnostic token storage; run alone with --ignored --nocapture"]
    fn token_stream_storage_diagnostic() {
        let source = "let value = 42\nprint value\n".repeat(256);
        let source_map = SourceMap::new(source, "tokens.to");
        let tokens = Lexer::new(&source_map).tokenize().expect("lex diagnostic");
        let token_count = tokens.len();
        let backing_bytes = tokens.capacity() * size_of::<Token>();
        let text_bytes: usize = tokens.iter().map(|token| token.text.capacity()).sum();
        let allocation = tokens.as_ptr();
        let mut compiler = compiler("");
        compiler.tokens = tokens.into_iter();
        assert_eq!(compiler.tokens.as_slice().as_ptr(), allocation);

        let mut consumed = 0;
        while compiler.current().kind != TokenKind::Eof {
            let kind = compiler.current().kind;
            let allocation = compiler.current().text.as_ptr();
            let token = compiler.eat(kind).expect("consume diagnostic token");
            assert_eq!(token.text.as_ptr(), allocation);
            consumed += 1;
        }
        assert_eq!(consumed + 1, token_count);
        assert_eq!(compiler.tokens.as_slice().len(), 1);
        assert_eq!(compiler.tokens.as_slice()[0].text.capacity(), 0);
        eprintln!(
            "token storage: tokens={token_count}, backing_bytes={backing_bytes}, original_text_bytes={text_bytes}, \
             consumed={consumed}, remaining_text_bytes=0, former_cursor_bytes={}, owning_cursor_bytes={}, token_bytes={}",
            size_of::<Vec<Token>>() + size_of::<usize>(),
            size_of_val(&compiler.tokens),
            size_of::<Token>(),
        );
    }
}
