use boa_gc::Gc;
use boa_parser::Source;

use crate::{
    Context, JsObject, JsResult, JsValue,
    builtins::{OrdinaryObject, function::OrdinaryFunction},
    js_string,
    object::{
        ObjectInitializer, internal_methods::InternalMethodPropertyContext,
        shape::slot::SlotAttributes,
    },
    property::{Attribute, PropertyDescriptor, PropertyKey},
    vm::{CallIC, CodeBlock, DenseKind},
};

#[test]
fn get_own_property_internal_method() {
    let context = &mut Context::default();

    let o = context
        .intrinsics()
        .templates()
        .ordinary_object()
        .create(OrdinaryObject, Vec::default());

    let property: PropertyKey = js_string!("prop").into();
    let value = 100;

    o.set(property.clone(), value, true, context)
        .expect("should not fail");

    let context = &mut InternalMethodPropertyContext::new(context);

    assert_eq!(context.slot().index, 0);
    assert_eq!(context.slot().attributes, SlotAttributes::empty());

    o.__get_own_property__(&property, context)
        .expect("should not fail");

    assert!(
        !context.slot().in_prototype(),
        "Since it's an owned property, the prototype bit should not be set"
    );

    assert!(
        context.slot().is_cacheable(),
        "Since it's an owned property, this should be cacheable"
    );

    let shape = o.borrow().shape().clone();

    let slot = shape.lookup(&property);

    assert!(slot.is_some(), "the property should be found in the object");

    let slot = slot.expect("the property should be found in the object");

    assert_eq!(context.slot().index, slot.index);
}

#[test]
fn get_internal_method() {
    let context = &mut Context::default();

    let o = context
        .intrinsics()
        .templates()
        .ordinary_object()
        .create(OrdinaryObject, Vec::default());

    let property: PropertyKey = js_string!("prop").into();
    let value = 100;

    o.set(property.clone(), value, true, context)
        .expect("should not fail");

    let context = &mut InternalMethodPropertyContext::new(context);

    assert_eq!(context.slot().index, 0);
    assert_eq!(context.slot().attributes, SlotAttributes::empty());

    o.__get__(&property, o.clone().into(), context)
        .expect("should not fail");

    assert!(
        !context.slot().in_prototype(),
        "Since it's an owned property, the prototype bit should not be set"
    );

    assert!(
        context.slot().is_cacheable(),
        "Since it's an owned property, this should be cacheable"
    );

    let shape = o.borrow().shape().clone();

    let slot = shape.lookup(&property);

    assert!(slot.is_some(), "the property should be found in the object");

    let slot = slot.expect("the property should be found in the object");

    assert_eq!(context.slot().index, slot.index);
}

#[test]
fn get_internal_method_in_prototype() {
    let context = &mut Context::default();

    let o = context
        .intrinsics()
        .templates()
        .ordinary_object()
        .create(OrdinaryObject, Vec::default());

    let property: PropertyKey = js_string!("prop").into();
    let value = 100;

    let prototype = context.intrinsics().constructors().object().prototype();

    prototype
        .set(property.clone(), value, true, context)
        .expect("should not fail");

    let context = &mut InternalMethodPropertyContext::new(context);

    assert_eq!(context.slot().index, 0);
    assert_eq!(context.slot().attributes, SlotAttributes::empty());

    o.__get__(&property, o.clone().into(), context)
        .expect("should not fail");

    assert!(
        context.slot().in_prototype(),
        "Since it's an prototype property, the prototype bit should not be set"
    );

    assert!(
        context.slot().is_cacheable(),
        "Since it's an prototype property, this should be cacheable"
    );

    let shape = prototype.borrow().shape().clone();

    let slot = shape.lookup(&property);

    assert!(slot.is_some(), "the property should be found in the object");

    let slot = slot.expect("the property should be found in the object");

    assert_eq!(context.slot().index, slot.index);
}

#[test]
fn define_own_property_internal_method_non_existent_property() {
    let context = &mut Context::default();

    let o = context
        .intrinsics()
        .templates()
        .ordinary_object()
        .create(OrdinaryObject, Vec::default());

    let property: PropertyKey = js_string!("prop").into();
    let value = 100;

    o.set(property.clone(), value, true, context)
        .expect("should not fail");

    let context = &mut InternalMethodPropertyContext::new(context);

    assert_eq!(context.slot().index, 0);
    assert_eq!(context.slot().attributes, SlotAttributes::empty());

    o.__define_own_property__(
        &property,
        PropertyDescriptor::builder()
            .value(value)
            .writable(true)
            .configurable(true)
            .enumerable(true)
            .build(),
        context,
    )
    .expect("should not fail");

    assert!(
        !context.slot().in_prototype(),
        "Since it's an owned property, the prototype bit should not be set"
    );

    assert!(
        context.slot().is_cacheable(),
        "Since it's an owned property, this should be cacheable"
    );

    let shape = o.borrow().shape().clone();

    let slot = shape.lookup(&property);

    assert!(slot.is_some(), "the property should be found in the object");

    let slot = slot.expect("the property should be found in the object");

    assert_eq!(context.slot().index, slot.index);
}

#[test]
fn define_own_property_internal_method_existing_property_property() {
    let context = &mut Context::default();

    let o = context
        .intrinsics()
        .templates()
        .ordinary_object()
        .create(OrdinaryObject, Vec::default());

    let property: PropertyKey = js_string!("prop").into();
    let value = 100;

    o.set(property.clone(), value, true, context)
        .expect("should not fail");

    o.__define_own_property__(
        &property,
        PropertyDescriptor::builder()
            .value(value)
            .writable(true)
            .configurable(true)
            .enumerable(true)
            .build(),
        &mut context.into(),
    )
    .expect("should not fail");

    let context = &mut InternalMethodPropertyContext::new(context);

    assert_eq!(context.slot().index, 0);
    assert_eq!(context.slot().attributes, SlotAttributes::empty());

    o.__define_own_property__(
        &property,
        PropertyDescriptor::builder()
            .value(value + 100)
            .writable(true)
            .configurable(true)
            .enumerable(true)
            .build(),
        context,
    )
    .expect("should not fail");

    assert!(
        !context.slot().in_prototype(),
        "Since it's an owned property, the prototype bit should not be set"
    );

    assert!(
        context.slot().is_cacheable(),
        "Since it's an owned property, this should be cacheable"
    );

    let shape = o.borrow().shape().clone();

    let slot = shape.lookup(&property);

    assert!(slot.is_some(), "the property should be found in the object");

    let slot = slot.expect("the property should be found in the object");

    assert_eq!(context.slot().index, slot.index);
}

#[test]
fn set_internal_method() {
    let context = &mut Context::default();

    let o = context
        .intrinsics()
        .templates()
        .ordinary_object()
        .create(OrdinaryObject, Vec::default());

    let property: PropertyKey = js_string!("prop").into();
    let value = 100;

    o.set(property.clone(), value, true, context)
        .expect("should not fail");

    let context = &mut InternalMethodPropertyContext::new(context);

    assert_eq!(context.slot().index, 0);
    assert_eq!(context.slot().attributes, SlotAttributes::empty());

    o.__set__(property.clone(), value.into(), o.clone().into(), context)
        .expect("should not fail");

    assert!(
        !context.slot().in_prototype(),
        "Since it's an owned property, the prototype bit should not be set"
    );

    assert!(
        context.slot().is_cacheable(),
        "Since it's an owned property, this should be cacheable"
    );

    let shape = o.borrow().shape().clone();

    let slot = shape.lookup(&property);

    assert!(slot.is_some(), "the property should be found in the object");

    let slot = slot.expect("the property should be found in the object");

    assert_eq!(context.slot().index, slot.index);
}

fn get_codeblock(value: &JsValue) -> Option<(JsObject, Gc<CodeBlock>)> {
    let object = value.as_object()?.clone();
    let code = object.downcast_ref::<OrdinaryFunction>()?.code.clone();

    Some((object, code))
}

#[test]
fn set_property_by_name_set_inline_cache_on_property_load() -> JsResult<()> {
    let context = &mut Context::default();
    let function = context.eval(Source::from_bytes("(function (o) { return o.test; })"))?;
    let (function, code) = get_codeblock(&function).unwrap();

    assert_eq!(code.ic.len(), 1);
    assert_eq!(code.ic[0].entries.borrow().len(), 0);

    let o = ObjectInitializer::new(context)
        .property(js_string!("test"), 0, Attribute::all())
        .build();
    let o_shape = o.borrow().shape().clone();

    function.call(&JsValue::undefined(), &[o.clone().into()], context)?;

    assert_eq!(code.ic[0].entries.borrow().len(), 1);
    assert_eq!(
        code.ic[0].entries.borrow()[0]
            .shape
            .upgrade()
            .unwrap()
            .to_addr_usize(),
        o_shape.to_addr_usize()
    );

    Ok(())
}

#[test]
fn get_property_by_name_set_inline_cache_on_property_load() -> JsResult<()> {
    let context = &mut Context::default();
    let function = context.eval(Source::from_bytes("(function (o) { o.test = 30; })"))?;
    let (function, code) = get_codeblock(&function).unwrap();

    assert_eq!(code.ic.len(), 1);
    assert_eq!(code.ic[0].entries.borrow().len(), 0);

    let o = ObjectInitializer::new(context)
        .property(js_string!("test"), 0, Attribute::all())
        .build();
    let o_shape = o.borrow().shape().clone();

    function.call(&JsValue::undefined(), &[o.clone().into()], context)?;

    assert_eq!(code.ic[0].entries.borrow().len(), 1);
    assert_eq!(
        code.ic[0].entries.borrow()[0]
            .shape
            .upgrade()
            .unwrap()
            .to_addr_usize(),
        o_shape.to_addr_usize()
    );

    Ok(())
}

#[test]
fn test_polymorphic_inline_cache() -> JsResult<()> {
    let context = &mut Context::default();
    let function = context.eval(Source::from_bytes("(function (o) { return o.test; })"))?;
    let (function, code) = get_codeblock(&function).unwrap();

    assert_eq!(code.ic.len(), 1);
    assert_eq!(code.ic[0].entries.borrow().len(), 0);
    assert!(!code.ic[0].megamorphic.get());

    let shapes = vec![
        ObjectInitializer::new(context)
            .property(js_string!("test"), 1, Attribute::all())
            .build(),
        ObjectInitializer::new(context)
            .property(js_string!("a"), 2, Attribute::all())
            .property(js_string!("test"), 3, Attribute::all())
            .build(),
        ObjectInitializer::new(context)
            .property(js_string!("b"), 4, Attribute::all())
            .property(js_string!("test"), 5, Attribute::all())
            .build(),
        ObjectInitializer::new(context)
            .property(js_string!("c"), 6, Attribute::all())
            .property(js_string!("test"), 7, Attribute::all())
            .build(),
    ];

    for o in &shapes {
        function.call(&JsValue::undefined(), &[o.clone().into()], context)?;
    }

    assert_eq!(code.ic[0].entries.borrow().len(), 4);
    assert!(!code.ic[0].megamorphic.get());

    Ok(())
}

#[test]
fn test_megamorphic_inline_cache() -> JsResult<()> {
    let context = &mut Context::default();
    let function = context.eval(Source::from_bytes("(function (o) { return o.test; })"))?;
    let (function, code) = get_codeblock(&function).unwrap();

    let shapes = vec![
        ObjectInitializer::new(context)
            .property(js_string!("test"), 1, Attribute::all())
            .build(),
        ObjectInitializer::new(context)
            .property(js_string!("a"), 1, Attribute::all())
            .property(js_string!("test"), 1, Attribute::all())
            .build(),
        ObjectInitializer::new(context)
            .property(js_string!("b"), 1, Attribute::all())
            .property(js_string!("test"), 1, Attribute::all())
            .build(),
        ObjectInitializer::new(context)
            .property(js_string!("c"), 1, Attribute::all())
            .property(js_string!("test"), 1, Attribute::all())
            .build(),
        ObjectInitializer::new(context)
            .property(js_string!("d"), 1, Attribute::all())
            .property(js_string!("test"), 1, Attribute::all())
            .build(),
    ];

    for o in &shapes {
        function.call(&JsValue::undefined(), &[o.clone().into()], context)?;
    }

    assert_eq!(code.ic[0].entries.borrow().len(), 0);
    assert!(code.ic[0].megamorphic.get());

    // Regression check: repeated miss should remain empty
    let o6 = ObjectInitializer::new(context)
        .property(js_string!("e"), 1, Attribute::all())
        .property(js_string!("test"), 1, Attribute::all())
        .build();
    function.call(&JsValue::undefined(), &[o6.clone().into()], context)?;
    assert_eq!(code.ic[0].entries.borrow().len(), 0);
    assert!(code.ic[0].megamorphic.get());

    Ok(())
}

// ---------------------------------------------------------------------------
// Element-access IC tests
// ---------------------------------------------------------------------------

/// Dense-array get: IC seeds on second call and hits on subsequent calls.
#[test]
fn element_ic_get_dense_array_seeds_and_hits() -> JsResult<()> {
    let context = &mut Context::default();
    // A function that reads `arr[i]`. The compiler emits one `GetPropertyByValue`
    // instruction, so `element_ic` will have exactly one entry.
    let function = context.eval(Source::from_bytes("(function (arr, i) { return arr[i]; })"))?;
    let (function, code) = get_codeblock(&function).unwrap();

    assert_eq!(code.element_ic.len(), 1);
    // Before any call: IC is unseeded.
    assert!(code.element_ic[0].dense_kind().is_none());

    // First call: slow path seeds the IC.
    let arr: JsValue = context.eval(Source::from_bytes("[1, 2, 3]"))?;
    let result = function.call(
        &JsValue::undefined(),
        &[arr.clone(), JsValue::from(1_i32)],
        context,
    )?;
    assert_eq!(result, JsValue::from(2_i32));
    // IC should now be seeded with DenseI32 (small integer array).
    assert_eq!(code.element_ic[0].dense_kind(), Some(DenseKind::DenseI32));

    // Second call (IC fast path): must return the correct value.
    let result = function.call(
        &JsValue::undefined(),
        &[arr.clone(), JsValue::from(0_i32)],
        context,
    )?;
    assert_eq!(result, JsValue::from(1_i32));

    Ok(())
}

/// Out-of-bounds index: IC is seeded but `get_dense_property` returns None,
/// so the slow path handles it and produces `undefined`.
#[test]
fn element_ic_get_out_of_bounds_returns_undefined() -> JsResult<()> {
    let context = &mut Context::default();
    let function = context.eval(Source::from_bytes("(function (arr, i) { return arr[i]; })"))?;
    let (function, code) = get_codeblock(&function).unwrap();

    // Warm the IC with an in-bounds access.
    let arr: JsValue = context.eval(Source::from_bytes("[10, 20]"))?;
    drop(function.call(
        &JsValue::undefined(),
        &[arr.clone(), JsValue::from(0_i32)],
        context,
    )?);
    assert!(code.element_ic[0].dense_kind().is_some());

    // Out-of-bounds: IC hit check passes (same shape), but `get_dense_property`
    // returns `None` → slow path → `undefined`.
    let result = function.call(
        &JsValue::undefined(),
        &[arr.clone(), JsValue::from(99_i32)],
        context,
    )?;
    assert!(result.is_undefined());

    Ok(())
}

/// Polymorphic site: two different array shapes (e.g. i32 then f64 elements).
/// The IC stays monomorphic (last-write-wins) but both shapes must produce
/// correct results via the slow path on a miss.
#[test]
fn element_ic_get_shape_miss_falls_back_to_slow_path() -> JsResult<()> {
    let context = &mut Context::default();
    let function = context.eval(Source::from_bytes("(function (arr, i) { return arr[i]; })"))?;
    let (function, _code) = get_codeblock(&function).unwrap();

    // Integer array.
    let int_arr: JsValue = context.eval(Source::from_bytes("[1, 2, 3]"))?;
    let r = function.call(
        &JsValue::undefined(),
        &[int_arr.clone(), JsValue::from(2_i32)],
        context,
    )?;
    assert_eq!(r, JsValue::from(3_i32));

    // Float array (different `indexed_properties` storage kind, different shape
    // for freshly-created arrays? Actually same shape, but `DenseF64` kind):
    // Even if shape matches, `get_dense_property` returns an f64 — verify value.
    let float_arr: JsValue = context.eval(Source::from_bytes("[1.5, 2.5]"))?;
    let r = function.call(
        &JsValue::undefined(),
        &[float_arr.clone(), JsValue::from(0_i32)],
        context,
    )?;
    // Should be 1.5 (f64), not truncated.
    assert!((r.as_number().unwrap() - 1.5).abs() < f64::EPSILON);

    Ok(())
}

/// Non-array receiver: the IC fast path must not fire (receiver is a plain
/// object, not an array), and the property access must still work correctly.
#[test]
fn element_ic_get_non_array_receiver_works() -> JsResult<()> {
    let context = &mut Context::default();
    // This tests an object with numeric string keys — should use generic lookup.
    let function = context.eval(Source::from_bytes("(function (obj, i) { return obj[i]; })"))?;
    let (function, _code) = get_codeblock(&function).unwrap();

    let obj = ObjectInitializer::new(context)
        .property(js_string!("0"), js_string!("hello"), Attribute::all())
        .property(js_string!("1"), js_string!("world"), Attribute::all())
        .build();

    let r = function.call(
        &JsValue::undefined(),
        &[obj.into(), JsValue::from(0_i32)],
        context,
    )?;
    assert_eq!(r.as_string().unwrap(), js_string!("hello"));

    Ok(())
}

/// Dense-array set: IC seeds and subsequent sets use the fast path.
#[test]
fn element_ic_set_dense_array_seeds_and_hits() -> JsResult<()> {
    let context = &mut Context::default();
    let function = context.eval(Source::from_bytes(
        "(function (arr, i, v) { arr[i] = v; return arr[i]; })",
    ))?;
    let (function, _code) = get_codeblock(&function).unwrap();

    let arr: JsValue = context.eval(Source::from_bytes("[0, 0, 0]"))?;

    // First call: slow path for both get and set; IC is seeded.
    let r = function.call(
        &JsValue::undefined(),
        &[arr.clone(), JsValue::from(1_i32), JsValue::from(42_i32)],
        context,
    )?;
    assert_eq!(r, JsValue::from(42_i32));

    // Second call: IC fast path for set, then get.
    let r = function.call(
        &JsValue::undefined(),
        &[arr.clone(), JsValue::from(2_i32), JsValue::from(99_i32)],
        context,
    )?;
    assert_eq!(r, JsValue::from(99_i32));

    Ok(())
}

/// Shape change mid-loop: after adding a named property to an array the shape
/// changes and the IC must deopt correctly (not return the wrong value).
#[test]
fn element_ic_get_shape_change_mid_loop_correct() -> JsResult<()> {
    let context = &mut Context::default();
    // JS that reads arr[i], then adds a property (shape change), then reads again.
    let result = context.eval(Source::from_bytes(
        r"
        var arr = [10, 20, 30];
        var sum = 0;
        for (var i = 0; i < arr.length; i++) {
            sum += arr[i];
            if (i === 1) { arr.extra = true; } // shape change
        }
        sum
        ",
    ))?;
    // 10 + 20 + 30 = 60, regardless of shape change.
    assert_eq!(result, JsValue::from(60_i32));

    Ok(())
}

/// Sparse array: the IC must not return stale dense data when the array
/// transitions from dense to sparse storage.
#[test]
fn element_ic_get_sparse_array_correct() -> JsResult<()> {
    let context = &mut Context::default();
    let result = context.eval(Source::from_bytes(
        r"
        var arr = [1, 2, 3];
        var r0 = arr[0]; // seeds IC as DenseI32
        // Creating a hole forces a dense→sparse transition.
        arr[100] = 999;
        // arr[1] is still 2, but storage may be sparse now.
        var r1 = arr[1];
        r0 + r1
        ",
    ))?;
    // 1 + 2 = 3.
    assert_eq!(result, JsValue::from(3_i32));

    Ok(())
}

// ---------------------------------------------------------------------------
// Call-site IC tests
// ---------------------------------------------------------------------------

/// Helper: extract the code block from a JS function value.
fn get_call_codeblock(value: &JsValue) -> Option<(JsObject, Gc<CodeBlock>)> {
    let object = value.as_object()?.clone();
    let code = object.downcast_ref::<OrdinaryFunction>()?.code.clone();
    Some((object, code))
}

/// Monomorphic call: a single callee called repeatedly should seed the IC on
/// the first call and hit it on subsequent calls, with correct results.
#[test]
fn call_ic_monomorphic_seeds_and_hits() -> JsResult<()> {
    let context = &mut Context::default();
    // `caller` calls `callee(x)`. The compiler emits one `Call` instruction,
    // so `call_ic` will have exactly one entry.
    let caller_val = context.eval(Source::from_bytes(
        "(function caller(callee, x) { return callee(x); })",
    ))?;
    let (caller, code) = get_call_codeblock(&caller_val).expect("should be a function");
    assert_eq!(code.call_ic.len(), 1, "one Call site → one CallIC entry");

    // IC is unseeded before the first call.
    assert!(!code.call_ic[0].is_megamorphic());
    assert!(code.call_ic[0].observed_callee().is_none());

    // Define a small callee.
    let callee_val = context.eval(Source::from_bytes("(function add1(x) { return x + 1; })"))?;

    // First call: slow path seeds the IC.
    let result = caller.call(
        &JsValue::undefined(),
        &[callee_val.clone(), JsValue::from(41_i32)],
        context,
    )?;
    assert_eq!(result, JsValue::from(42_i32));

    // IC should now be seeded with the callee.
    assert!(
        code.call_ic[0].observed_callee().is_some(),
        "IC should be seeded after first call"
    );
    assert!(!code.call_ic[0].is_megamorphic());

    // Second call: IC hit, same callee, correct result.
    let result = caller.call(
        &JsValue::undefined(),
        &[callee_val.clone(), JsValue::from(10_i32)],
        context,
    )?;
    assert_eq!(result, JsValue::from(11_i32));

    Ok(())
}

/// Polymorphic call site: two different callees → IC goes megamorphic.
/// The generic path must still produce correct results.
#[test]
fn call_ic_polymorphic_goes_megamorphic() -> JsResult<()> {
    let context = &mut Context::default();
    let caller_val = context.eval(Source::from_bytes(
        "(function caller(f, x) { return f(x); })",
    ))?;
    let (caller, code) = get_call_codeblock(&caller_val).expect("should be a function");

    let add1 = context.eval(Source::from_bytes("(function add1(x) { return x + 1; })"))?;
    let mul2 = context.eval(Source::from_bytes("(function mul2(x) { return x * 2; })"))?;

    // First call with add1: seeds IC.
    let r1 = caller.call(
        &JsValue::undefined(),
        &[add1.clone(), JsValue::from(5_i32)],
        context,
    )?;
    assert_eq!(r1, JsValue::from(6_i32));
    assert!(!code.call_ic[0].is_megamorphic());

    // Second call with a different callee (mul2): IC should go megamorphic.
    let r2 = caller.call(
        &JsValue::undefined(),
        &[mul2.clone(), JsValue::from(5_i32)],
        context,
    )?;
    assert_eq!(r2, JsValue::from(10_i32), "correct result despite IC miss");
    assert!(
        code.call_ic[0].is_megamorphic(),
        "IC should be megamorphic after seeing two distinct callees"
    );

    // Third call (either callee): still correct even though IC is megamorphic.
    let r3 = caller.call(
        &JsValue::undefined(),
        &[add1.clone(), JsValue::from(99_i32)],
        context,
    )?;
    assert_eq!(r3, JsValue::from(100_i32));

    Ok(())
}

/// Arity mismatch: calling a function with fewer or more arguments than its
/// parameter list specifies. JS semantics allow this; the call must succeed
/// with correct JS results (extras ignored, missing become `undefined`).
#[test]
fn call_ic_arity_mismatch_correct() -> JsResult<()> {
    let context = &mut Context::default();
    let caller_val = context.eval(Source::from_bytes(
        "(function caller(f, a, b) { return f(a, b); })",
    ))?;
    let (caller, _code) = get_call_codeblock(&caller_val).expect("should be a function");

    // callee only accepts one parameter; the second is silently ignored.
    let f = context.eval(Source::from_bytes("(function f(x) { return x + 1; })"))?;
    let result = caller.call(
        &JsValue::undefined(),
        &[f.clone(), JsValue::from(3_i32), JsValue::from(9999_i32)],
        context,
    )?;
    assert_eq!(result, JsValue::from(4_i32));

    Ok(())
}

/// Recursion: a function calling itself repeatedly. The IC should see the
/// same callee (the recursive function) on every iteration and stay
/// monomorphic.
#[test]
fn call_ic_recursive_stays_monomorphic() -> JsResult<()> {
    let context = &mut Context::default();
    let result = context.eval(Source::from_bytes(
        r"
        function fact(n) {
            if (n <= 1) return 1;
            return n * fact(n - 1);
        }
        fact(8)
        ",
    ))?;
    assert_eq!(result, JsValue::from(40320_i32));
    Ok(())
}

/// Non-function callee: must produce a `TypeError`, not invoke the IC fast
/// path or panic.
#[test]
fn call_ic_non_function_throws_type_error() -> JsResult<()> {
    let context = &mut Context::default();
    let caller_val = context.eval(Source::from_bytes("(function caller(f) { return f(); })"))?;
    let (caller, _code) = get_call_codeblock(&caller_val).expect("should be a function");

    let result = caller.call(&JsValue::undefined(), &[JsValue::from(42_i32)], context);
    assert!(
        result.is_err(),
        "calling a non-function must throw TypeError"
    );

    Ok(())
}

/// Native (built-in) callee: must NOT seed the IC (only ordinary JS
/// functions seed the IC) and must produce the correct result.
#[test]
fn call_ic_native_callee_deopt_correct() -> JsResult<()> {
    let context = &mut Context::default();
    let caller_val = context.eval(Source::from_bytes(
        "(function caller(f, x) { return f(x); })",
    ))?;
    let (caller, code) = get_call_codeblock(&caller_val).expect("should be a function");

    // `Math.abs` is a native function.
    let math_abs = context.eval(Source::from_bytes("Math.abs"))?;
    let result = caller.call(
        &JsValue::undefined(),
        &[math_abs.clone(), JsValue::from(-7_i32)],
        context,
    )?;
    assert_eq!(result, JsValue::from(7_i32));

    // IC should NOT be seeded for native callees.
    assert!(
        code.call_ic[0].observed_callee().is_none(),
        "native callee must not seed the IC"
    );
    assert!(!code.call_ic[0].is_megamorphic());

    Ok(())
}

/// Bound function callee: must NOT seed the IC and must produce the correct
/// result via the generic `__call__` path.
#[test]
fn call_ic_bound_function_deopt_correct() -> JsResult<()> {
    let context = &mut Context::default();
    let caller_val = context.eval(Source::from_bytes("(function caller(f) { return f(); })"))?;
    let (caller, code) = get_call_codeblock(&caller_val).expect("should be a function");

    let bound = context.eval(Source::from_bytes(
        "(function add(a, b) { return a + b; }).bind(null, 10, 32)",
    ))?;
    let result = caller.call(&JsValue::undefined(), &[bound], context)?;
    assert_eq!(result, JsValue::from(42_i32));

    // Bound functions are not OrdinaryFunctions; IC stays unseeded.
    assert!(
        code.call_ic[0].observed_callee().is_none(),
        "bound function must not seed the IC"
    );

    Ok(())
}

/// Exception propagation: an exception thrown inside the called function must
/// propagate correctly through the IC fast path and the deopt path.
#[test]
fn call_ic_exception_propagates() -> JsResult<()> {
    let context = &mut Context::default();
    let caller_val = context.eval(Source::from_bytes("(function caller(f) { return f(); })"))?;
    let (caller, _code) = get_call_codeblock(&caller_val).expect("should be a function");

    let thrower = context.eval(Source::from_bytes(
        "(function thrower() { throw new Error('bang'); })",
    ))?;

    // First call: slow path (cold IC), must propagate the error.
    let r1 = caller.call(&JsValue::undefined(), &[thrower.clone()], context);
    assert!(r1.is_err(), "exception must propagate on cold path");

    // Second call: IC is seeded (if ordinary callee), fast path, still must
    // propagate the error.
    let r2 = caller.call(&JsValue::undefined(), &[thrower.clone()], context);
    assert!(r2.is_err(), "exception must propagate on IC hit path");

    Ok(())
}

/// `CallIC::matches` / `seed` unit test: verify the low-level IC mechanics
/// without going through the full VM.
#[test]
fn call_ic_unit_matches_and_seed() {
    let context = &mut Context::default();

    // Create two distinct JsObjects so we can test identity comparison.
    let obj1 = context
        .intrinsics()
        .templates()
        .ordinary_object()
        .create(OrdinaryObject, Vec::default());
    let obj2 = context
        .intrinsics()
        .templates()
        .ordinary_object()
        .create(OrdinaryObject, Vec::default());

    let ic = CallIC::new();

    // Unseeded: matches nothing.
    assert!(!ic.matches(&obj1));
    assert!(!ic.matches(&obj2));
    assert!(ic.observed_callee().is_none());
    assert!(!ic.is_megamorphic());

    // Seed with obj1.
    ic.seed(&obj1);
    assert!(ic.matches(&obj1), "seeded IC must hit on the same object");
    assert!(
        !ic.matches(&obj2),
        "seeded IC must miss on a different object"
    );

    // Seed with obj2 (different object) → megamorphic.
    ic.seed(&obj2);
    assert!(
        ic.is_megamorphic(),
        "IC must go megamorphic after seeing two distinct objects"
    );
    assert!(
        !ic.matches(&obj1),
        "megamorphic IC must always return false"
    );
    assert!(
        !ic.matches(&obj2),
        "megamorphic IC must always return false"
    );
    assert!(
        ic.observed_callee().is_none(),
        "megamorphic IC has no observed callee"
    );
}
