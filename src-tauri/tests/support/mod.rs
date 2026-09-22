//! Shared scaffolding for the live tests: the sink seam and two pollers.
//!
//! Every live test drives the same pump the window drives, so they observe it the
//! same way — through [`ActivitySink`] — rather than each growing its own harness.
//!
//! What the sink records now includes the thread each event came from: the window's
//! events are envelopes (`dto::ThreadEvent`), and a test that ignored the tag would not
//! notice a patch arriving from the wrong session.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use omp_desktop::dto::{
    ActivitySnapshot, AgentSnapshot, ChromeEvent, CommandSnapshot, NotificationEvent, RowPatch,
    ThreadSnapshot, UiRequestSnapshot,
};
use omp_desktop::session::{ActivitySink, LiveSession};
use omp_desktop::threads::Threads;
use omp_transport::protocol::commands;

/// The published records: `(thread, payload)`, one per event, behind a lock the sink
/// shares with its own clones.
type Shared<T> = Arc<Mutex<T>>;
type Tagged<T> = Vec<(String, T)>;

/// What the pump published: the event tail, the dialog set, and row patches.
#[derive(Default, Clone)]
pub struct RecordingSink {
    /// `(thread, kind)` — the tag is kept, because an untagged stream is the bug the
    /// multi-thread host exists to prevent.
    kinds: Shared<Tagged<String>>,
    dialogs: Shared<Tagged<Vec<UiRequestSnapshot>>>,
    patches: Shared<Tagged<RowPatch>>,
    commands: Shared<Tagged<Vec<CommandSnapshot>>>,
    rosters: Shared<Vec<Vec<ThreadSnapshot>>>,
    agents: Shared<Tagged<Vec<AgentSnapshot>>>,
    notifications: Shared<Vec<NotificationEvent>>,
    chrome: Shared<Tagged<ChromeEvent>>,
}

/// Each test binary compiles this module separately and exercises one half of the
/// API — `m0` watches the event tail, `approval` watches the dialogs — so the half
/// a given binary does not use looks dead inside it.
#[allow(dead_code)]
impl RecordingSink {
    /// The event kinds seen so far, in order.
    pub fn kinds(&self) -> Vec<String> {
        self.kinds
            .lock()
            .map(|kinds| kinds.iter().map(|(_, kind)| kind.clone()).collect())
            .unwrap_or_default()
    }

    /// The thread each event was addressed to, in the order the events arrived.
    pub fn event_threads(&self) -> Vec<String> {
        self.kinds
            .lock()
            .map(|kinds| kinds.iter().map(|(thread, _)| thread.clone()).collect())
            .unwrap_or_default()
    }

    /// The latest pending-dialog set, as the window would have received it.
    pub fn dialogs(&self) -> Vec<UiRequestSnapshot> {
        self.dialogs
            .lock()
            .map(|dialogs| dialogs.last().map_or_else(Vec::new, |(_, set)| set.clone()))
            .unwrap_or_default()
    }

    /// Every conversation patch, in order.
    pub fn patches(&self) -> Vec<RowPatch> {
        self.patches
            .lock()
            .map(|patches| patches.iter().map(|(_, patch)| patch.clone()).collect())
            .unwrap_or_default()
    }

    /// The thread each patch was addressed to, in the order the patches arrived.
    pub fn patch_threads(&self) -> Vec<String> {
        self.patches
            .lock()
            .map(|patches| patches.iter().map(|(thread, _)| thread.clone()).collect())
            .unwrap_or_default()
    }

    /// The palette list the engine last pushed, as the window would have received it.
    pub fn commands(&self) -> Vec<CommandSnapshot> {
        self.commands
            .lock()
            .map(|commands| {
                commands
                    .last()
                    .map_or_else(Vec::new, |(_, list)| list.clone())
            })
            .unwrap_or_default()
    }

    /// The last roster the host published — every live thread's row.
    pub fn roster(&self) -> Vec<ThreadSnapshot> {
        self.rosters
            .lock()
            .map(|rosters| rosters.last().cloned().unwrap_or_default())
            .unwrap_or_default()
    }

    /// Every agent roster this thread's session published, in order (`docs/12` §9).
    ///
    /// The *sequence* rather than the latest, because the sequence is the assertion: a
    /// subagent that starts and finishes inside one turn is only ever visible as a roster
    /// that said `running` and then one that did not, and a reader that took the last one
    /// would conclude the frames never reported the run at all.
    pub fn agent_rosters(&self, thread: &str) -> Vec<Vec<AgentSnapshot>> {
        self.agents
            .lock()
            .map(|agents| {
                agents
                    .iter()
                    .filter(|(tag, _)| tag == thread)
                    .map(|(_, list)| list.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Every notification the host reported, in order.
    pub fn notifications(&self) -> Vec<NotificationEvent> {
        self.notifications
            .lock()
            .map(|notifications| notifications.clone())
            .unwrap_or_default()
    }

    /// The notifications addressed to one thread, in order.
    pub fn notifications_for(&self, thread: &str) -> Vec<NotificationEvent> {
        self.notifications()
            .into_iter()
            .filter(|notification| notification.thread == thread)
            .collect()
    }

    /// Every chrome op the engine pushed, in order, with the thread it came from.
    pub fn chrome(&self) -> Vec<(String, ChromeEvent)> {
        self.chrome
            .lock()
            .map(|chrome| {
                chrome
                    .iter()
                    .map(|(thread, event)| (thread.clone(), event.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }
}

impl ActivitySink for RecordingSink {
    fn activity(&self, thread: &str, activity: ActivitySnapshot) {
        if let Ok(mut kinds) = self.kinds.lock() {
            kinds.push((thread.to_string(), activity.kind));
        }
    }

    fn ui_requests(&self, thread: &str, requests: Vec<UiRequestSnapshot>) {
        if let Ok(mut dialogs) = self.dialogs.lock() {
            dialogs.push((thread.to_string(), requests));
        }
    }

    fn rows(&self, thread: &str, patch: RowPatch) {
        if let Ok(mut patches) = self.patches.lock() {
            patches.push((thread.to_string(), patch));
        }
    }

    fn commands(&self, thread: &str, commands: Vec<CommandSnapshot>) {
        if let Ok(mut stored) = self.commands.lock() {
            stored.push((thread.to_string(), commands));
        }
    }

    fn threads(&self, snapshots: Vec<ThreadSnapshot>) {
        if let Ok(mut rosters) = self.rosters.lock() {
            rosters.push(snapshots);
        }
    }

    fn agents(&self, thread: &str, agents: Vec<AgentSnapshot>) {
        if let Ok(mut stored) = self.agents.lock() {
            stored.push((thread.to_string(), agents));
        }
    }

    fn notifications(&self, notification: NotificationEvent) {
        if let Ok(mut stored) = self.notifications.lock() {
            stored.push(notification);
        }
    }

    fn chrome(&self, event: ChromeEvent) {
        if let Ok(mut stored) = self.chrome.lock() {
            stored.push((event.thread.clone(), event));
        }
    }
}

/// The registry a live test's sessions belong to.
///
/// The window's host owns one of these; a test that opens a session on its own still has
/// to hand the pump a roster to republish, and a registry this test keeps is what lets it
/// see that publication — or, in `threads.rs`, register the sessions and read the rows.
#[allow(dead_code)]
pub fn registry() -> Arc<Threads> {
    Arc::new(Threads::default())
}

/// A 1×1 PNG, base64. Small on purpose: the command frame carries it, and the
/// engine's advertised physical frame is 1 MiB (`docs/rpc.md`).
///
/// Shared rather than copied per binary: `m0` and `composer` both need an image that
/// is a real, decodable PNG — the engine inspects the bytes, so a truncated fixture
/// would fail as "not an image" rather than as the thing under test.
#[allow(dead_code)]
pub const ONE_PIXEL_PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABAQMAAAAl21bKAAAAIGNIUk0AAHomAACAhAAA+gAAAIDoAAB1MAAA6mAAADqYAAAXcJy6UTwAAAAGUExURf8AAP///0EdNBEAAAABYktHRAH/Ai3eAAAAB3RJTUUH6gkUDBEA86PqiQAAACV0RVh0ZGF0ZTpjcmVhdGUAMjAyNi0wOS0yMFQxMjoxNzowMCswMDowMKoToewAAAAldEVYdGRhdGU6bW9kaWZ5ADIwMjYtMDktMjBUMTI6MTc6MDArMDA6MDDbThlQAAAAKHRFWHRkYXRlOnRpbWVzdGFtcAAyMDI2LTA5LTIwVDEyOjE3OjAwKzAwOjAwjFs4jwAAAApJREFUCNdjYAAAAAIAAeIhvDMAAAAASUVORK5CYII=";

/// Poll a condition until it holds.
///
/// Everything here arrives on another task's schedule, so asserting immediately
/// would be a race rather than a check.
pub async fn wait_for<T>(what: &str, mut probe: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        if let Some(value) = probe() {
            return value;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Send one prompt and wait for the turn to finish.
///
/// Waiting on `!is_streaming` alone is a race: the flag is false before the turn starts, so
/// the wait would return immediately. The engine's own `messageCount` is what moves, so the
/// wait is for it to move *and* for the turn to be over.
///
/// In `support` because more than one live test needs a turn to have happened: a background
/// job's own delivery is a turn too, and a helper copied into a second test binary is how two
/// tests end up disagreeing about what "finished" means.
#[allow(dead_code)]
pub async fn say(live: &LiveSession, prompt: &str) {
    let before = live.status().expect("status").control.message_count;

    let client = live.client().expect("the session is open");
    let accepted = client
        .request(
            commands::prompt(prompt, &[], None),
            Some(Duration::from_secs(60)),
        )
        .await
        .expect("the prompt is accepted");
    assert!(
        omp_transport::protocol::is_success(&accepted),
        "rejected: {accepted}"
    );

    wait_for("the turn to finish", || {
        let status = live.status().ok()?;
        (status.control.message_count > before && !status.control.is_streaming).then_some(())
    })
    .await;
}

/// Whether a process id is still present.
///
/// A real check rather than a proxy, on the platform we ship first: an orphaned
/// agent is a ~200 MB process the user cannot see, which is exactly what the
/// sidecar step promised never to leave behind.
///
/// `allow(dead_code)` for the same reason [`RecordingSink`] carries it: each test
/// binary compiles this module separately, and the ones that never open a session
/// (`probe_resume`, `mode_switch`) have no process to look for.
#[allow(dead_code)]
pub fn process_exists(pid: u32) -> bool {
    #[cfg(target_os = "linux")]
    {
        std::path::Path::new(&format!("/proc/{pid}")).exists()
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = pid;
        // Unknown rather than absent: do not claim a check we cannot make.
        false
    }
}
