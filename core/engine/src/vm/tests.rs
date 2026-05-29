use crate::error::RuntimeLimitError;
use crate::vm::CallFrame;
use crate::vm::call_frame::CallFrameLocation;
use crate::vm::source_info::SourcePath;
use crate::{
    Context, JsNativeErrorKind, JsValue, NativeFunction, TestAction, js_string,
    property::Attribute, run_test_actions, run_test_actions_with,
};
use boa_ast::Position;
use boa_macros::js_str;
use boa_parser::Source;
use indoc::indoc;

#[test]
fn typeof_string() {
    run_test_actions([TestAction::assert_eq(
        indoc! {r#"
            const a = "hello";
            typeof a;
        "#},
        js_str!("string"),
    )]);
}

#[test]
fn typeof_number() {
    run_test_actions([TestAction::assert_eq(
        indoc! {r#"
            let a = 1234;
            typeof a;
        "#},
        js_str!("number"),
    )]);
}

#[test]
fn basic_op() {
    run_test_actions([TestAction::assert_eq(
        indoc! {r#"
            const a = 1;
            const b = 2;
            a + b
        "#},
        3,
    )]);
}

#[test]
fn position() {
    let context = &mut Context::default();
    context
        .register_global_callable(
            js_string!("check_stack"),
            2,
            NativeFunction::from_copy_closure(|_, _, context| {
                let frame = context.stack_trace().collect::<Vec<&CallFrame>>();

                assert_eq!(frame.len(), 4);
                assert_eq!(
                    frame[0].position(),
                    CallFrameLocation {
                        function_name: js_string!("myOtherFunction"),
                        path: SourcePath::None,
                        position: Some(Position::new(2, 16))
                    }
                );
                assert_eq!(
                    frame[1].position(),
                    CallFrameLocation {
                        function_name: js_string!("<eval>"),
                        path: SourcePath::Eval,
                        position: Some(Position::new(1, 16))
                    }
                );
                assert_eq!(
                    frame[2].position(),
                    CallFrameLocation {
                        function_name: js_string!("myFunction"),
                        path: SourcePath::None,
                        position: Some(Position::new(5, 9))
                    }
                );
                assert_eq!(
                    frame[3].position(),
                    CallFrameLocation {
                        function_name: js_string!("<main>"),
                        path: SourcePath::None,
                        position: Some(Position::new(8, 11))
                    }
                );
                Ok(JsValue::undefined())
            }),
        )
        .expect("Could not register function");
    run_test_actions_with(
        [TestAction::run(indoc! {r#"
            const myOtherFunction = () => {
                check_stack();
            };
            function myFunction() {
                eval("myOtherFunction()");
            }

            myFunction();
        "#})],
        context,
    );
}

#[test]
fn try_catch_finally_from_init() {
    // the initialisation of the array here emits a PopOnReturnAdd op
    //
    // here we test that the stack is not popped more than intended due to multiple catches in the
    // same function, which could lead to VM stack corruption
    run_test_actions([TestAction::assert_opaque_error(
        indoc! {r#"
            try {
                [(() => {throw "h";})()];
            } catch (x) {
                throw "h";
            } finally {
            }
        "#},
        js_str!("h"),
    )]);
}

#[test]
fn multiple_catches() {
    // see explanation on `try_catch_finally_from_init`
    run_test_actions([TestAction::assert_eq(
        indoc! {r#"
            try {
                try {
                    [(() => {throw "h";})()];
                } catch (x) {
                    throw "h";
                }
            } catch (y) {
            }
        "#},
        JsValue::undefined(),
    )]);
}

#[test]
fn use_last_expr_try_block() {
    run_test_actions([TestAction::assert_eq(
        indoc! {r#"
            try {
                19;
                7.5;
                "Hello!";
            } catch (y) {
                14;
                "Bye!"
            }
        "#},
        js_str!("Hello!"),
    )]);
}

#[test]
fn use_last_expr_catch_block() {
    run_test_actions([TestAction::assert_eq(
        indoc! {r#"
            try {
                throw Error("generic error");
                19;
                7.5;
            } catch (y) {
                14;
                "Hello!";
            }
        "#},
        js_str!("Hello!"),
    )]);
}

#[test]
fn no_use_last_expr_finally_block() {
    run_test_actions([TestAction::assert_eq(
        indoc! {r#"
            try {
            } catch (y) {
            } finally {
                "Unused";
            }
        "#},
        JsValue::undefined(),
    )]);
}

#[test]
fn finally_block_binding_env() {
    run_test_actions([TestAction::assert_eq(
        indoc! {r#"
            let buf = "Hey hey";
            try {
            } catch (y) {
            } finally {
                let x = " people";
                buf += x;
            }
            buf
        "#},
        js_str!("Hey hey people"),
    )]);
}

#[test]
fn run_super_method_in_object() {
    run_test_actions([TestAction::assert_eq(
        indoc! {r#"
            let proto = {
                m() { return "super"; }
            };
            let obj = {
                v() { return super.m(); }
            };
            Object.setPrototypeOf(obj, proto);
            obj.v();
        "#},
        js_str!("super"),
    )]);
}

#[test]
fn get_reference_by_super() {
    run_test_actions([TestAction::assert_eq(
        indoc! {r#"
            var fromA, fromB;
            var A = { fromA: 'a', fromB: 'a' };
            var B = { fromB: 'b' };
            Object.setPrototypeOf(B, A);
            var obj = {
                fromA: 'c',
                fromB: 'c',
                method() {
                    fromA = (() => { return super.fromA; })();
                    fromB = (() => { return super.fromB; })();
                }
            };
            Object.setPrototypeOf(obj, B);
            obj.method();
            fromA + fromB
        "#},
        js_str!("ab"),
    )]);
}

#[test]
fn super_call_constructor_null() {
    run_test_actions([TestAction::assert_native_error(
        indoc! {r#"
            class A extends Object {
                constructor() {
                    Object.setPrototypeOf(A, null);
                    super(A);
                }
            }
            new A();
        "#},
        JsNativeErrorKind::Type,
        "super constructor object must be constructor",
    )]);
}

#[test]
fn super_call_get_constructor_before_arguments_execution() {
    run_test_actions([TestAction::assert(indoc! {r#"
        class A extends Object {
            constructor() {
                super(Object.setPrototypeOf(A, null));
            }
        }
        new A() instanceof A;
    "#})]);
}

#[test]
fn order_of_execution_in_assignment() {
    run_test_actions([
        TestAction::run(indoc! {r#"
                let i = 0;
                let array = [[]];

                array[i++][i++] = i++;
            "#}),
        TestAction::assert_eq("i", 3),
        TestAction::assert_eq("array.length", 1),
        TestAction::assert_eq("array[0].length", 2),
    ]);
}

#[test]
fn order_of_execution_in_assignment_with_comma_expressions() {
    run_test_actions([TestAction::assert_eq(
        indoc! {r#"
            let result = "";
            function f(i) {
                result += i;
            }
            let a = [[]];
            (f(1), a)[(f(2), 0)][(f(3), 0)] = (f(4), 123);
            result
        "#},
        js_str!("1234"),
    )]);
}

#[test]
fn loop_runtime_limit() {
    run_test_actions([
        TestAction::assert_eq(
            indoc! {r#"
                for (let i = 0; i < 20; ++i) { }
            "#},
            JsValue::undefined(),
        ),
        TestAction::inspect_context(|context| {
            context.runtime_limits_mut().set_loop_iteration_limit(10);
        }),
        TestAction::assert_runtime_limit_error(
            indoc! {r#"
                for (let i = 0; i < 20; ++i) { }
            "#},
            RuntimeLimitError::LoopIteration,
        ),
        TestAction::assert_eq(
            indoc! {r#"
                for (let i = 0; i < 10; ++i) { }
            "#},
            JsValue::undefined(),
        ),
        TestAction::assert_runtime_limit_error(
            indoc! {r#"
                while (1) { }
            "#},
            RuntimeLimitError::LoopIteration,
        ),
    ]);
}

#[test]
fn recursion_runtime_limit() {
    run_test_actions([
        TestAction::run(indoc! {r#"
            function factorial(n) {
                if (n == 0) {
                    return 1;
                }

                return n * factorial(n - 1);
            }
        "#}),
        TestAction::assert_eq("factorial(8)", JsValue::new(40_320)),
        TestAction::assert_eq("factorial(11)", JsValue::new(39_916_800)),
        TestAction::inspect_context(|context| {
            context.runtime_limits_mut().set_recursion_limit(10);
        }),
        TestAction::assert_runtime_limit_error("factorial(11)", RuntimeLimitError::Recursion),
        TestAction::assert_eq("factorial(8)", JsValue::new(40_320)),
        TestAction::assert_runtime_limit_error(
            indoc! {r#"
                function x() {
                    x()
                }

                x()
            "#},
            RuntimeLimitError::Recursion,
        ),
    ]);
}

#[test]
fn arguments_object_constructor_valid_index() {
    run_test_actions([TestAction::assert_eq(
        indoc! {r#"
            let args;
            function F(a = 1) {
                args = arguments;
            }
            new F();
            typeof args
        "#},
        js_str!("object"),
    )]);
}

#[test]
fn empty_return_values() {
    run_test_actions([
        TestAction::run(indoc! {r#"do {{}} while (false);"#}),
        TestAction::run(indoc! {r#"do try {{}} catch {} while (false);"#}),
        TestAction::run(indoc! {r#"do {} while (false);"#}),
        TestAction::run(indoc! {r#"do try {{}{}} catch {} while (false);"#}),
        TestAction::run(indoc! {r#"do {{}{}} while (false);"#}),
        TestAction::run(indoc! {r#"do {;{}} while (false);"#}),
        TestAction::run(indoc! {r#"do {e: {}} while (false);"#}),
        TestAction::run(indoc! {r#"do {e: ;} while (false);"#}),
        TestAction::run(indoc! {r#"do { break } while (false);"#}),
        TestAction::run(indoc! {r#"while (true) a: break"#}),
        TestAction::run(indoc! {r#"while (true) a: {"a"; break};"#}),
        TestAction::run(indoc! {r#"do {"a";{}} while (false);"#}),
        TestAction::run(indoc! {r#"
            switch (false) {
                default: {}
            }
        "#}),
        TestAction::run(indoc! {r#"
            switch (false) {
                default: {}{}
            }
        "#}),
        TestAction::run(indoc! {r#"
            switch (false) {
                default: ;{}{}
            }
        "#}),
    ]);
}

#[test]
fn truncate_environments_on_non_caught_native_error() {
    let source = "with (new Proxy({}, {has: p => false})) {a}";
    run_test_actions([
        TestAction::assert_native_error(source, JsNativeErrorKind::Reference, "a is not defined"),
        TestAction::assert_native_error(source, JsNativeErrorKind::Reference, "a is not defined"),
    ]);
}

#[test]
fn super_construction_with_parameter_expression() {
    run_test_actions([
        TestAction::run(indoc! {r#"
            class Person {
                constructor(name) {
                    this.name = name;
                }
            }

            class Student extends Person {
                constructor(name = 'unknown') {
                    super(name);
                }
            }
        "#}),
        TestAction::assert_eq("new Student().name", js_str!("unknown")),
        TestAction::assert_eq("new Student('Jack').name", js_str!("Jack")),
    ]);
}

#[test]
fn cross_context_function_call() {
    let context1 = &mut Context::default();
    let result = context1.eval(Source::from_bytes(indoc! {r"
        var global = 100;

        (function x() {
            return global;
        })
    "}));

    assert!(result.is_ok());
    let result = result.unwrap();
    assert!(result.is_callable());

    let context2 = &mut Context::default();

    context2
        .register_global_property(js_string!("func"), result, Attribute::all())
        .unwrap();

    let result = context2.eval(Source::from_bytes("func()"));

    assert_eq!(result, Ok(JsValue::new(100)));
}

// See: https://github.com/boa-dev/boa/issues/1848
#[test]
fn long_object_chain_gc_trace_stack_overflow() {
    run_test_actions([
        TestAction::run(indoc! {r#"
            let old = {};
            for (let i = 0; i < 100000; i++) {
                old = { old };
            }
        "#}),
        TestAction::inspect_context(|_| boa_gc::force_collect()),
    ]);
}

// See: https://github.com/boa-dev/boa/issues/4515
#[test]
fn recursion_in_async_gen_throws_uncatchable_error() {
    run_test_actions([
        TestAction::inspect_context(|context| {
            context.runtime_limits_mut().set_recursion_limit(128);
        }),
        TestAction::assert_runtime_limit_error(
            indoc! {r#"
                async function* f() {}
                f().return({
                  get then() {
                    this.then;
                  },
                });
            "#},
            RuntimeLimitError::Recursion,
        ),
    ]);
}

#[test]
fn recursion_in_setter_throws_uncatchable_error() {
    run_test_actions([
        TestAction::inspect_context(|context| {
            context.runtime_limits_mut().set_recursion_limit(128);
        }),
        TestAction::assert_runtime_limit_error(
            indoc! {r#"
                const obj = {
                  set x(value) {
                    this.x = value;
                  },
                };
                obj.x = 1;
            "#},
            RuntimeLimitError::Recursion,
        ),
    ]);
}

// ============================================================================
// Adaptive quickening tests
//
// These tests exercise the PEP-659-style opcode quickening for Add/Sub/Mul.
// The threshold is QUICKEN_THRESHOLD (8 executions), so loops of ≥9 iterations
// trigger specialization.  Deopt tests verify that the specialized path
// correctly falls back to full JS semantics when operands change type.
// ============================================================================

/// After the quickening threshold is reached with integer operands the site
/// specializes to `AddInt`.  All results must still be exactly correct.
#[test]
fn quicken_add_int_basic() {
    run_test_actions([TestAction::assert_eq(
        indoc! {r#"
            let sum = 0;
            // 20 iterations — well past the threshold of 8.
            for (let i = 0; i < 20; i++) { sum = sum + i; }
            sum
        "#},
        JsValue::new(190),
    )]);
}

/// Float-dominated loop: specializes to `AddF64` and keeps f64 precision.
#[test]
fn quicken_add_f64_basic() {
    run_test_actions([TestAction::assert_eq(
        indoc! {r#"
            let x = 0.0;
            for (let i = 0; i < 20; i++) { x = x + 0.1; }
            // Round to avoid floating-point noise in the assertion.
            Math.round(x * 10)
        "#},
        JsValue::new(20),
    )]);
}

/// Sub quickened to `SubInt`.
#[test]
fn quicken_sub_int_basic() {
    run_test_actions([TestAction::assert_eq(
        indoc! {r#"
            let x = 100;
            for (let i = 0; i < 20; i++) { x = x - 1; }
            x
        "#},
        JsValue::new(80),
    )]);
}

/// Mul quickened to `MulInt`.
#[test]
fn quicken_mul_int_basic() {
    run_test_actions([TestAction::assert_eq(
        indoc! {r#"
            let x = 1;
            // Multiply by 1 so we don't overflow, 20 iterations.
            for (let i = 0; i < 20; i++) { x = x * 1; }
            x
        "#},
        JsValue::new(1),
    )]);
}

/// Deopt: site quickens to `AddInt` then receives a float — must produce the
/// same result as the non-quickened path (no wrong answer allowed).
#[test]
fn quicken_add_int_then_float_deopt() {
    run_test_actions([TestAction::assert_eq(
        indoc! {r#"
            function f(a, b) { return a + b; }
            // Warm up with integers to trigger quickening.
            for (let i = 0; i < 10; i++) { f(i, 1); }
            // Now pass a float — must deopt and produce correct answer.
            f(3, 0.5)
        "#},
        JsValue::new(3.5_f64),
    )]);
}

/// Deopt: site quickens to `AddInt` then receives a string — must produce the
/// concatenated string (full JS coercion path).
#[test]
fn quicken_add_int_then_string_deopt() {
    run_test_actions([TestAction::assert_eq(
        indoc! {r#"
            function f(a, b) { return a + b; }
            for (let i = 0; i < 10; i++) { f(i, 1); }
            f(1, "x")
        "#},
        js_string!("1x"),
    )]);
}

/// Deopt: site quickens to `AddF64` then receives a string — must produce
/// concatenation via the generic path.
#[test]
fn quicken_add_f64_then_string_deopt() {
    run_test_actions([TestAction::assert_eq(
        indoc! {r#"
            function f(a, b) { return a + b; }
            for (let i = 0; i < 10; i++) { f(1.5, 0.5); }
            f(1.5, "x")
        "#},
        js_string!("1.5x"),
    )]);
}

/// Overflow: i32 addition that overflows must produce the correct f64 result.
/// This exercises the overflow deopt path in `AddInt`.
#[test]
fn quicken_add_int_overflow() {
    run_test_actions([TestAction::assert_eq(
        indoc! {r#"
            function f(a, b) { return a + b; }
            for (let i = 0; i < 10; i++) { f(1, 1); }
            // 2^31 - 1 + 1 overflows i32, must produce 2^31 as f64.
            f(2147483647, 1)
        "#},
        JsValue::new(2_147_483_648.0_f64),
    )]);
}

/// Mixed operands through the same site: int then float then string — the site
/// must handle all three without producing a wrong result.
#[test]
fn quicken_add_mixed_types() {
    run_test_actions([
        TestAction::assert_eq(
            indoc! {r#"
                function f(a, b) { return a + b; }
                // Force quickening to AddInt.
                let last = 0;
                for (let i = 0; i < 10; i++) { last = f(i, 1); }
                last  // 10
            "#},
            JsValue::new(10),
        ),
        TestAction::assert_eq(
            indoc! {r#"
                function f(a, b) { return a + b; }
                for (let i = 0; i < 10; i++) { f(i, 1); }
                // Now drive through float — deopt.
                let r1 = f(1.5, 2.5);  // 4.0
                // Then back to int — generic handles it (counter reset).
                let r2 = f(3, 4);       // 7
                // Then string.
                let r3 = f("a", "b");   // "ab"
                r1 + "-" + r2 + "-" + r3
            "#},
            js_string!("4-7-ab"),
        ),
    ]);
}

/// `-0` semantics for `MulInt`: `0 * -1 === -0` in JS.
/// The quickened handler must not produce `0` (integer) in this case.
#[test]
fn quicken_mul_negative_zero() {
    run_test_actions([
        // `Object.is(-0, -0)` is true; `-0 === 0` is also true.
        // Distinguish via `1 / (-0)` which produces `-Infinity`.
        TestAction::assert_eq(
            indoc! {r#"
                function f(a, b) { return a * b; }
                for (let i = 1; i < 10; i++) { f(i, i); }
                1 / f(0, -1)   // must be -Infinity
            "#},
            JsValue::new(f64::NEG_INFINITY),
        ),
    ]);
}

/// Sub with integer overflow produces the correct f64 result.
#[test]
fn quicken_sub_int_overflow() {
    run_test_actions([TestAction::assert_eq(
        indoc! {r#"
            function f(a, b) { return a - b; }
            for (let i = 0; i < 10; i++) { f(10, i); }
            f(-2147483648, 1)  // MIN_INT - 1 overflows, must be -2147483649.0
        "#},
        JsValue::new(-2_147_483_649.0_f64),
    )]);
}

/// Verify that quickening state is per-function, not global: two functions
/// with the same `+` expression in the same script are independent sites.
#[test]
fn quicken_independent_sites() {
    run_test_actions([TestAction::assert_eq(
        indoc! {r#"
            function addInt(a, b) { return a + b; }
            function addStr(a, b) { return a + b; }
            // Warm up addInt with integers.
            for (let i = 0; i < 10; i++) { addInt(i, 1); }
            // Warm up addStr with strings.
            for (let i = 0; i < 10; i++) { addStr("x", "y"); }
            // Each site should operate correctly with its trained type.
            addInt(5, 3) + addStr("a", "b").length
        "#},
        JsValue::new(10), // 8 + 2
    )]);
}
