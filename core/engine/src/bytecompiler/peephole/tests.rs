//! Tests for the peephole Move-elision analysis.
//!
//! Each test constructs synthetic bytecode via the same
//! [`BytecodeEmitter`] the real bytecompiler uses, then asserts the
//! analysis either does or does not flag a specific `Move + Op` pair
//! for elision. The "should NOT elide" cases are the load-bearing ones
//! — they encode the ES §13.15.2 hazard the analysis exists to prevent.

use thin_vec::thin_vec;

use crate::vm::{
    Handler,
    opcode::{Address, BytecodeEmitter, IndexOperand, Opcode, RegisterOperand},
};

use super::{Elision, elide_moves, find_safe_move_elisions};

fn r(i: u32) -> RegisterOperand {
    RegisterOperand::from(i)
}

fn idx(i: u32) -> IndexOperand {
    IndexOperand::from(i)
}

fn read_u32(bytes: &[u8], pos: usize) -> u32 {
    u32::from_le_bytes(bytes[pos..pos + 4].try_into().unwrap())
}

#[test]
fn elides_safe_move_before_set_property_by_name() {
    // Pattern: Move tmp, src; SetPropertyByName value, tmp, ic
    // No further uses of `tmp` — the analysis should flag the elision.
    let mut emit = BytecodeEmitter::new();
    let move_pc = u32::from(emit.next_opcode_location());
    emit.emit_move(/* dst */ r(10), /* src */ r(2));
    let op_pc = u32::from(emit.next_opcode_location());
    emit.emit_set_property_by_name(/* value */ r(3), /* object */ r(10), idx(0));

    let bytecode = emit.into_bytecode();
    let elisions = find_safe_move_elisions(&bytecode, &[]);

    assert_eq!(
        elisions,
        vec![Elision {
            move_pc,
            op_pc,
            src: r(2),
            dst: r(10),
            // SetPropertyByName: register operands are [value, object]
            // and `tmp` (r10) is at position 1.
            op_operand_idx: 1,
        }],
        "Move feeding SetPropertyByName should be elidable when tmp is dead after",
    );
}

#[test]
fn refuses_elision_when_dst_is_read_again() {
    // The ES §13.15.2 hazard, simplified:
    //   Move tmp, src                    ; snapshot
    //   SetPropertyByName value, tmp     ; consume snapshot
    //   PushFromRegister tmp             ; reads tmp again
    // Eliding the Move would let `PushFromRegister` see whatever `src`
    // currently holds rather than the snapshot.
    let mut emit = BytecodeEmitter::new();
    emit.emit_move(r(10), r(2));
    emit.emit_set_property_by_name(r(3), r(10), idx(0));
    emit.emit_push_from_register(r(10));

    let elisions = find_safe_move_elisions(&emit.into_bytecode(), &[]);
    assert!(
        elisions.is_empty(),
        "elision should be refused when tmp is read later: {elisions:?}",
    );
}

#[test]
fn allows_elision_when_dst_is_overwritten_before_read() {
    // After `tmp` is consumed by `SetPropertyByName`, a subsequent Move
    // writes a fresh value into `tmp` before anything reads it. The
    // original Move's value can't escape — elision is safe.
    //
    // Note: the second `Move r10, r7; PushFromRegister r10` pair is
    // also a valid elision candidate (the dead-after analysis sees
    // PushFromRegister reads r10 once with nothing reading r10
    // afterward). We only assert on the first pair here — the one
    // that demonstrates the "dst-overwritten-before-read" property
    // this test names.
    let mut emit = BytecodeEmitter::new();
    let move_pc = u32::from(emit.next_opcode_location());
    emit.emit_move(r(10), r(2));
    let op_pc = u32::from(emit.next_opcode_location());
    emit.emit_set_property_by_name(r(3), r(10), idx(0));
    // Overwrite `tmp` with a different source — `tmp`'s post-elision
    // observable value is whatever this later Move writes.
    emit.emit_move(r(10), r(7));
    emit.emit_push_from_register(r(10));

    let elisions = find_safe_move_elisions(&emit.into_bytecode(), &[]);
    let first = elisions
        .iter()
        .find(|e| e.move_pc == move_pc)
        .copied()
        .expect("expected an elision for the first Move");
    assert_eq!(
        first,
        Elision {
            move_pc,
            op_pc,
            src: r(2),
            dst: r(10),
            op_operand_idx: 1,
        },
    );
}

#[test]
fn refuses_elision_when_op_writes_dst() {
    // Move tmp, src; GetPropertyByName tmp, value, ic
    // GetPropertyByName writes `tmp` (it's the `dst` field). Eliding the
    // preceding Move would not change semantics here in the trivial
    // sense — the Move's value is immediately overwritten anyway — but
    // the analysis refuses because `dst` appears as a Write in the next
    // op, not as a Read. That's a stricter (safer) interpretation; a
    // smarter rewriter could legitimately drop the Move *and* leave the
    // Op alone, but that's a separate optimization.
    let mut emit = BytecodeEmitter::new();
    emit.emit_move(r(10), r(2));
    emit.emit_get_property_by_name(/* dst */ r(10), /* value */ r(3), idx(0));

    let elisions = find_safe_move_elisions(&emit.into_bytecode(), &[]);
    assert!(
        elisions.is_empty(),
        "GetPropertyByName writes its dst — elision refused as a Read substitution",
    );
}

#[test]
fn refuses_elision_when_next_op_is_unknown() {
    // The next opcode after the Move isn't in our operand-info
    // whitelist. The analysis must fail closed.
    let mut emit = BytecodeEmitter::new();
    emit.emit_move(r(10), r(2));
    // StoreUndefined isn't in the whitelist.
    emit.emit_store_undefined(r(11));

    let elisions = find_safe_move_elisions(&emit.into_bytecode(), &[]);
    assert!(
        elisions.is_empty(),
        "unknown next-op must abort the elision attempt",
    );
}

#[test]
fn refuses_elision_when_subsequent_op_is_unknown() {
    // The pattern is fine up to the SetPropertyByName, but then we hit
    // an opcode we don't have metadata for. The forward dead-after
    // scan must fail closed — for all we know, that opcode reads `tmp`.
    let mut emit = BytecodeEmitter::new();
    emit.emit_move(r(10), r(2));
    emit.emit_set_property_by_name(r(3), r(10), idx(0));
    emit.emit_store_undefined(r(99));

    let elisions = find_safe_move_elisions(&emit.into_bytecode(), &[]);
    assert!(
        elisions.is_empty(),
        "unknown opcode in forward scan must abort: {elisions:?}",
    );
}

#[test]
fn refuses_elision_when_dst_appears_twice() {
    // SetPropertyByNameWithThis takes [object, receiver, value]. If
    // both `object` and `receiver` happen to be `tmp`, retargeting only
    // one of them would change semantics. The analysis bails.
    let mut emit = BytecodeEmitter::new();
    emit.emit_move(r(10), r(2));
    emit.emit_set_property_by_name_with_this(
        /* value */ r(3),
        /* receiver */ r(10),
        /* object */ r(10),
        idx(0),
    );

    let elisions = find_safe_move_elisions(&emit.into_bytecode(), &[]);
    assert!(
        elisions.is_empty(),
        "elision must refuse when tmp appears in multiple operand slots",
    );
}

#[test]
fn finds_multiple_independent_elisions() {
    // Two independent Move + Op pairs in the same bytecode, both safe.
    let mut emit = BytecodeEmitter::new();
    let m1 = u32::from(emit.next_opcode_location());
    emit.emit_move(r(10), r(2));
    let o1 = u32::from(emit.next_opcode_location());
    emit.emit_set_property_by_name(r(3), r(10), idx(0));
    let m2 = u32::from(emit.next_opcode_location());
    emit.emit_move(r(11), r(5));
    let o2 = u32::from(emit.next_opcode_location());
    emit.emit_set_property_by_name(r(6), r(11), idx(1));

    let elisions = find_safe_move_elisions(&emit.into_bytecode(), &[]);
    assert_eq!(elisions.len(), 2, "should find both elisions: {elisions:?}");
    assert_eq!(elisions[0].move_pc, m1);
    assert_eq!(elisions[0].op_pc, o1);
    assert_eq!(elisions[1].move_pc, m2);
    assert_eq!(elisions[1].op_pc, o2);
}

#[test]
fn refuses_elision_when_op_is_a_jump_target() {
    // The data-flow conditions hold, but a backward `Jump` lands on the `Op`.
    // On that path the `Move` never ran, so `tmp` and `src` may differ —
    // retargeting `Op` to read `src` would miscompile the jumped-to path.
    // The analysis (which has no CFG) must refuse based on the target set.
    let mut emit = BytecodeEmitter::new();
    emit.emit_move(r(10), r(2));
    let op_pc = u32::from(emit.next_opcode_location());
    emit.emit_set_property_by_name(r(3), r(10), idx(0));
    let jump_pc = emit.next_opcode_location();
    emit.emit_jump(Address::new(0)); // placeholder
    emit.patch_jump(jump_pc, Address::new(op_pc)); // jump back to the Op

    let elisions = find_safe_move_elisions(&emit.into_bytecode(), &[]);
    assert!(
        elisions.is_empty(),
        "must refuse: Op is reachable via a jump that bypasses the Move: {elisions:?}",
    );
}

#[test]
fn refuses_elision_when_op_is_a_handler_catch_entry() {
    // A `Handler` whose catch entry (`end`) is the `Op` means an exception can
    // transfer control to `Op` without executing the `Move`. Same hazard as a
    // jump target; the analysis must consult handlers too.
    let mut emit = BytecodeEmitter::new();
    emit.emit_move(r(10), r(2));
    let op_pc = u32::from(emit.next_opcode_location());
    emit.emit_set_property_by_name(r(3), r(10), idx(0));

    let handlers = [Handler {
        start: Address::new(0),
        end: Address::new(op_pc),
        environment_count: 0,
    }];
    let elisions = find_safe_move_elisions(&emit.into_bytecode(), &handlers);
    assert!(
        elisions.is_empty(),
        "must refuse: Op is a handler catch entry: {elisions:?}",
    );
}

#[test]
fn rewriter_removes_move_patches_consumer_and_remaps_jump() {
    // End-to-end byte surgery: a `Jump` whose target sits *after* an elidable
    // `Move` must have its absolute address decremented by the 9 bytes the
    // `Move` occupied, the consumer's operand must be retargeted tmp -> src,
    // and the deleted Move's bytes must be gone.
    let mut emit = BytecodeEmitter::new();
    let jump_pc = emit.next_opcode_location(); // 0
    emit.emit_jump(Address::new(0)); // placeholder, 5 bytes
    emit.emit_move(r(10), r(2)); // elidable, 9 bytes
    emit.emit_set_property_by_name(r(3), r(10), idx(0)); // consumer, 13 bytes
    let target = emit.next_opcode_location(); // jump destination, after the Move
    emit.emit_push_from_register(r(3)); // reads r3, leaves r10 dead
    emit.patch_jump(jump_pc, target);

    let old_target = u32::from(target);
    let bytecode = emit.into_bytecode();
    let old_len = bytecode.bytes.len();

    let rewritten = elide_moves(bytecode, thin_vec![], Box::default());
    let out = &rewritten.bytecode.bytes;

    // The 9-byte Move is gone.
    assert_eq!(out.len(), old_len - 9, "Move bytes should be removed");
    // Jump address remapped past the deletion.
    assert_eq!(
        read_u32(out, u32::from(jump_pc) as usize + 1),
        old_target - 9,
        "jump target must be decremented by the removed Move width",
    );
    // The instruction now at the remapped target is the PushFromRegister.
    assert_eq!(
        Opcode::decode(out[(old_target - 9) as usize]),
        Opcode::PushFromRegister,
        "remapped jump must still land on its intended instruction",
    );
    // Consumer's `object` operand (2nd register, at +1 + 4) is now `src` (r2).
    // The consumer follows the 5-byte Jump directly now that the Move is gone.
    let consumer_pc = u32::from(jump_pc) as usize + 5;
    assert_eq!(
        Opcode::decode(out[consumer_pc]),
        Opcode::SetPropertyByName,
    );
    assert_eq!(
        read_u32(out, consumer_pc + 1 + 4),
        u32::from(r(2)),
        "consumer operand must be retargeted from tmp (r10) to src (r2)",
    );
}
