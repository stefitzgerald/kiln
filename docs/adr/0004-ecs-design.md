# ADR 0004: Sparse-set ECS with exclusive-world systems

**Status:** Accepted (M0)

## Context
Godot uses a node/scene tree. Kiln uses an ECS for data layout, testability and
parallelism headroom, and layers a transform hierarchy on top. We need something small
enough to own and audit, but with a path to a scheduler and parallel systems.

## Decision
- **Entities**: 32-bit index + 32-bit generation. Slots are recycled with generation bumps
  and retired on overflow.
- **Storage**: one **sparse set per component type**. O(1) insert/remove/lookup, dense
  iteration. Adding a component never moves other components (unlike archetypes).
- **Components** opt in with `impl Component for T {}`. This explicit marker allows
  `Bundle` to be implemented for tuples without coherence conflicts. A derive macro is
  planned. **Resources** are any `Send + Sync + 'static` type.
- **Queries** are typed (`&T`, `&mut T`, `Option<_>`, `Entity`, `Has<T>`, tuples up to 8)
  with filters (`With`, `Without`). Iteration is driven by the smallest required storage.
  Conflicting access (`&mut A` with `&A`, or `&mut A` twice) is rejected **when the query
  is created**. `try_query` returns `QueryError::ConflictingAccess`; `query` panics with
  the component name.
- **Systems** are `FnMut(&mut World)` run by a staged schedule in `kiln_app`. Structural
  changes during iteration go through `Commands`. **Events** are double-buffered with
  per-reader cursors.

## Consequences
- Simple, auditable `unsafe`. Queries create references to individual storage fields,
  never the whole storage, so shared reads of membership coexist with exclusive
  component access.
- M0 systems run sequentially with exclusive world access. Parallel execution needs
  system parameter declarations; that is planned for a later milestone and the `Access`
  sets already exist to support it.
- There is no change detection yet, so assets are treated as immutable once uploaded by
  the renderer.
- Baseline performance is tracked by `cargo bench -p kiln_ecs` (TC-ECS-08). An archetype
  table storage can be added later for hot components if benchmarks justify it.
