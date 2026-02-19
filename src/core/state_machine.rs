use crate::core::models::Phase;

/// State machine transition event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    BootComplete,
    UserDisconnect,
    DisconnectTimerExpired,
    CalibrationDone,
    AuroraDetected,
    NoAuroraDetected,
    TimeRangeEnded,
    StorageCritical,
    SafeModeComplete,
}

/// Errors for invalid state transitions.
#[derive(Debug, thiserror::Error)]
pub enum StateMachineError {
    #[error("Invalid transition from {from} with event {event:?}")]
    InvalidTransition { from: Phase, event: Event },
}

/// Pure state machine: computes the next phase from (current phase, event).
/// No side effects — all I/O is handled by the orchestrator.
pub struct StateMachine {
    phase: Phase,
    consecutive_detections: u32,
    required_detections: u32,
}

impl StateMachine {
    pub fn new(required_detections: u32) -> Self {
        Self {
            phase: Phase::Boot,
            consecutive_detections: 0,
            required_detections,
        }
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    pub fn consecutive_detections(&self) -> u32 {
        self.consecutive_detections
    }

    /// Attempt a state transition. Returns the new phase or an error.
    pub fn transition(&mut self, event: Event) -> Result<Phase, StateMachineError> {
        let next = match (&self.phase, &event) {
            // Boot → Arm
            (Phase::Boot, Event::BootComplete) => Phase::Arm,

            // Arm → Disconnect
            (Phase::Arm, Event::UserDisconnect) => Phase::Disconnect,

            // Disconnect → Calibration
            (Phase::Disconnect, Event::DisconnectTimerExpired) => Phase::Calibration,

            // Calibration → Watch
            (Phase::Calibration, Event::CalibrationDone) => Phase::Watch,

            // Watch → Run (after N consecutive detections)
            (Phase::Watch, Event::AuroraDetected) => {
                self.consecutive_detections += 1;
                if self.consecutive_detections >= self.required_detections {
                    Phase::Run
                } else {
                    Phase::Watch
                }
            }

            // Watch: no aurora resets consecutive counter
            (Phase::Watch, Event::NoAuroraDetected) => {
                self.consecutive_detections = 0;
                Phase::Watch
            }

            // Watch → Shutdown (time range ended)
            (Phase::Watch, Event::TimeRangeEnded) => Phase::Shutdown,

            // Run → Shutdown (time range ended, NO return to Watch)
            (Phase::Run, Event::TimeRangeEnded) => Phase::Shutdown,

            // Any → SafeMode (storage critical)
            (_, Event::StorageCritical) => Phase::SafeMode,

            // SafeMode → Shutdown
            (Phase::SafeMode, Event::SafeModeComplete) => Phase::Shutdown,

            // Invalid transitions
            _ => {
                return Err(StateMachineError::InvalidTransition {
                    from: self.phase,
                    event,
                });
            }
        };

        // Reset detection counter when leaving Watch
        if self.phase == Phase::Watch && next != Phase::Watch {
            self.consecutive_detections = 0;
        }

        self.phase = next;
        Ok(next)
    }

    /// Force phase (used for testing or recovery).
    pub fn set_phase(&mut self, phase: Phase) {
        self.phase = phase;
        if phase != Phase::Watch {
            self.consecutive_detections = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normal_lifecycle() {
        let mut sm = StateMachine::new(2);
        assert_eq!(sm.phase(), Phase::Boot);

        assert_eq!(sm.transition(Event::BootComplete).unwrap(), Phase::Arm);
        assert_eq!(sm.transition(Event::UserDisconnect).unwrap(), Phase::Disconnect);
        assert_eq!(sm.transition(Event::DisconnectTimerExpired).unwrap(), Phase::Calibration);
        assert_eq!(sm.transition(Event::CalibrationDone).unwrap(), Phase::Watch);
    }

    #[test]
    fn test_aurora_detection_requires_consecutive() {
        let mut sm = StateMachine::new(2);
        sm.set_phase(Phase::Watch);

        // First detection: stay in Watch
        assert_eq!(sm.transition(Event::AuroraDetected).unwrap(), Phase::Watch);
        assert_eq!(sm.consecutive_detections(), 1);

        // Reset with no detection
        assert_eq!(sm.transition(Event::NoAuroraDetected).unwrap(), Phase::Watch);
        assert_eq!(sm.consecutive_detections(), 0);

        // Two consecutive: enter Run
        assert_eq!(sm.transition(Event::AuroraDetected).unwrap(), Phase::Watch);
        assert_eq!(sm.transition(Event::AuroraDetected).unwrap(), Phase::Run);
    }

    #[test]
    fn test_run_does_not_return_to_watch() {
        let mut sm = StateMachine::new(2);
        sm.set_phase(Phase::Run);

        // Only valid exit from Run is TimeRangeEnded or StorageCritical
        assert_eq!(sm.transition(Event::TimeRangeEnded).unwrap(), Phase::Shutdown);
    }

    #[test]
    fn test_storage_critical_from_any_phase() {
        for phase in [Phase::Boot, Phase::Arm, Phase::Watch, Phase::Run, Phase::Calibration] {
            let mut sm = StateMachine::new(2);
            sm.set_phase(phase);
            assert_eq!(sm.transition(Event::StorageCritical).unwrap(), Phase::SafeMode);
        }
    }

    #[test]
    fn test_safe_mode_to_shutdown() {
        let mut sm = StateMachine::new(2);
        sm.set_phase(Phase::SafeMode);
        assert_eq!(sm.transition(Event::SafeModeComplete).unwrap(), Phase::Shutdown);
    }

    #[test]
    fn test_invalid_transition() {
        let mut sm = StateMachine::new(2);
        // Can't go from Boot to Run
        assert!(sm.transition(Event::AuroraDetected).is_err());
    }

    #[test]
    fn test_watch_time_range_shutdown() {
        let mut sm = StateMachine::new(2);
        sm.set_phase(Phase::Watch);
        assert_eq!(sm.transition(Event::TimeRangeEnded).unwrap(), Phase::Shutdown);
    }
}
