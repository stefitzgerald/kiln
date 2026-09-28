/// Double-buffered event queue, stored as a resource.
///
/// Events live for two [`Events::update`] calls (normally two frames), so every system that
/// runs once per frame sees each event exactly once through its own [`EventCursor`],
/// regardless of whether it runs before or after the sender.
#[derive(Debug)]
pub struct Events<E> {
    /// Older buffer: events sent during the previous frame.
    previous: Vec<E>,
    /// Newer buffer: events sent during the current frame.
    current: Vec<E>,
    /// Id of the first event in `previous`.
    previous_start: usize,
    /// Total events ever sent.
    count: usize,
}

impl<E> Default for Events<E> {
    fn default() -> Self {
        Self { previous: Vec::new(), current: Vec::new(), previous_start: 0, count: 0 }
    }
}

impl<E> Events<E> {
    /// Send an event.
    pub fn send(&mut self, event: E) {
        self.current.push(event);
        self.count += 1;
    }

    /// Advance one frame: events older than two updates are dropped.
    pub fn update(&mut self) {
        self.previous_start += self.previous.len();
        self.previous = std::mem::take(&mut self.current);
    }

    /// Number of buffered events.
    pub fn len(&self) -> usize {
        self.previous.len() + self.current.len()
    }

    /// `true` if no events are buffered.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Iterate all buffered events, oldest first, without tracking what was seen.
    pub fn iter(&self) -> impl Iterator<Item = &E> {
        self.previous.iter().chain(self.current.iter())
    }

    /// Drop all events.
    pub fn clear(&mut self) {
        self.previous_start = self.count;
        self.previous.clear();
        self.current.clear();
    }

    /// A cursor that will only see events sent from now on.
    pub fn cursor_at_end(&self) -> EventCursor<E> {
        EventCursor { next: self.count, _marker: std::marker::PhantomData }
    }
}

/// Tracks which events a reader has already seen.
#[derive(Debug)]
pub struct EventCursor<E> {
    next: usize,
    _marker: std::marker::PhantomData<fn() -> E>,
}

impl<E> Default for EventCursor<E> {
    fn default() -> Self {
        Self { next: 0, _marker: std::marker::PhantomData }
    }
}

impl<E> Clone for EventCursor<E> {
    fn clone(&self) -> Self {
        Self { next: self.next, _marker: std::marker::PhantomData }
    }
}

impl<E> EventCursor<E> {
    /// Iterate events not yet seen by this cursor, and mark them seen.
    pub fn read<'a>(&mut self, events: &'a Events<E>) -> impl Iterator<Item = &'a E> + use<'a, E> {
        let start = self.next.max(events.previous_start);
        let skip = start - events.previous_start;
        self.next = events.count;
        events.iter().skip(skip)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_live_two_updates() {
        let mut ev = Events::default();
        let mut cursor = EventCursor::default();
        ev.send(1);
        ev.update();
        ev.send(2);
        assert_eq!(cursor.read(&ev).copied().collect::<Vec<_>>(), [1, 2]);
        assert_eq!(cursor.read(&ev).count(), 0, "already seen");
        ev.update();
        ev.update();
        assert!(ev.is_empty());
        ev.send(3);
        assert_eq!(cursor.read(&ev).copied().collect::<Vec<_>>(), [3]);
    }

    #[test]
    fn late_reader_skips_dropped_events() {
        let mut ev = Events::default();
        ev.send(1);
        ev.update();
        ev.update();
        ev.send(2);
        let mut cursor = EventCursor::default();
        assert_eq!(cursor.read(&ev).copied().collect::<Vec<_>>(), [2]);
        let mut fresh = ev.cursor_at_end();
        ev.send(3);
        assert_eq!(fresh.read(&ev).copied().collect::<Vec<_>>(), [3]);
    }
}
