//! Real-time ETA tracking and throughput calculation for world generation.
//!
//! Tracks tile/chunk/block generation rates using an exponential moving average (EMA)
//! to produce accurate, jitter-free ETA estimates in both CLI and GUI.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Real-time throughput and ETA calculator.
pub struct EtaTracker {
    stage_name: &'static str,
    total_items: usize,
    completed_items: AtomicUsize,
    start_time: Instant,
    last_update: Mutex<Instant>,
    last_completed: AtomicUsize,
    smoothed_rate: Mutex<f64>, // items per second
}

impl EtaTracker {
    /// Creates a new ETA tracker for a stage with a known item count.
    pub fn new(stage_name: &'static str, total_items: usize) -> Self {
        let now = Instant::now();
        Self {
            stage_name,
            total_items: total_items.max(1),
            completed_items: AtomicUsize::new(0),
            start_time: now,
            last_update: Mutex::new(now),
            last_completed: AtomicUsize::new(0),
            smoothed_rate: Mutex::new(0.0),
        }
    }

    /// Records completion of `count` items.
    pub fn add(&self, count: usize) {
        self.completed_items.fetch_add(count, Ordering::Relaxed);
    }

    /// Records completion of 1 item.
    pub fn inc(&self) {
        self.add(1);
    }

    /// Computes current completion, items/s rate, and estimated remaining duration.
    pub fn sample(&self) -> (usize, f64, Duration) {
        let completed = self.completed_items.load(Ordering::Relaxed).min(self.total_items);
        let now = Instant::now();

        let mut last_t = self.last_update.lock().unwrap();
        let dt = now.duration_since(*last_t).as_secs_f64();

        let mut rate_guard = self.smoothed_rate.lock().unwrap();

        if dt >= 0.1 {
            let prev_c = self.last_completed.swap(completed, Ordering::Relaxed);
            let dc = completed.saturating_sub(prev_c) as f64;
            let current_instant_rate = dc / dt;

            // Exponential moving average (alpha = 0.25)
            if *rate_guard <= 0.001 {
                *rate_guard = current_instant_rate;
            } else {
                *rate_guard = 0.25 * current_instant_rate + 0.75 * (*rate_guard);
            }
            *last_t = now;
        }

        // Fallback to average rate if EMA has not initialized
        let total_elapsed = now.duration_since(self.start_time).as_secs_f64();
        let effective_rate = if *rate_guard > 0.001 {
            *rate_guard
        } else if total_elapsed > 0.05 && completed > 0 {
            completed as f64 / total_elapsed
        } else {
            0.0
        };

        let remaining_items = self.total_items.saturating_sub(completed);
        let remaining_secs = if effective_rate > 0.001 {
            remaining_items as f64 / effective_rate
        } else {
            0.0
        };

        (completed, effective_rate, Duration::from_secs_f64(remaining_secs))
    }

    /// Percentage completed (0.0 to 100.0).
    pub fn percent(&self) -> f64 {
        let completed = self.completed_items.load(Ordering::Relaxed);
        (completed as f64 / self.total_items as f64 * 100.0).clamp(0.0, 100.0)
    }

    /// Total items.
    pub fn total(&self) -> usize {
        self.total_items
    }

    /// Formats a clean, human-readable ETA summary string.
    pub fn format_summary(&self) -> String {
        let (completed, rate, remaining) = self.sample();
        let secs = remaining.as_secs();
        let mins = secs / 60;
        let rem_secs = secs % 60;
        let hours = mins / 60;
        let rem_mins = mins % 60;

        let eta_str = if completed == 0 || rate <= 0.001 {
            "ETA: calculating...".to_string()
        } else if hours > 0 {
            format!("ETA: {:02}h {:02}m", hours, rem_mins)
        } else {
            format!("ETA: {:02}m {:02}s", rem_mins, rem_secs)
        };

        format!(
            "[{}/{} {} | {:.1}/s | {}]",
            completed, self.total_items, self.stage_name, rate, eta_str
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;

    #[test]
    fn test_eta_tracker_initial_state() {
        let tracker = EtaTracker::new("tiles", 100);
        assert_eq!(tracker.total_items, 100);
        let (done, rate, rem) = tracker.sample();
        assert_eq!(done, 0);
        assert_eq!(rate, 0.0);
        assert_eq!(rem, Duration::from_secs(0));
        assert!(tracker.format_summary().contains("calculating"));
    }

    #[test]
    fn test_eta_tracker_progress() {
        let tracker = EtaTracker::new("tiles", 100);
        sleep(Duration::from_millis(120));
        tracker.add(10);
        let (done, rate, _rem) = tracker.sample();
        assert_eq!(done, 10);
        assert!(rate > 0.0);
        let summary = tracker.format_summary();
        assert!(summary.contains("10/100"));
        assert!(summary.contains("tiles"));
    }
}
