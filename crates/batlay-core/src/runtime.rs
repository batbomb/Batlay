//! Temps, minuteurs et tâches de fond, injectables.
//!
//! En production : vraie horloge, un fil dédié aux minuteurs, un fil par tâche.
//! En test : horloge manuelle, minuteurs déclenchés à la main, tâches exécutées
//! sur-le-champ : chaque scénario (attente du live, reconnexion…) se rejoue
//! de façon déterministe, sans `sleep`.

use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

pub type Task = Box<dyn FnOnce() + Send + 'static>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TimerId(pub u64);

/// Horloge en ms depuis l'époque Unix.
pub trait Clock: Send + Sync {
    fn now(&self) -> u64;
}

pub struct SystemClock;
impl Clock for SystemClock {
    fn now(&self) -> u64 {
        crate::util::now_ms()
    }
}

/// Horloge de test.
#[derive(Default)]
pub struct ManualClock(AtomicU64);
impl ManualClock {
    pub fn new(start: u64) -> Arc<ManualClock> {
        Arc::new(ManualClock(AtomicU64::new(start)))
    }
    pub fn set(&self, t: u64) {
        self.0.store(t, Ordering::SeqCst)
    }
    pub fn advance(&self, ms: u64) {
        self.0.fetch_add(ms, Ordering::SeqCst);
    }
}
impl Clock for ManualClock {
    fn now(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
}

/// Minuteurs : `after` programme une tâche, `cancel` l'annule (sans effet si déjà partie).
pub trait Timers: Send + Sync {
    fn after(&self, delay_ms: u64, task: Task) -> TimerId;
    fn cancel(&self, id: TimerId);
}

/// Tâches de fond (requêtes réseau bloquantes…).
pub trait Spawner: Send + Sync {
    fn spawn(&self, name: &str, task: Task);
}

pub struct ThreadSpawner;
impl Spawner for ThreadSpawner {
    fn spawn(&self, name: &str, task: Task) {
        let _ = thread::Builder::new().name(name.to_string()).spawn(task);
    }
}

/// Exécute la tâche immédiatement (tests).
pub struct InlineSpawner;
impl Spawner for InlineSpawner {
    fn spawn(&self, _name: &str, task: Task) {
        task()
    }
}

// -- Minuteurs sur un fil dédié ----------------------------------------------------

struct TimerState {
    queue: BTreeMap<(Instant, u64), Task>,
    deadlines: HashMap<u64, Instant>,
    shutdown: bool,
}

/// Un seul fil pour tous les minuteurs de Batlay ; chaque tâche échue est
/// lancée sur ce fil (elle doit être courte : les tâches longues passent par un `Spawner`).
pub struct ThreadTimers {
    state: Arc<(Mutex<TimerState>, Condvar)>,
    next: AtomicU64,
}

impl ThreadTimers {
    pub fn new() -> Arc<ThreadTimers> {
        let state = Arc::new((Mutex::new(TimerState { queue: BTreeMap::new(), deadlines: HashMap::new(), shutdown: false }), Condvar::new()));
        let worker = state.clone();
        thread::Builder::new()
            .name("batlay-timers".into())
            .spawn(move || {
                let (lock, cvar) = &*worker;
                let mut st = lock.lock().unwrap_or_else(|e| e.into_inner());
                loop {
                    if st.shutdown {
                        return;
                    }
                    let now = Instant::now();
                    match st.queue.keys().next().copied() {
                        Some(key) if key.0 <= now => {
                            let task = st.queue.remove(&key).expect("clé présente");
                            st.deadlines.remove(&key.1);
                            drop(st);
                            // Une tâche en panne ne doit pas arrêter tous les minuteurs de l'application.
                            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(task));
                            st = lock.lock().unwrap_or_else(|e| e.into_inner());
                        }
                        Some(key) => {
                            st = cvar.wait_timeout(st, key.0 - now).unwrap_or_else(|e| e.into_inner()).0;
                        }
                        None => {
                            st = cvar.wait(st).unwrap_or_else(|e| e.into_inner());
                        }
                    }
                }
            })
            .expect("fil des minuteurs");
        Arc::new(ThreadTimers { state, next: AtomicU64::new(1) })
    }

    pub fn shutdown(&self) {
        let (lock, cvar) = &*self.state;
        lock.lock().unwrap_or_else(|e| e.into_inner()).shutdown = true;
        cvar.notify_all();
    }
}

impl Timers for ThreadTimers {
    fn after(&self, delay_ms: u64, task: Task) -> TimerId {
        let id = self.next.fetch_add(1, Ordering::SeqCst);
        let at = Instant::now() + Duration::from_millis(delay_ms);
        let (lock, cvar) = &*self.state;
        let mut st = lock.lock().unwrap_or_else(|e| e.into_inner());
        st.queue.insert((at, id), task);
        st.deadlines.insert(id, at);
        cvar.notify_all();
        TimerId(id)
    }
    fn cancel(&self, id: TimerId) {
        let (lock, _) = &*self.state;
        let mut st = lock.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(at) = st.deadlines.remove(&id.0) {
            st.queue.remove(&(at, id.0));
        }
    }
}

// -- Minuteurs de test ---------------------------------------------------------------

/// Minuteurs déclenchés à la main : `fire_next`, `fire_all_due`, `pending`.
#[derive(Default)]
pub struct ManualTimers {
    inner: Mutex<(u64, Vec<(TimerId, u64, Option<Task>)>)>,
}

impl ManualTimers {
    pub fn new() -> Arc<ManualTimers> {
        Arc::new(ManualTimers::default())
    }
    /// Délais (ms) des minuteurs encore programmés, dans l'ordre de création.
    pub fn pending(&self) -> Vec<u64> {
        self.inner.lock().unwrap().1.iter().map(|(_, d, _)| *d).collect()
    }
    pub fn count(&self) -> usize {
        self.inner.lock().unwrap().1.len()
    }
    /// Déclenche le dernier minuteur programmé. Faux s'il n'y en a pas.
    pub fn fire_last(&self) -> bool {
        let task = {
            let mut inner = self.inner.lock().unwrap();
            match inner.1.pop() {
                Some((_, _, task)) => task,
                None => return false,
            }
        };
        if let Some(task) = task {
            task();
        }
        true
    }
    /// Déclenche le minuteur le plus ancien.
    pub fn fire_first(&self) -> bool {
        let task = {
            let mut inner = self.inner.lock().unwrap();
            if inner.1.is_empty() {
                return false;
            }
            inner.1.remove(0).2
        };
        if let Some(task) = task {
            task();
        }
        true
    }
    /// Déclenche le premier minuteur programmé avec ce délai.
    pub fn fire_delay(&self, delay: u64) -> bool {
        let task = {
            let mut inner = self.inner.lock().unwrap();
            match inner.1.iter().position(|(_, d, _)| *d == delay) {
                Some(i) => inner.1.remove(i).2,
                None => return false,
            }
        };
        if let Some(task) = task {
            task();
        }
        true
    }
}

impl Timers for ManualTimers {
    fn after(&self, delay_ms: u64, task: Task) -> TimerId {
        let mut inner = self.inner.lock().unwrap();
        inner.0 += 1;
        let id = TimerId(inner.0);
        inner.1.push((id, delay_ms, Some(task)));
        id
    }
    fn cancel(&self, id: TimerId) {
        self.inner.lock().unwrap().1.retain(|(i, _, _)| *i != id);
    }
}

/// Le minimum dont un service a besoin pour vivre dans le temps.
#[derive(Clone)]
pub struct Rt {
    pub clock: Arc<dyn Clock>,
    pub timers: Arc<dyn Timers>,
    pub spawner: Arc<dyn Spawner>,
}

impl Rt {
    pub fn production() -> Rt {
        Rt { clock: Arc::new(SystemClock), timers: ThreadTimers::new(), spawner: Arc::new(ThreadSpawner) }
    }
    pub fn now(&self) -> u64 {
        self.clock.now()
    }
    pub fn after(&self, delay_ms: u64, task: impl FnOnce() + Send + 'static) -> TimerId {
        self.timers.after(delay_ms, Box::new(task))
    }
    pub fn cancel(&self, id: Option<TimerId>) {
        if let Some(id) = id {
            self.timers.cancel(id);
        }
    }
    pub fn spawn(&self, name: &str, task: impl FnOnce() + Send + 'static) {
        self.spawner.spawn(name, Box::new(task));
    }
}

/// Environnement de test : horloge manuelle, minuteurs manuels, tâches immédiates.
pub fn test_rt(start: u64) -> (Rt, Arc<ManualClock>, Arc<ManualTimers>) {
    let clock = ManualClock::new(start);
    let timers = ManualTimers::new();
    (Rt { clock: clock.clone(), timers: timers.clone(), spawner: Arc::new(InlineSpawner) }, clock, timers)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn thread_timers_fire_in_order_and_cancel() {
        let timers = ThreadTimers::new();
        let (tx, rx) = mpsc::channel();
        let t1 = tx.clone();
        timers.after(60, Box::new(move || t1.send(2).unwrap()));
        let t2 = tx.clone();
        timers.after(10, Box::new(move || t2.send(1).unwrap()));
        let t3 = tx.clone();
        let cancelled = timers.after(30, Box::new(move || t3.send(99).unwrap()));
        timers.cancel(cancelled);
        timers.after(5, Box::new(|| panic!("tâche en panne : les autres continuent")));
        assert_eq!(rx.recv_timeout(Duration::from_secs(2)).unwrap(), 1);
        assert_eq!(rx.recv_timeout(Duration::from_secs(2)).unwrap(), 2);
        assert!(rx.recv_timeout(Duration::from_millis(100)).is_err());
        timers.shutdown();
    }

    #[test]
    fn manual_timers() {
        let (rt, clock, timers) = test_rt(1000);
        let hits = Arc::new(Mutex::new(Vec::new()));
        let h = hits.clone();
        rt.after(90_000, move || h.lock().unwrap().push("a"));
        let h = hits.clone();
        let id = rt.after(5, move || h.lock().unwrap().push("b"));
        assert_eq!(timers.pending(), vec![90_000, 5]);
        rt.cancel(Some(id));
        assert!(timers.fire_delay(90_000));
        assert!(!timers.fire_last());
        clock.advance(10);
        assert_eq!((rt.now(), hits.lock().unwrap().clone()), (1010, vec!["a"]));
    }
}
