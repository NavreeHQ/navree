//! In-process single flight: one computation per key at a time.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use tokio::sync::OwnedMutexGuard;

/// One async mutex per key that currently has a computation in flight.
/// Entries are removed when the last holder or waiter is gone.
#[derive(Default)]
pub(crate) struct Flights {
    slots: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
}

impl Flights {
    /// Waits until no other task holds `key`, then holds it until the guard drops.
    pub(crate) async fn enter(&self, key: &str) -> FlightGuard<'_> {
        let slot = self.lock().entry(key.to_owned()).or_default().clone();
        let guard = slot.clone().lock_owned().await;
        FlightGuard {
            flights: self,
            key: key.to_owned(),
            slot: Some(slot),
            guard: Some(guard),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Arc<tokio::sync::Mutex<()>>>> {
        self.slots.lock().unwrap_or_else(PoisonError::into_inner)
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.lock().len()
    }
}

/// Holds one key of [`Flights`].
pub(crate) struct FlightGuard<'a> {
    flights: &'a Flights,
    key: String,
    slot: Option<Arc<tokio::sync::Mutex<()>>>,
    guard: Option<OwnedMutexGuard<()>>,
}

impl Drop for FlightGuard<'_> {
    fn drop(&mut self) {
        // Release the mutex and our handle first; the map's own handle is then
        // the only one left unless another task is waiting (handles are only
        // cloned under the map lock, so the count cannot grow behind our back).
        drop(self.guard.take());
        drop(self.slot.take());
        let mut slots = self.flights.lock();
        if slots
            .get(&self.key)
            .is_some_and(|slot| Arc::strong_count(slot) == 1)
        {
            slots.remove(&self.key);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[tokio::test(start_paused = true)]
    async fn second_entrant_waits_for_the_first() {
        let flights = Arc::new(Flights::default());
        let first = flights.enter("k").await;

        let waiter = {
            let flights = flights.clone();
            tokio::spawn(async move {
                let _guard = flights.enter("k").await;
            })
        };
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert!(!waiter.is_finished(), "the key is held");

        drop(first);
        waiter.await.unwrap();
        assert_eq!(flights.len(), 0, "slots are removed once unused");
    }

    #[tokio::test]
    async fn different_keys_do_not_wait() {
        let flights = Flights::default();
        let _a = flights.enter("a").await;
        let _b = flights.enter("b").await;
        assert_eq!(flights.len(), 2);
    }
}
