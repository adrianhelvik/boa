use arrayvec::ArrayVec;
use itertools::Itertools;
use std::{cell::Cell, fmt};

use boa_gc::GcRefCell;
use boa_macros::{Finalize, Trace};

use crate::{
    JsString,
    object::shape::{Shape, WeakShape, slot::Slot},
};

#[cfg(test)]
mod tests;

pub(crate) const PIC_CAPACITY: usize = 4;

/// A cached shape-to-slot mapping for a polymorphic inline cache.
///
/// `shape_addr` is the live heap address of the cached shape's GC-managed
/// inner allocation. It's the hot field used for the equality check on the
/// fast path. `weak` keeps a weak reference to the shape for liveness
/// verification — needed to avoid a false-positive match if the cached
/// shape was dropped and the heap address was reused for a different shape.
/// Checking `weak.is_upgradable()` is cheap (no atomic ref-count traffic),
/// unlike `weak.upgrade()` which the previous implementation called.
#[derive(Clone, Debug, Trace, Finalize)]
pub(crate) struct CacheEntry {
    /// Address of the cached shape's `Inner` GC allocation. Used for the
    /// pointer-equality check that dominates the IC hit path.
    #[unsafe_ignore_trace]
    pub(crate) shape_addr: usize,

    /// Weak reference to the cached shape. Only consulted on the IC hit
    /// path for an aliveness check (`is_upgradable()`, no atomic ops); the
    /// `upgrade()` path is reserved for cold operations.
    pub(crate) shape: WeakShape,

    /// Slot within the shape's property table where the property lives.
    #[unsafe_ignore_trace]
    pub(crate) slot: Slot,
}

/// An inline cache entry for a property access.
#[derive(Clone, Debug, Trace, Finalize)]
pub(crate) struct InlineCache {
    /// The property that is accessed.
    pub(crate) name: JsString,

    /// Multiple cached shape-to-slot entries.
    pub(crate) entries: GcRefCell<ArrayVec<CacheEntry, PIC_CAPACITY>>,

    /// Whether this access site has seen too many shapes and should no longer be cached.
    #[unsafe_ignore_trace]
    pub(crate) megamorphic: Cell<bool>,
}

impl fmt::Display for InlineCache {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "(name:{} entries:", self.name.display_escaped())?;

        if self.megamorphic.get() {
            return write!(f, "(megamorphic))");
        }

        let entries = self.entries.borrow();
        let entries = entries.iter().map(|e| e.shape_addr).format(", ");

        write!(f, "({entries:#x}))")
    }
}

impl InlineCache {
    pub(crate) fn new(name: JsString) -> Self {
        Self {
            name,
            entries: GcRefCell::new(ArrayVec::new()),
            megamorphic: Cell::new(false),
        }
    }

    /// Cache a `(shape, slot)` pair. If the cache is full, transition to
    /// megamorphic and stop caching. Stale (dead-weak) entries are evicted
    /// before deciding whether the cache is full.
    pub(crate) fn set(&self, shape: &Shape, slot: Slot) {
        if self.megamorphic.get() {
            return;
        }

        let mut entries = self.entries.borrow_mut();

        // Cleanup pass: drop entries whose shape has been collected. This
        // is the only place we pay the cost of touching weak refs, since
        // `set` runs only on misses.
        entries.retain(|entry| entry.shape.is_upgradable());

        let new_entry = CacheEntry {
            shape_addr: shape.to_addr_usize(),
            shape: shape.into(),
            slot,
        };

        if entries.try_push(new_entry).is_err() {
            // Polymorphic cache is full, transition to megamorphic.
            self.megamorphic.set(true);
            entries.clear();
        }
    }

    /// Fast IC lookup. Returns the cached `Slot` for the given shape, or
    /// `None` on miss / megamorphic / cache-stale.
    ///
    /// This is the hot path of every cached property access. The work is:
    ///   * one branch on `megamorphic`
    ///   * a borrow of the entries vec (debug-checked refcount in
    ///     `GcRefCell`, ~one load + cmov)
    ///   * up to `PIC_CAPACITY` (=4) pointer-equality checks against the
    ///     cached shape addresses
    ///   * one liveness check via `WeakShape::is_upgradable()`, which is
    ///     a plain memory load — no atomic ops
    ///
    /// The previous implementation called `WeakShape::upgrade()` per
    /// candidate entry, costing two atomic ref-count operations per IC hit
    /// (one to construct the `Gc`, one to drop it).
    #[inline]
    pub(crate) fn get(&self, shape: &Shape) -> Option<Slot> {
        if self.megamorphic.get() {
            return None;
        }

        let entries = self.entries.borrow();
        let shape_addr = shape.to_addr_usize();

        for entry in entries.iter() {
            if entry.shape_addr == shape_addr && entry.shape.is_upgradable() {
                return Some(entry.slot);
            }
        }

        None
    }
}
