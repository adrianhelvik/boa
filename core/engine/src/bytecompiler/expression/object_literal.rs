use crate::bytecompiler::{ByteCompiler, FunctionSpec, MethodKind, Register};
use boa_ast::{
    Expression,
    expression::literal::{ObjectLiteral, PropertyDefinition},
    property::{MethodDefinitionKind, PropertyName},
};
use boa_interner::Sym;
use thin_vec::ThinVec;

impl ByteCompiler<'_> {
    pub(crate) fn compile_object_literal(&mut self, literal: &ObjectLiteral, dst: &Register) {
        self.bytecode.emit_store_empty_object(dst.variable());

        for property in literal.properties() {
            match property {
                PropertyDefinition::IdentifierReference(ident) => {
                    let index = self.get_or_insert_name(ident.sym());
                    let dst_op = dst.variable();
                    let idx_op = index.into();
                    // `{ x }` shorthand — `x` is an Identifier expression.
                    // `compile_expr_operand` resolves it to the binding's
                    // register without a temporary Move when the binding is
                    // local or const-cached.
                    self.compile_expr_operand(&Expression::Identifier(*ident), |c, value_op| {
                        c.bytecode
                            .emit_define_own_property_by_name(dst_op, value_op, idx_op);
                    });
                }
                PropertyDefinition::Property(name, expr) => match name {
                    PropertyName::Literal(name) => {
                        let dst_op = dst.variable();
                        if *name == Sym::__PROTO__ && !self.json_parse {
                            let value = self.register_allocator.alloc();
                            self.compile_expr(expr, &value);
                            self.bytecode.emit_set_prototype(dst_op, value.variable());
                            self.register_allocator.dealloc(value);
                        } else {
                            // Fast path: if `expr` is an Identifier with a
                            // cached/local register, hand it to the closure
                            // directly — `DefineOwnPropertyByName` accepts an
                            // arbitrary register operand for `value`.
                            let index = self.get_or_insert_name(name.sym());
                            let idx_op = index.into();
                            self.compile_expr_operand(expr, |c, value_op| {
                                c.bytecode
                                    .emit_define_own_property_by_name(dst_op, value_op, idx_op);
                            });
                        }
                    }
                    PropertyName::Computed(name_node) => {
                        let key = self.register_allocator.alloc();
                        self.compile_expr(name_node, &key);
                        self.bytecode
                            .emit_to_property_key(key.variable(), key.variable());
                        let function = self.register_allocator.alloc();
                        self.compile_expr(expr, &function);
                        if expr.is_anonymous_function_definition() {
                            self.bytecode.emit_set_function_name(
                                function.variable(),
                                key.variable(),
                                0u32.into(),
                            );
                        }
                        self.bytecode.emit_define_own_property_by_value(
                            function.variable(),
                            key.variable(),
                            dst.variable(),
                        );
                        self.register_allocator.dealloc(key);
                        self.register_allocator.dealloc(function);
                    }
                },
                PropertyDefinition::MethodDefinition(m) => {
                    let kind = match m.kind() {
                        MethodDefinitionKind::Get => MethodKind::Get,
                        MethodDefinitionKind::Set => MethodKind::Set,
                        _ => MethodKind::Ordinary,
                    };
                    match m.name() {
                        PropertyName::Literal(name) => {
                            let method = self.object_method(m.into(), kind);
                            self.bytecode
                                .emit_set_home_object(method.variable(), dst.variable());
                            let index = self.get_or_insert_name(name.sym());
                            match kind {
                                MethodKind::Get => self.bytecode.emit_set_property_getter_by_name(
                                    dst.variable(),
                                    method.variable(),
                                    index.into(),
                                ),
                                MethodKind::Set => self.bytecode.emit_set_property_setter_by_name(
                                    dst.variable(),
                                    method.variable(),
                                    index.into(),
                                ),
                                MethodKind::Ordinary => {
                                    self.bytecode.emit_define_own_property_by_name(
                                        dst.variable(),
                                        method.variable(),
                                        index.into(),
                                    );
                                }
                            }
                            self.register_allocator.dealloc(method);
                        }
                        PropertyName::Computed(name_node) => {
                            self.compile_object_literal_computed_method(
                                name_node,
                                m.into(),
                                kind,
                                dst,
                            );
                        }
                    }
                }
                PropertyDefinition::SpreadObject(expr) => {
                    let source = self.register_allocator.alloc();
                    self.compile_expr(expr, &source);
                    self.bytecode.emit_copy_data_properties(
                        dst.variable(),
                        source.variable(),
                        ThinVec::new(),
                    );
                    self.register_allocator.dealloc(source);
                }
                PropertyDefinition::CoverInitializedName(_, _) => {
                    unreachable!("invalid assignment pattern in object literal")
                }
            }
        }
    }

    fn compile_object_literal_computed_method(
        &mut self,
        expr: &Expression,
        function: FunctionSpec<'_>,
        kind: MethodKind,
        object: &Register,
    ) {
        let key = self.register_allocator.alloc();
        self.compile_expr(expr, &key);

        self.bytecode
            .emit_to_property_key(key.variable(), key.variable());

        let method = self.object_method(function, kind);
        let value: u32 = match kind {
            MethodKind::Get => 1,
            MethodKind::Set => 2,
            MethodKind::Ordinary => 0,
        };

        self.bytecode
            .emit_set_function_name(method.variable(), key.variable(), value.into());
        self.bytecode
            .emit_set_home_object(method.variable(), object.variable());

        match kind {
            MethodKind::Get => self.bytecode.emit_set_property_getter_by_value(
                method.variable(),
                key.variable(),
                object.variable(),
            ),
            MethodKind::Set => self.bytecode.emit_set_property_setter_by_value(
                method.variable(),
                key.variable(),
                object.variable(),
            ),
            MethodKind::Ordinary => self.bytecode.emit_define_own_property_by_value(
                method.variable(),
                key.variable(),
                object.variable(),
            ),
        }

        self.register_allocator.dealloc(key);
        self.register_allocator.dealloc(method);
    }
}
