use crate::{
    Context, JsResult, JsValue,
    vm::opcode::{Operation, RegisterOperand},
};

macro_rules! implement_bin_ops {
    ($name:ident, $op:ident, $kind:ident, $ovf:expr, $doc_string:literal $(, $fast_fn: ident)?) => {
        #[doc= concat!("`", stringify!($name), "` implements the `OpCode` Operation for `Opcode::", stringify!($name), "`\n")]
        #[doc= "\n"]
        #[doc="Operation:\n"]
        #[doc= concat!(" - ", $doc_string)]
        #[derive(Debug, Clone, Copy)]
        pub(crate) struct $name;

        impl $name {
            #[inline]
            pub(crate) fn operation(
                (dst, lhs, rhs): (RegisterOperand, RegisterOperand, RegisterOperand),
                context: &mut Context,
            ) -> JsResult<()> {
                let lhs = context.vm.get_register(lhs.into());
                let rhs = context.vm.get_register(rhs.into());

                // Off-by-default opportunity instrumentation. See
                // `crate::vm::arith_instrument`. Classifies the operand pair
                // before any clone/coercion and records it against this site.
                #[cfg(feature = "arith-instrument")]
                {
                    let class = lhs.classify_arith_pair(rhs, $ovf);
                    let frame = context.vm.frame();
                    // Stable per-CodeBlock identity for the process lifetime.
                    let code_block: usize =
                        std::ptr::addr_of!(*frame.code_block).cast::<()>() as usize;
                    let pc = frame.pc;
                    crate::vm::arith_instrument::record(
                        code_block,
                        pc,
                        crate::vm::arith_instrument::ArithKind::$kind,
                        class,
                    );
                }

                $(
                // Fast path: try numeric operation without cloning.
                if let Some(value) = JsValue::$fast_fn(lhs, rhs) {
                    context.vm.set_register(dst.into(), value.into());
                    return Ok(());
                }
                )?

                // Slow path: clone and use full method with type coercion.
                let lhs = lhs.clone();
                let rhs = rhs.clone();
                let value = lhs.$op(&rhs, context)?;
                context.vm.set_register(dst.into(), value.into());
                Ok(())
            }
        }

        impl Operation for $name {
            const NAME: &'static str = stringify!($name);
            const INSTRUCTION: &'static str = stringify!("INST - " + $name);
            const COST: u8 = 2;
        }
    };
}

// Overflow predicates per op: return true when both-i32 operands would NOT stay
// in i32 (i.e. a specialized int opcode would have to deopt to the generic path).
// Mirrors the exact fast-path overflow conditions in value/operations.rs.

implement_bin_ops!(
    Add,
    add,
    Add,
    |x: i32, y: i32| x.checked_add(y).is_none(),
    "Binary `+` operator.",
    add_fast
);
implement_bin_ops!(
    Sub,
    sub,
    Sub,
    |x: i32, y: i32| x.checked_sub(y).is_none(),
    "Binary `-` operator.",
    sub_fast
);
implement_bin_ops!(
    Mul,
    mul,
    Mul,
    |x: i32, y: i32| x
        .checked_mul(y)
        .as_ref()
        .is_none_or(|v| !(*v != 0 || i32::min(x, y) >= 0)),
    "Binary `*` operator.",
    mul_fast
);
implement_bin_ops!(
    Div,
    div,
    Div,
    |x: i32, y: i32| x.checked_div(y).as_ref().is_none_or(|div| y * div != x),
    "Binary `/` operator.",
    div_fast
);
implement_bin_ops!(
    Pow,
    pow,
    Pow,
    |x: i32, y: i32| u32::try_from(y)
        .ok()
        .and_then(|y| x.checked_pow(y))
        .is_none(),
    "Binary `**` operator.",
    pow_fast
);
implement_bin_ops!(
    Mod,
    rem,
    Mod,
    |x: i32, y: i32| y == 0 || (x % y == 0 && x < 0),
    "Binary `%` operator.",
    rem_fast
);
implement_bin_ops!(
    BitAnd,
    bitand,
    BitAnd,
    |_x: i32, _y: i32| false,
    "Binary `&` operator.",
    bitand_fast
);
implement_bin_ops!(
    BitOr,
    bitor,
    BitOr,
    |_x: i32, _y: i32| false,
    "Binary `|` operator.",
    bitor_fast
);
implement_bin_ops!(
    BitXor,
    bitxor,
    BitXor,
    |_x: i32, _y: i32| false,
    "Binary `^` operator.",
    bitxor_fast
);
implement_bin_ops!(
    ShiftLeft,
    shl,
    Shl,
    |_x: i32, _y: i32| false,
    "Binary `<<` operator.",
    shl_fast
);
implement_bin_ops!(
    ShiftRight,
    shr,
    Shr,
    |_x: i32, _y: i32| false,
    "Binary `>>` operator.",
    shr_fast
);
implement_bin_ops!(
    UnsignedShiftRight,
    ushr,
    Ushr,
    |_x: i32, _y: i32| false,
    "Binary `>>>` operator.",
    ushr_fast
);
implement_bin_ops!(
    Eq,
    equals,
    Eq,
    |_x: i32, _y: i32| false,
    "Binary `==` operator.",
    equals_fast
);
implement_bin_ops!(
    NotEq,
    not_equals,
    NotEq,
    |_x: i32, _y: i32| false,
    "Binary `!=` operator.",
    not_equals_fast
);
implement_bin_ops!(
    GreaterThan,
    gt,
    Gt,
    |_x: i32, _y: i32| false,
    "Binary `>` operator.",
    gt_fast
);
implement_bin_ops!(
    GreaterThanOrEq,
    ge,
    Ge,
    |_x: i32, _y: i32| false,
    "Binary `>=` operator.",
    ge_fast
);
implement_bin_ops!(
    LessThan,
    lt,
    Lt,
    |_x: i32, _y: i32| false,
    "Binary `<` operator.",
    lt_fast
);
implement_bin_ops!(
    LessThanOrEq,
    le,
    Le,
    |_x: i32, _y: i32| false,
    "Binary `<=` operator.",
    le_fast
);

/// `InstanceOf` implements the `OpCode` Operation for `Opcode::InstanceOf`.
///
/// Operation:
///  - Binary `instanceof` operator.
///
/// Not arithmetic, so it is intentionally left out of the arith-opportunity
/// instrumentation (its operands are always objects and would only add noise).
#[derive(Debug, Clone, Copy)]
pub(crate) struct InstanceOf;

impl InstanceOf {
    #[inline]
    pub(crate) fn operation(
        (dst, lhs, rhs): (RegisterOperand, RegisterOperand, RegisterOperand),
        context: &mut Context,
    ) -> JsResult<()> {
        let lhs = context.vm.get_register(lhs.into()).clone();
        let rhs = context.vm.get_register(rhs.into()).clone();
        let value = lhs.instance_of(&rhs, context)?;
        context.vm.set_register(dst.into(), value.into());
        Ok(())
    }
}

impl Operation for InstanceOf {
    const NAME: &'static str = "InstanceOf";
    const INSTRUCTION: &'static str = stringify!("INST - " + InstanceOf);
    const COST: u8 = 2;
}
