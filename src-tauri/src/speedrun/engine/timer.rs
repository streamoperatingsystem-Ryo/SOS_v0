// =============================================================================
// Timer — Modèle de timer speedrun (port de TimerModel.cs + LiveSplitState.cs)
// -----------------------------------------------------------------------------
// Gère les phases du timer (NotRunning → Running → Ended), les splits,
// le game time (temps de jeu sans les loading times), et le temps réel.
//
// Équivalent C# :
//   - TimerPhase (enum) → TimerPhase (enum Rust)
//   - TimeStamp (static Now) → instant::Instant
//   - Time (RealTime + GameTime) → Time struct
//   - LiveSplitState → TimerState
//   - TimerModel → TimerModel
// =============================================================================
use std::time::Instant;

/// Phase du timer (équivalent de TimerPhase.cs).
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum TimerPhase {
    NotRunning,
    Running,
    Paused,
    Ended,
}

/// Temps écoulé (temps réel + temps de jeu optionnel).
#[derive(Clone, Debug, Default)]
pub struct Time {
    pub real_time: Option<f64>, // secondes
    pub game_time: Option<f64>, // secondes
}

impl Time {
    /// Retourne le game time s'il existe, sinon le real time.
    pub fn primary(&self) -> Option<f64> {
        self.game_time.or(self.real_time)
    }

    /// Formate en string HH:MM:SS.mmm (comme LiveSplit).
    pub fn format(&self) -> String {
        match self.primary() {
            Some(secs) => format_time(secs),
            None => "00:00.00".to_string(),
        }
    }
}

/// État du timer (équivalent de LiveSplitState.cs).
pub struct TimerState {
    pub current_phase: TimerPhase,
    pub current_split_index: i32,
    pub total_splits: i32,
    pub final_time: Option<f64>,

    // Timestamps internes
    start_time: Instant,
    adjusted_start: Instant,
    time_paused_at: f64,

    // Game time
    loading_times: Option<f64>, // Some = game time initialized
    game_time_pause: Option<f64>,
    is_game_time_paused: bool,
}

impl TimerState {
    pub fn new() -> Self {
        let now = Instant::now();
        Self {
            current_phase: TimerPhase::NotRunning,
            current_split_index: -1,
            total_splits: 0,
            final_time: None,
            start_time: now,
            adjusted_start: now,
            time_paused_at: 0.0,
            loading_times: None,
            game_time_pause: None,
            is_game_time_paused: false,
        }
    }

    /// Temps écoulé courant (real time + game time).
    pub fn current_time(&self) -> Time {
        let real_time = match self.current_phase {
            TimerPhase::NotRunning => Some(0.0),
            TimerPhase::Running => Some(elapsed_secs(self.adjusted_start)),
            TimerPhase::Paused => Some(self.time_paused_at),
            TimerPhase::Ended => self.final_time,
        };

        let game_time = if self.current_phase == TimerPhase::Ended {
            self.final_time
        } else if self.is_game_time_paused {
            self.game_time_pause
        } else if self.is_game_time_initialized() {
            real_time.map(|rt| rt - self.loading_times.unwrap_or(0.0))
        } else {
            None
        };

        Time {
            real_time,
            game_time,
        }
    }

    pub fn is_game_time_initialized(&self) -> bool {
        self.loading_times.is_some()
    }

    pub fn initialize_game_time(&mut self) {
        if self.loading_times.is_none() {
            self.loading_times = Some(0.0);
        }
    }

    pub fn set_game_time(&mut self, game_time: f64) {
        if let Some(rt) = self.current_time().real_time {
            self.loading_times = Some(rt - game_time);
            if self.is_game_time_paused {
                self.game_time_pause = Some(game_time);
            }
        }
    }

    pub fn set_is_game_time_paused(&mut self, paused: bool) {
        if !paused && self.is_game_time_paused {
            // Reprendre : calculer le loading times accumulé
            if let (Some(rt), Some(gt)) = (
                self.current_time().real_time,
                self.current_time().game_time,
            ) {
                self.loading_times = Some(rt - gt);
            }
        } else if paused && !self.is_game_time_paused {
            self.game_time_pause = self.current_time().game_time;
        }
        self.is_game_time_paused = paused;
    }

    pub fn loading_times(&self) -> f64 {
        self.loading_times.unwrap_or(0.0)
    }

    pub fn set_loading_times(&mut self, lt: f64) {
        self.loading_times = Some(lt);
    }
}

impl Default for TimerState {
    fn default() -> Self {
        Self::new()
    }
}

/// Modèle de timer avec actions (équivalent de TimerModel.cs).
pub struct TimerModel {
    pub state: TimerState,
}

impl TimerModel {
    pub fn new() -> Self {
        Self {
            state: TimerState::new(),
        }
    }

    pub fn start(&mut self) {
        if self.state.current_phase == TimerPhase::NotRunning {
            let now = Instant::now();
            self.state.current_phase = TimerPhase::Running;
            self.state.current_split_index = 0;
            self.state.start_time = now;
            self.state.adjusted_start = now;
            self.state.time_paused_at = 0.0;
            self.state.loading_times = None;
            self.state.final_time = None;
        }
    }

    pub fn split(&mut self) {
        if self.state.current_phase == TimerPhase::Running {
            let ct = self.state.current_time();
            if ct.real_time.map(|t| t > 0.0).unwrap_or(false) {
                self.state.current_split_index += 1;
                if self.state.total_splits > 0
                    && self.state.total_splits == self.state.current_split_index
                {
                    // Run terminée
                    self.state.final_time = ct.game_time.or(ct.real_time);
                    self.state.current_phase = TimerPhase::Ended;
                }
            }
        }
    }

    pub fn reset(&mut self) {
        if self.state.current_phase != TimerPhase::NotRunning {
            self.state.current_phase = TimerPhase::NotRunning;
            self.state.current_split_index = -1;
            self.state.final_time = None;
            self.state.is_game_time_paused = false;
            self.state.loading_times = None;
            self.state.game_time_pause = None;
        }
    }

    pub fn pause(&mut self) {
        match self.state.current_phase {
            TimerPhase::Running => {
                self.state.time_paused_at = elapsed_secs(self.state.adjusted_start);
                self.state.current_phase = TimerPhase::Paused;
            }
            TimerPhase::Paused => {
                // Resume
                self.state.adjusted_start =
                    Instant::now() - std::time::Duration::from_secs_f64(self.state.time_paused_at);
                self.state.current_phase = TimerPhase::Running;
            }
            TimerPhase::NotRunning => {
                self.start();
            }
            _ => {}
        }
    }

    pub fn undo_split(&mut self) {
        if self.state.current_phase != TimerPhase::NotRunning && self.state.current_split_index > 0
        {
            if self.state.current_phase == TimerPhase::Ended {
                self.state.current_phase = TimerPhase::Running;
                self.state.final_time = None;
            }
            self.state.current_split_index -= 1;
        }
    }

    pub fn skip_split(&mut self) {
        if (self.state.current_phase == TimerPhase::Running
            || self.state.current_phase == TimerPhase::Paused)
            && (self.state.total_splits == 0
                || self.state.current_split_index < self.state.total_splits - 1)
        {
            self.state.current_split_index += 1;
        }
    }
}

impl Default for TimerModel {
    fn default() -> Self {
        Self::new()
    }
}

/// Calcule le temps écoulé en secondes depuis un Instant.
fn elapsed_secs(start: Instant) -> f64 {
    start.elapsed().as_secs_f64()
}

/// Formate un temps en secondes vers HH:MM:SS.mmm.
pub fn format_time(secs: f64) -> String {
    let total_ms = (secs * 1000.0).round() as u64;
    let hours = total_ms / 3_600_000;
    let minutes = (total_ms % 3_600_000) / 60_000;
    let seconds = (total_ms % 60_000) / 1000;
    let ms = total_ms % 1000;

    if hours > 0 {
        format!("{}:{:02}:{:02}.{:03}", hours, minutes, seconds, ms)
    } else {
        format!("{:02}:{:02}.{:03}", minutes, seconds, ms)
    }
}
