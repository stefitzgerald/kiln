//! ECS acceptance tests (TC-ECS-*). See docs/testing/M0-test-plan.md.

#![allow(clippy::unwrap_used, clippy::chunks_exact_to_as_chunks)]

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use kiln_ecs::*;
use proptest::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq)]
struct A(i32);
impl Component for A {}
#[derive(Debug, Clone, Copy, PartialEq)]
struct B(i32);
impl Component for B {}
#[derive(Debug, Clone, Copy, PartialEq)]
struct C;
impl Component for C {}

/// TC-ECS-01
#[test]
fn tc_ecs_01_spawn_despawn_reuses_index_with_new_generation() {
    let mut w = World::new();
    let e1 = w.spawn(A(1));
    assert!(w.despawn(e1));
    assert!(!w.despawn(e1), "second despawn is a no-op");
    let e2 = w.spawn(A(2));
    assert_eq!(e2.index(), e1.index());
    assert_ne!(e2.generation(), e1.generation());
    assert!(!w.is_alive(e1));
    assert!(w.is_alive(e2));
    assert_eq!(
        w.get::<A>(e1),
        None,
        "stale id must not see the new entity's data"
    );
    assert_eq!(w.get::<A>(e2), Some(&A(2)));
    assert_eq!(w.insert(e1, B(0)), Err(EntityError::NoSuchEntity(e1)));
}

/// TC-ECS-02
#[test]
fn tc_ecs_02_tuple_query_visits_only_matches_and_mutations_persist() {
    let mut w = World::new();
    let ab1 = w.spawn((A(1), B(10)));
    let _a = w.spawn(A(2));
    let _b = w.spawn(B(20));
    let ab2 = w.spawn((A(3), B(30)));

    let mut seen = Vec::new();
    for (e, a, b) in w.query::<(Entity, &A, &mut B)>() {
        b.0 += a.0;
        seen.push(e);
    }
    seen.sort();
    assert_eq!(seen, vec![ab1, ab2]);
    assert_eq!(w.get::<B>(ab1), Some(&B(11)));
    assert_eq!(w.get::<B>(ab2), Some(&B(33)));

    // Option<&T> visits everyone with the required parts.
    let with_opt: Vec<_> = w
        .query::<(&A, Option<&B>)>()
        .map(|(a, b)| (a.0, b.copied()))
        .collect();
    assert_eq!(with_opt.len(), 3);
    assert!(with_opt.contains(&(2, None)));

    // Querying a component type that was never inserted matches nothing.
    #[derive(Debug)]
    struct Never;
    impl Component for Never {}
    assert_eq!(w.query::<&Never>().count(), 0);
}

/// TC-ECS-03
#[test]
fn tc_ecs_03_with_without_filters() {
    let mut w = World::new();
    let a = w.spawn(A(0));
    let ac = w.spawn((A(1), C));
    let abc = w.spawn((A(2), B(0), C));

    let mut with_c: Vec<_> = w.query_filtered::<Entity, (With<A>, With<C>)>().collect();
    with_c.sort();
    assert_eq!(with_c, vec![ac, abc]);

    let without_c: Vec<_> = w
        .query_filtered::<Entity, (With<A>, Without<C>)>()
        .collect();
    assert_eq!(without_c, vec![a]);

    let mixed: Vec<_> = w
        .query_filtered::<&A, (With<C>, Without<B>)>()
        .map(|a| a.0)
        .collect();
    assert_eq!(mixed, vec![1]);

    let has: Vec<_> = w.query::<(&A, Has<B>)>().map(|(a, h)| (a.0, h)).collect();
    assert_eq!(has.len(), 3);
    assert!(has.contains(&(2, true)) && has.contains(&(0, false)));
}

/// TC-ECS-04
#[test]
fn tc_ecs_04_runtime_add_remove_updates_membership() {
    let mut w = World::new();
    let e = w.spawn(A(1));
    assert_eq!(w.query::<(&A, &B)>().count(), 0);
    w.insert(e, B(5)).unwrap();
    assert_eq!(w.query::<(&A, &B)>().count(), 1);
    assert_eq!(w.remove::<B>(e), Some(B(5)));
    assert_eq!(w.query::<(&A, &B)>().count(), 0);
    assert!(w.has::<A>(e));
    // Re-inserting replaces.
    w.insert(e, A(9)).unwrap();
    assert_eq!(w.get::<A>(e), Some(&A(9)));
    assert_eq!(w.query::<&A>().count(), 1);
}

/// TC-ECS-05
#[test]
fn tc_ecs_05_commands_defer_despawn_until_apply() {
    let mut w = World::new();
    for i in 0..10 {
        w.spawn(A(i));
    }
    let mut cmds = Commands::new();
    let mut visited = 0;
    for (e, a) in w.query::<(Entity, &A)>() {
        visited += 1;
        if a.0 % 2 == 0 {
            cmds.despawn(e);
            cmds.despawn(e); // duplicate despawn is harmless
        } else {
            cmds.insert(e, B(a.0));
        }
    }
    assert_eq!(visited, 10, "iteration is unaffected by queued commands");
    assert_eq!(w.entity_count(), 10);
    cmds.spawn((A(100), C));
    cmds.apply(&mut w);
    assert!(cmds.is_empty());
    assert_eq!(w.entity_count(), 6);
    assert_eq!(w.query::<(&A, &B)>().count(), 5);
    assert_eq!(w.query_filtered::<&A, With<C>>().count(), 1);
}

/// TC-ECS-06
#[test]
fn tc_ecs_06_resources() {
    #[derive(Debug, Default, PartialEq)]
    struct Score(u32);

    let mut w = World::new();
    assert!(w.resource::<Score>().is_none());
    assert_eq!(w.insert_resource(Score(1)), None);
    assert_eq!(w.insert_resource(Score(2)), Some(Score(1)));
    w.resource_mut::<Score>().unwrap().0 += 1;
    assert_eq!(w.resource::<Score>(), Some(&Score(3)));
    let doubled = w.resource_scope(|world, s: &mut Score| {
        world.spawn(A(0));
        s.0 *= 2;
        s.0
    });
    assert_eq!(doubled, Some(6));
    assert_eq!(w.remove_resource::<Score>(), Some(Score(6)));
    assert!(!w.contains_resource::<Score>());
    assert_eq!(w.init_resource::<Score>().0, 0);
}

/// TC-ECS-07
#[test]
fn tc_ecs_07_aliasing_access_is_rejected() {
    let mut w = World::new();
    w.spawn((A(1), B(1)));
    let err = w.try_query::<(&mut A, &A)>().unwrap_err();
    assert!(
        matches!(err, QueryError::ConflictingAccess { component } if component.ends_with("::A"))
    );
    assert!(w.try_query::<(&mut A, &mut A)>().is_err());
    assert!(w.try_query::<(&A, Option<&mut A>)>().is_err());
    // Shared + shared, and filters, are fine.
    assert!(w.try_query::<(&A, &A)>().is_ok());
    assert!(w.try_query_filtered::<&mut A, With<A>>().is_ok());
    let msg = err.to_string();
    assert!(msg.contains("conflicting access"), "{msg}");
}

#[test]
#[should_panic(expected = "conflicting access")]
fn tc_ecs_07_query_panics_with_clear_message() {
    let mut w = World::new();
    let _ = w.query::<(&mut B, &B)>();
}

#[test]
fn components_are_dropped_on_despawn() {
    struct Tracked(Arc<AtomicUsize>);
    impl Component for Tracked {}
    impl Drop for Tracked {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    let drops = Arc::new(AtomicUsize::new(0));
    let mut w = World::new();
    let e = w.spawn(Tracked(drops.clone()));
    w.spawn(Tracked(drops.clone()));
    w.despawn(e);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    drop(w);
    assert_eq!(drops.load(Ordering::SeqCst), 2);
}

#[test]
fn query_ref_and_query_one() {
    let mut w = World::new();
    let e = w.spawn((A(1), B(2)));
    let world_ref: &World = &w;
    assert_eq!(world_ref.query_ref::<&A, ()>().count(), 1);
    if let Some((a, b)) = w.query_one::<(&mut A, &B)>(e) {
        a.0 += b.0;
    }
    assert_eq!(w.get::<A>(e), Some(&A(3)));
    w.despawn(e);
    assert!(w.query_one::<&A>(e).is_none());
}

#[test]
fn events_as_resource() {
    let mut w = World::new();
    w.init_resource::<Events<u32>>().send(7);
    let mut cursor = EventCursor::<u32>::default();
    let got: Vec<u32> = cursor
        .read(w.resource::<Events<u32>>().unwrap())
        .copied()
        .collect();
    assert_eq!(got, [7]);
}

#[derive(Debug, Clone)]
enum Op {
    Spawn(i32, bool),
    Despawn(usize),
    AddB(usize, i32),
    RemoveB(usize),
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        (any::<i32>(), any::<bool>()).prop_map(|(v, b)| Op::Spawn(v, b)),
        any::<usize>().prop_map(Op::Despawn),
        (any::<usize>(), any::<i32>()).prop_map(|(i, v)| Op::AddB(i, v)),
        any::<usize>().prop_map(Op::RemoveB),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    /// Randomized model check: world contents always match a simple Vec model.
    #[test]
    fn world_matches_model(ops in proptest::collection::vec(op(), 0..500)) {
        let mut w = World::new();
        let mut model: Vec<(Entity, i32, Option<i32>)> = Vec::new();
        for op in ops {
            match op {
                Op::Spawn(v, with_b) => {
                    let e = if with_b { w.spawn((A(v), B(v))) } else { w.spawn(A(v)) };
                    model.push((e, v, with_b.then_some(v)));
                }
                Op::Despawn(i) if !model.is_empty() => {
                    let (e, ..) = model.swap_remove(i % model.len());
                    prop_assert!(w.despawn(e));
                }
                Op::AddB(i, v) if !model.is_empty() => {
                    let n = model.len();
                    let m = &mut model[i % n];
                    w.insert(m.0, B(v)).unwrap();
                    m.2 = Some(v);
                }
                Op::RemoveB(i) if !model.is_empty() => {
                    let n = model.len();
                    let m = &mut model[i % n];
                    prop_assert_eq!(w.remove::<B>(m.0).map(|b| b.0), m.2.take());
                }
                _ => {}
            }
        }
        prop_assert_eq!(w.entity_count(), model.len());
        for (e, a, b) in &model {
            prop_assert_eq!(w.get::<A>(*e).map(|c| c.0), Some(*a));
            prop_assert_eq!(w.get::<B>(*e).map(|b| b.0), *b);
        }
        let with_b = model.iter().filter(|m| m.2.is_some()).count();
        prop_assert_eq!(w.query::<(&A, &B)>().count(), with_b);
        prop_assert_eq!(w.query_filtered::<&A, Without<B>>().count(), model.len() - with_b);
    }
}
