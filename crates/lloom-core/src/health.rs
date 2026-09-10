//! Health-aware model state machine (ROUTING-PLAN §P3).
//!
//! Passive feedback from `ai_client::chat` failures/timeouts/429 feeds a per-model
//! sliding window; the machine derives a health_state that `router::plan()` uses as a
//! hard gate (`down` is excluded from the candidate set). Active probing (from
//! `server.rs` background loop) drives `down → up` recovery.
//!
//! Transitions (plan):
//!   - `unknown → up` on first success
//!   - `up → degraded` when ≥2 failures within the sliding window (default last 5)
//!   - `degraded → down` on 3 consecutive failures
//!   - `down → up` only when active probe succeeds
//!   - circuit breaker: consecutive failures ≥ `health.circuit_threshold` (default 5)
//!     force `down` regardless of window
//!
//! State is only written to the DB when it *changes*, so the hot path does not
//! hammer `UPDATEs` on every request.

use std::collections::HashMap;
use std::collections::VecDeque;
use std::sync::{Mutex, OnceLock};

/// Per-model health window: recent outcomes + running consecutive-failure counter.
#[derive(Debug, Clone)]
pub struct ModelHealth {
    state: String,
    window: VecDeque<bool>, // true=ok
    consecutive_fail: u32,
}

impl ModelHealth {
    fn new() -> Self {
        Self {
            state: "unknown".to_string(),
            window: VecDeque::new(),
            consecutive_fail: 0,
        }
    }
}

static HEALTH: OnceLock<Mutex<HashMap<String, ModelHealth>>> = OnceLock::new();

fn health_map() -> &'static Mutex<HashMap<String, ModelHealth>> {
    HEALTH.get_or_init(|| Mutex::new(HashMap::new()))
}

fn window_size(db: &crate::db::Db) -> usize {
    crate::config::health_fail_window(db).max(1) as usize
}
fn degraded_fails(db: &crate::db::Db) -> u32 {
    crate::config::health_degraded_fails(db).max(1)
}
fn down_consecutive(db: &crate::db::Db) -> u32 {
    crate::config::health_down_consecutive(db).max(1)
}
fn circuit_threshold(db: &crate::db::Db) -> u32 {
    crate::config::health_circuit_threshold(db).max(1)
}

/// Record one LLM call outcome for `name`. Returns the resulting state (after
/// persisted transition). Never panics; DB write failures are swallowed.
pub fn record_outcome(db: &crate::db::Db, name: &str, ok: bool) -> String {
    let mut map = health_map().lock().unwrap_or_else(|p| p.into_inner());
    let h = map.entry(name.to_string()).or_insert_with(ModelHealth::new);

    let w = window_size(db);
    let d_fails = degraded_fails(db);
    let d_cons = down_consecutive(db);
    let c_thresh = circuit_threshold(db);

    if ok {
        h.consecutive_fail = 0;
        h.window.push_back(true);
        while h.window.len() > w {
            h.window.pop_front();
        }
        // 成功永远向 up 收敛：unknown→up、degraded→up、down→up（主动探测/降级恢复均可）
        persist_transition(db, name, &mut h.state, "up");
        return h.state.clone();
    }

    // failure
    h.consecutive_fail += 1;
    h.window.push_back(false);
    while h.window.len() > w {
        h.window.pop_front();
    }
    let win_fails = h.window.iter().filter(|&&b| !b).count() as u32;
    let next = if h.consecutive_fail >= c_thresh || h.consecutive_fail >= d_cons {
        "down"
    } else if win_fails >= d_fails {
        "degraded"
    } else {
        "up"
    };
    persist_transition(db, name, &mut h.state, next);
    h.state.clone()
}

/// Persist a state change (`state` → `next`) to the DB only when it differs.
fn persist_transition(db: &crate::db::Db, name: &str, state: &mut String, next: &str) {
    if state == next {
        return;
    }
    let _ = db.set_model_health(name, next);
    *state = next.to_string();
}

/// Current in-memory state for `name` (may diverge from DB if no call yet); `unknown` default.
pub fn current_state(name: &str) -> String {
    let map = health_map().lock().unwrap_or_else(|p| p.into_inner());
    map.get(name)
        .map(|h| h.state.clone())
        .unwrap_or_else(|| "unknown".to_string())
}

/// Reset in-memory window for `name` back to a concrete state (used when seeding
/// from DB, e.g. models already flagged `down` at startup). Avoids re-degrading.
pub fn seed_state(db: &crate::db::Db, name: &str, state: &str) {
    let mut map = health_map().lock().unwrap_or_else(|p| p.into_inner());
    let h = map.entry(name.to_string()).or_insert_with(ModelHealth::new);
    h.state = state.to_string();
    h.consecutive_fail = if state == "down" {
        down_consecutive(db)
    } else {
        0
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_db() -> &'static crate::db::Db {
        static DB: OnceLock<crate::db::Db> = OnceLock::new();
        DB.get_or_init(|| {
            let dir =
                std::env::temp_dir().join(format!("lloom_health_test_{}", std::process::id()));
            let _ = std::fs::create_dir_all(&dir);
            crate::db::Db::new(dir.join("health_test.db")).expect("test db")
        })
    }

    // 每个用例用独立模型名，避免并行共享全局 HEALTH map 相互干扰。

    #[test]
    fn first_success_goes_up() {
        assert_eq!(record_outcome(test_db(), "t_success", true), "up");
    }

    #[test]
    fn first_failure_stays_up() {
        // 单次失败不降级（滑窗 1/5 < 2）
        record_outcome(test_db(), "t_single_fail", false);
        assert_eq!(current_state("t_single_fail"), "up");
    }

    #[test]
    fn two_failures_in_window_degrade() {
        record_outcome(test_db(), "t_degrade", true); // up
        record_outcome(test_db(), "t_degrade", false);
        assert_eq!(current_state("t_degrade"), "up");
        record_outcome(test_db(), "t_degrade", false); // 2/5 in window → degraded
        assert_eq!(current_state("t_degrade"), "degraded");
    }

    #[test]
    fn three_consecutive_failures_down() {
        record_outcome(test_db(), "t_down3", true);
        record_outcome(test_db(), "t_down3", false);
        record_outcome(test_db(), "t_down3", false);
        assert_eq!(current_state("t_down3"), "degraded");
        record_outcome(test_db(), "t_down3", false); // 连续 3 → down
        assert_eq!(current_state("t_down3"), "down");
    }

    #[test]
    fn recovery_after_success() {
        // degraded → up
        record_outcome(test_db(), "t_recover", false);
        record_outcome(test_db(), "t_recover", false);
        assert_eq!(current_state("t_recover"), "degraded");
        record_outcome(test_db(), "t_recover", true);
        assert_eq!(current_state("t_recover"), "up");
    }

    #[test]
    fn down_persists_until_success() {
        record_outcome(test_db(), "t_down_persist", true);
        record_outcome(test_db(), "t_down_persist", false);
        record_outcome(test_db(), "t_down_persist", false);
        record_outcome(test_db(), "t_down_persist", false); // down
        assert_eq!(current_state("t_down_persist"), "down");
        // 继续失败仍 down
        record_outcome(test_db(), "t_down_persist", false);
        assert_eq!(current_state("t_down_persist"), "down");
        // 成功恢复 → up
        record_outcome(test_db(), "t_down_persist", true);
        assert_eq!(current_state("t_down_persist"), "up");
    }

    #[test]
    fn seed_state_seeds_down() {
        seed_state(test_db(), "t_seed", "down");
        assert_eq!(current_state("t_seed"), "down");
        // 已 down 的模型即使恢复也需成功探测
        record_outcome(test_db(), "t_seed", true);
        assert_eq!(current_state("t_seed"), "up");
    }
}
