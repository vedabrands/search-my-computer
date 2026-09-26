use crate::schema::job_kind;
use parking_lot::Mutex;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{debug, info};

pub const USER_IDLE_THRESHOLD: Duration = Duration::from_secs(60);

/// Abstraction for platform system status (power, battery, user idle, thread priority).
pub trait SystemStateProvider: Send + Sync {
    fn is_on_battery(&self) -> bool;
    fn is_battery_saver(&self) -> bool;
    fn get_idle_duration(&self) -> Duration;
    fn set_thread_priority_below_normal(&self);
}

#[cfg(windows)]
pub struct WindowsSystemStateProvider;

#[cfg(windows)]
impl SystemStateProvider for WindowsSystemStateProvider {
    fn is_on_battery(&self) -> bool {
        use windows_sys::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
        unsafe {
            let mut status = std::mem::zeroed::<SYSTEM_POWER_STATUS>();
            if GetSystemPowerStatus(&mut status) != 0 {
                // ACLineStatus: 0 = Offline (on battery), 1 = Online (AC), 255 = Unknown
                status.ACLineStatus == 0
            } else {
                false
            }
        }
    }

    fn is_battery_saver(&self) -> bool {
        use windows_sys::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
        unsafe {
            let mut status = std::mem::zeroed::<SYSTEM_POWER_STATUS>();
            if GetSystemPowerStatus(&mut status) != 0 {
                // SystemStatusFlag: 1 = Battery Saver active, 0 = Off
                status.SystemStatusFlag == 1
            } else {
                false
            }
        }
    }

    fn get_idle_duration(&self) -> Duration {
        use windows_sys::Win32::System::SystemInformation::GetTickCount;
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
        unsafe {
            let mut lii = LASTINPUTINFO {
                cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
                dwTime: 0,
            };
            if GetLastInputInfo(&mut lii) != 0 {
                let now = GetTickCount();
                let elapsed_ms = now.saturating_sub(lii.dwTime);
                Duration::from_millis(elapsed_ms as u64)
            } else {
                Duration::ZERO
            }
        }
    }

    fn set_thread_priority_below_normal(&self) {
        use windows_sys::Win32::System::Threading::{
            GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_BELOW_NORMAL,
        };
        unsafe {
            let thread = GetCurrentThread();
            let res = SetThreadPriority(thread, THREAD_PRIORITY_BELOW_NORMAL);
            if res == 0 {
                debug!("failed to set thread priority to below normal");
            }
        }
    }
}

/// Fallback provider for non-Windows platforms.
pub struct FallbackSystemStateProvider;

impl SystemStateProvider for FallbackSystemStateProvider {
    fn is_on_battery(&self) -> bool {
        false
    }

    fn is_battery_saver(&self) -> bool {
        false
    }

    fn get_idle_duration(&self) -> Duration {
        Duration::ZERO
    }

    fn set_thread_priority_below_normal(&self) {}
}

/// Deterministic mock system state provider for unit testing.
pub struct MockSystemState {
    pub on_battery: Mutex<bool>,
    pub battery_saver: Mutex<bool>,
    pub idle_duration: Mutex<Duration>,
    pub priority_changes: Mutex<usize>,
}

impl MockSystemState {
    pub fn new() -> Self {
        Self {
            on_battery: Mutex::new(false),
            battery_saver: Mutex::new(false),
            idle_duration: Mutex::new(Duration::ZERO),
            priority_changes: Mutex::new(0),
        }
    }

    pub fn set_on_battery(&self, value: bool) {
        *self.on_battery.lock() = value;
    }

    pub fn set_battery_saver(&self, value: bool) {
        *self.battery_saver.lock() = value;
    }

    pub fn set_idle_duration(&self, duration: Duration) {
        *self.idle_duration.lock() = duration;
    }

    pub fn priority_call_count(&self) -> usize {
        *self.priority_changes.lock()
    }
}

impl Default for MockSystemState {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemStateProvider for MockSystemState {
    fn is_on_battery(&self) -> bool {
        *self.on_battery.lock()
    }

    fn is_battery_saver(&self) -> bool {
        *self.battery_saver.lock()
    }

    fn get_idle_duration(&self) -> Duration {
        *self.idle_duration.lock()
    }

    fn set_thread_priority_below_normal(&self) {
        *self.priority_changes.lock() += 1;
    }
}

#[derive(Debug, Clone, PartialEq)]
enum PauseState {
    NotPaused,
    Until(Instant),
    Indefinite,
}

/// Resource Governor coordinating background indexing with laptop battery & user activity.
pub struct ResourceGovernor {
    provider: Arc<dyn SystemStateProvider>,
    pause_state: Mutex<PauseState>,
}

impl ResourceGovernor {
    pub fn new(provider: Arc<dyn SystemStateProvider>) -> Self {
        Self {
            provider,
            pause_state: Mutex::new(PauseState::NotPaused),
        }
    }

    /// Creates a default governor using the platform's native provider.
    pub fn new_default() -> Self {
        #[cfg(windows)]
        {
            Self::new(Arc::new(WindowsSystemStateProvider))
        }
        #[cfg(not(windows))]
        {
            Self::new(Arc::new(FallbackSystemStateProvider))
        }
    }

    pub fn provider(&self) -> &Arc<dyn SystemStateProvider> {
        &self.provider
    }

    /// Pause background indexing for a specified duration.
    pub fn pause_for(&self, duration: Duration) {
        let deadline = Instant::now() + duration;
        *self.pause_state.lock() = PauseState::Until(deadline);
        info!(
            duration_secs = duration.as_secs(),
            "background indexing paused by user"
        );
    }

    /// Pause background indexing until restart or explicit resume.
    pub fn pause_until_restart(&self) {
        *self.pause_state.lock() = PauseState::Indefinite;
        info!("background indexing paused until restart by user");
    }

    /// Resume background indexing.
    pub fn resume(&self) {
        *self.pause_state.lock() = PauseState::NotPaused;
        info!("background indexing resumed");
    }

    /// Returns true if background indexing is paused by user action.
    pub fn is_user_paused(&self) -> bool {
        let mut guard = self.pause_state.lock();
        match *guard {
            PauseState::NotPaused => false,
            PauseState::Until(deadline) => {
                if Instant::now() >= deadline {
                    *guard = PauseState::NotPaused;
                    false
                } else {
                    true
                }
            }
            PauseState::Indefinite => true,
        }
    }

    /// Returns remaining user pause duration, if any.
    pub fn remaining_user_pause(&self) -> Option<Duration> {
        let mut guard = self.pause_state.lock();
        match *guard {
            PauseState::NotPaused => None,
            PauseState::Until(deadline) => {
                let now = Instant::now();
                if now >= deadline {
                    *guard = PauseState::NotPaused;
                    None
                } else {
                    Some(deadline - now)
                }
            }
            PauseState::Indefinite => None,
        }
    }

    /// Calculates the maximum number of background worker threads allowed under current state.
    pub fn allowed_worker_threads(&self, configured_max: usize) -> usize {
        if self.is_user_paused() {
            return 0;
        }

        if self.provider.is_battery_saver() {
            return 0;
        }

        if self.provider.is_on_battery() {
            return 1;
        }

        // On AC power: check user idle state
        if self.provider.get_idle_duration() < USER_IDLE_THRESHOLD {
            1
        } else {
            configured_max.max(1)
        }
    }

    /// Determines if a specific background job kind is allowed to execute right now.
    pub fn is_job_allowed(&self, kind: &str) -> bool {
        if self.is_user_paused() || self.provider.is_battery_saver() {
            return false;
        }

        if self.provider.is_on_battery() {
            // On battery: only allow lightweight filename indexing and basic text extraction
            match kind {
                job_kind::INDEX_FILENAME | job_kind::EXTRACT => true,
                job_kind::EMBED | job_kind::VISION => false,
                _ => false,
            }
        } else {
            // On AC: all jobs allowed
            true
        }
    }

    /// Returns true if periodic SQLite maintenance (vacuum, checkpoint, FTS optimize) is allowed.
    pub fn is_maintenance_allowed(&self) -> bool {
        if self.is_user_paused()
            || self.provider.is_battery_saver()
            || self.provider.is_on_battery()
        {
            return false;
        }

        self.provider.get_idle_duration() >= USER_IDLE_THRESHOLD
    }

    /// Produces a human-readable status string for the tray/UI.
    pub fn status_label(&self, active_jobs_count: usize) -> String {
        if self.provider.is_battery_saver() {
            return "Paused: battery saver".to_string();
        }

        if let Some(remaining) = self.remaining_user_pause() {
            let mins = remaining.as_secs().div_ceil(60);
            return format!("Paused by you ({}m remaining)", mins);
        }

        let guard = self.pause_state.lock();
        if *guard == PauseState::Indefinite {
            return "Paused by you (until restart)".to_string();
        }
        drop(guard);

        if self.provider.is_on_battery() {
            if active_jobs_count > 0 {
                return format!("Indexing (on battery: {} jobs)", active_jobs_count);
            } else {
                return "Paused: on battery".to_string();
            }
        }

        if active_jobs_count > 0 {
            format!("Indexing {} files", active_jobs_count)
        } else {
            "Idle".to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_governor_policy_matrix() {
        let mock = Arc::new(MockSystemState::new());
        let governor = ResourceGovernor::new(mock.clone());

        // 1. Initial State: AC, Active user (< 60s idle)
        mock.set_on_battery(false);
        mock.set_battery_saver(false);
        mock.set_idle_duration(Duration::from_secs(10));

        assert_eq!(governor.allowed_worker_threads(4), 1);
        assert!(governor.is_job_allowed(job_kind::INDEX_FILENAME));
        assert!(governor.is_job_allowed(job_kind::EXTRACT));
        assert!(governor.is_job_allowed(job_kind::EMBED));
        assert!(governor.is_job_allowed(job_kind::VISION));
        assert!(!governor.is_maintenance_allowed());

        // 2. AC, Idle user (>= 60s idle)
        mock.set_idle_duration(Duration::from_secs(75));
        assert_eq!(governor.allowed_worker_threads(4), 4);
        assert!(governor.is_job_allowed(job_kind::INDEX_FILENAME));
        assert!(governor.is_job_allowed(job_kind::EMBED));
        assert!(governor.is_job_allowed(job_kind::VISION));
        assert!(governor.is_maintenance_allowed());

        // 3. Battery mode
        mock.set_on_battery(true);
        mock.set_idle_duration(Duration::from_secs(120));
        assert_eq!(governor.allowed_worker_threads(4), 1);
        assert!(governor.is_job_allowed(job_kind::INDEX_FILENAME));
        assert!(governor.is_job_allowed(job_kind::EXTRACT));
        assert!(!governor.is_job_allowed(job_kind::EMBED));
        assert!(!governor.is_job_allowed(job_kind::VISION));
        assert!(!governor.is_maintenance_allowed());

        // 4. Battery Saver mode
        mock.set_battery_saver(true);
        assert_eq!(governor.allowed_worker_threads(4), 0);
        assert!(!governor.is_job_allowed(job_kind::INDEX_FILENAME));
        assert!(!governor.is_job_allowed(job_kind::EXTRACT));
        assert!(!governor.is_job_allowed(job_kind::EMBED));
        assert!(!governor.is_job_allowed(job_kind::VISION));
        assert!(!governor.is_maintenance_allowed());
        assert_eq!(governor.status_label(5), "Paused: battery saver");

        // 5. User Pause
        mock.set_battery_saver(false);
        mock.set_on_battery(false);
        governor.pause_for(Duration::from_secs(900)); // 15 mins
        assert!(governor.is_user_paused());
        assert_eq!(governor.allowed_worker_threads(4), 0);
        assert!(!governor.is_job_allowed(job_kind::INDEX_FILENAME));
        assert!(governor.status_label(0).contains("Paused by you"));

        // 6. Resume
        governor.resume();
        assert!(!governor.is_user_paused());
        assert_eq!(governor.allowed_worker_threads(4), 4);
    }

    #[test]
    fn test_timed_pause_expiration() {
        let mock = Arc::new(MockSystemState::new());
        let governor = ResourceGovernor::new(mock);

        governor.pause_for(Duration::from_millis(10));
        assert!(governor.is_user_paused());

        std::thread::sleep(Duration::from_millis(25));
        assert!(!governor.is_user_paused());
    }

    #[test]
    fn test_priority_lowering_mock() {
        let mock = Arc::new(MockSystemState::new());
        mock.set_thread_priority_below_normal();
        assert_eq!(mock.priority_call_count(), 1);
    }
}
