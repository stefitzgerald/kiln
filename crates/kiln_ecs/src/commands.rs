use crate::{Bundle, Component, Entity, Resource, World};

type Command = Box<dyn FnOnce(&mut World) + Send + Sync>;

/// Queue of deferred world mutations.
///
/// Use it to spawn, despawn or change components while a query borrows the world, then
/// [`Commands::apply`] it afterwards. The schedule applies the commands of each system
/// automatically once that system returns.
#[derive(Default)]
pub struct Commands {
    queue: Vec<Command>,
}

impl std::fmt::Debug for Commands {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Commands").field("len", &self.queue.len()).finish()
    }
}

impl Commands {
    /// Empty queue.
    pub fn new() -> Self {
        Self::default()
    }

    /// Queue an arbitrary world mutation.
    pub fn push(&mut self, f: impl FnOnce(&mut World) + Send + Sync + 'static) {
        self.queue.push(Box::new(f));
    }

    /// Queue spawning an entity.
    pub fn spawn<B: Bundle>(&mut self, bundle: B) {
        self.push(move |w| {
            w.spawn(bundle);
        });
    }

    /// Queue despawning an entity. Despawning an already-dead entity is a no-op.
    pub fn despawn(&mut self, entity: Entity) {
        self.push(move |w| {
            w.despawn(entity);
        });
    }

    /// Queue inserting components. Ignored if the entity is dead when applied.
    pub fn insert<B: Bundle>(&mut self, entity: Entity, bundle: B) {
        self.push(move |w| {
            let _ = w.insert(entity, bundle);
        });
    }

    /// Queue removing a component.
    pub fn remove<T: Component>(&mut self, entity: Entity) {
        self.push(move |w| {
            w.remove::<T>(entity);
        });
    }

    /// Queue inserting a resource.
    pub fn insert_resource<R: Resource>(&mut self, resource: R) {
        self.push(move |w| {
            w.insert_resource(resource);
        });
    }

    /// Number of queued commands.
    pub fn len(&self) -> usize {
        self.queue.len()
    }

    /// `true` if nothing is queued.
    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    /// Apply all queued commands in order, leaving the queue empty.
    pub fn apply(&mut self, world: &mut World) {
        for cmd in self.queue.drain(..) {
            cmd(world);
        }
    }
}
