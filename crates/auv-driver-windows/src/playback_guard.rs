//! Step 2 Idempotency & Command Counting Guard for QQ Music and SMTC playback.
//!
//! Provides guarded write execution for Step 2 ("Ensure Playing & Volume 40%"):
//! - Reads pre-state (current volume, current playback status).
//! - Evaluates a [`Step2Plan`] with volume tolerance (±0.05) and play status gating.
//! - Dispatches writes ONLY when necessary (`SetMasterVolume` and `Play`).
//! - Records exact invocation counts ([`Step2CommandCounts`]).
//! - Verifies ONLY actually changed fields, eliminating redundant polling loops.

use std::time::{Duration, Instant};

use auv_driver_common::error::DriverResult;
use serde::{Deserialize, Serialize};

use crate::media::{MediaPlaybackStatus, ProcessAudioVolume, SmtcSession};

/// Default target playback volume (40%).
pub const DEFAULT_TARGET_VOLUME: f32 = 0.40;

/// Default volume tolerance (±0.05), treating [0.35, 0.45] as satisfied.
pub const DEFAULT_VOLUME_TOLERANCE: f32 = 0.05;

/// Epsilon to guard against IEEE 754 precision discrepancies (e.g. 0.40 - 0.35 = 0.050000012).
const FLOAT_TOLERANCE_EPSILON: f32 = 1e-4;

/// Default timeout when waiting for playback transition to `Playing`.
pub const DEFAULT_PLAY_POLL_TIMEOUT: Duration = Duration::from_millis(2000);

/// Exact count of commands dispatched during Step 2 execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Step2CommandCounts {
  pub set_volume_calls: usize,
  pub play_calls: usize,
}

/// Evaluated execution plan determining which writes must be performed.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Step2Plan {
  pub need_set_volume: bool,
  pub need_play: bool,
  pub target_volume: f32,
}

impl Step2Plan {
  /// Evaluates pre-state against target volume and tolerance.
  ///
  /// - `need_set_volume` is true if `|current_volume - target_volume| > tolerance`.
  /// - `need_play` is true when playback is paused/stopped/closed.
  ///   A `Changing` session is already transitioning, so it must be observed
  ///   until it reaches `Playing` without dispatching a second `Play` command.
  pub fn evaluate(current_volume: f32, current_status: MediaPlaybackStatus, target_volume: f32, tolerance: f32) -> Self {
    let need_set_volume = (current_volume - target_volume).abs() > (tolerance + FLOAT_TOLERANCE_EPSILON);
    let need_play = !matches!(current_status, MediaPlaybackStatus::Playing | MediaPlaybackStatus::Changing);
    Self {
      need_set_volume,
      need_play,
      target_volume,
    }
  }
}

/// Execution options for Step 2.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Step2Options {
  pub target_volume: f32,
  pub volume_tolerance: f32,
  pub play_poll_timeout: Duration,
  pub fire_and_forget_play: bool,
}

impl Default for Step2Options {
  fn default() -> Self {
    Self {
      target_volume: DEFAULT_TARGET_VOLUME,
      volume_tolerance: DEFAULT_VOLUME_TOLERANCE,
      play_poll_timeout: DEFAULT_PLAY_POLL_TIMEOUT,
      fire_and_forget_play: false,
    }
  }
}

impl Step2Options {
  pub fn new(target_volume: f32, volume_tolerance: f32) -> Self {
    Self {
      target_volume,
      volume_tolerance,
      play_poll_timeout: DEFAULT_PLAY_POLL_TIMEOUT,
      fire_and_forget_play: false,
    }
  }

  pub fn with_timeout(mut self, timeout: Duration) -> Self {
    self.play_poll_timeout = timeout;
    self
  }

  pub fn with_fire_and_forget_play(mut self, fire_and_forget: bool) -> Self {
    self.fire_and_forget_play = fire_and_forget;
    self
  }
}

/// Result of Step 2 execution, detailing skip decisions, command counts, and gate status.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Step2Result {
  pub skipped_volume_write: bool,
  pub skipped_play_write: bool,
  pub play_dispatched: bool,
  pub command_counts: Step2CommandCounts,
  pub final_volume: f32,
  pub final_status: MediaPlaybackStatus,
  pub gate_passed: bool,
}

/// Abstract sink for media playback and process volume mutations.
///
/// Implemented by [`RealPlaybackSink`] for live Windows sessions and
/// [`MockPlaybackSink`] for unit testing and deterministic verification.
pub trait PlaybackActionSink {
  /// Reads the current audio volume level in `[0.0, 1.0]`.
  fn get_volume(&self) -> DriverResult<f32>;

  /// Sets the audio volume level to `volume` in `[0.0, 1.0]`.
  fn set_volume(&mut self, volume: f32) -> DriverResult<()>;

  /// Reads the current media playback status.
  fn get_playback_status(&self) -> DriverResult<MediaPlaybackStatus>;

  /// Commands the media session to play.
  fn play(&mut self) -> DriverResult<()>;

  /// Polls or waits until status becomes `MediaPlaybackStatus::Playing` or timeout expires.
  ///
  /// Default implementation uses adaptive backoff polling.
  fn wait_for_playing(&self, timeout: Duration) -> DriverResult<MediaPlaybackStatus> {
    let mut status = self.get_playback_status()?;
    if status == MediaPlaybackStatus::Playing || timeout.is_zero() {
      return Ok(status);
    }
    let start = Instant::now();
    let mut backoff = Duration::from_millis(10);
    while start.elapsed() < timeout {
      std::thread::sleep(backoff);
      backoff = (backoff * 2).min(Duration::from_millis(80));
      status = self.get_playback_status()?;
      if status == MediaPlaybackStatus::Playing {
        break;
      }
    }
    Ok(status)
  }
}

/// Executor encapsulating Step 2 idempotency logic, command dispatch, and selective verification.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Step2Executor {
  pub options: Step2Options,
}

impl Step2Executor {
  pub fn new(options: Step2Options) -> Self {
    Self { options }
  }

  /// Evaluates whether volume write and play invocation are necessary.
  pub fn plan(&self, current_volume: f32, current_status: MediaPlaybackStatus) -> Step2Plan {
    Step2Plan::evaluate(current_volume, current_status, self.options.target_volume, self.options.volume_tolerance)
  }

  /// Executes Step 2 given pre-state values.
  ///
  /// Writes ONLY necessary items, records exact command counts, and verifies ONLY actually changed fields.
  pub fn execute<S: PlaybackActionSink>(&self, sink: &mut S, pre_volume: f32, pre_status: MediaPlaybackStatus) -> DriverResult<Step2Result> {
    let plan = self.plan(pre_volume, pre_status);
    let mut counts = Step2CommandCounts::default();

    // 1. Dispatch writes ONLY when needed
    if plan.need_set_volume {
      sink.set_volume(self.options.target_volume)?;
      counts.set_volume_calls += 1;
    }

    let play_dispatched = if plan.need_play {
      sink.play()?;
      counts.play_calls += 1;
      true
    } else {
      false
    };

    // 2. Selective verification: verify ONLY fields that were actually changed.
    // If a field was skipped, its pre-state was already within required bounds.
    let (final_volume, vol_ok) = if plan.need_set_volume {
      let v = sink.get_volume()?;
      let ok = (v - self.options.target_volume).abs() <= (self.options.volume_tolerance + FLOAT_TOLERANCE_EPSILON);
      (v, ok)
    } else {
      (pre_volume, true)
    };

    let need_verify_play = plan.need_play || (pre_status == MediaPlaybackStatus::Changing && !self.options.fire_and_forget_play);
    let (final_status, status_ok) = if need_verify_play {
      if self.options.fire_and_forget_play && plan.need_play {
        (pre_status, true)
      } else {
        let st = sink.wait_for_playing(self.options.play_poll_timeout)?;
        let ok = st == MediaPlaybackStatus::Playing;
        (st, ok)
      }
    } else {
      (pre_status, true)
    };

    let gate_passed = vol_ok && status_ok;

    Ok(Step2Result {
      skipped_volume_write: !plan.need_set_volume,
      skipped_play_write: !plan.need_play,
      play_dispatched,
      command_counts: counts,
      final_volume,
      final_status,
      gate_passed,
    })
  }

  /// Reads pre-state directly from `sink` and executes guarded Step 2 writes.
  pub fn execute_guarded<S: PlaybackActionSink>(&self, sink: &mut S) -> DriverResult<Step2Result> {
    let pre_vol = sink.get_volume()?;
    let pre_st = sink.get_playback_status()?;
    self.execute(sink, pre_vol, pre_st)
  }
}

/// Adapter bridging real Windows media objects ([`ProcessAudioVolume`] and [`SmtcSession`])
/// to [`PlaybackActionSink`].
pub struct RealPlaybackSink<'a> {
  pub audio: Option<&'a ProcessAudioVolume>,
  pub session: &'a SmtcSession,
  pub counts: Step2CommandCounts,
}

impl<'a> RealPlaybackSink<'a> {
  pub fn new(audio: Option<&'a ProcessAudioVolume>, session: &'a SmtcSession) -> Self {
    Self {
      audio,
      session,
      counts: Step2CommandCounts::default(),
    }
  }
}

impl<'a> PlaybackActionSink for RealPlaybackSink<'a> {
  fn get_volume(&self) -> DriverResult<f32> {
    if let Some(audio) = self.audio {
      audio.get_volume()
    } else {
      Ok(0.0)
    }
  }

  fn set_volume(&mut self, volume: f32) -> DriverResult<()> {
    self.counts.set_volume_calls += 1;
    if let Some(audio) = self.audio {
      audio.set_volume(volume)?;
    }
    Ok(())
  }

  fn get_playback_status(&self) -> DriverResult<MediaPlaybackStatus> {
    self.session.playback_status()
  }

  fn play(&mut self) -> DriverResult<()> {
    self.counts.play_calls += 1;
    self.session.play()?;
    Ok(())
  }
}

/// Convenience entry point to execute Step 2 against real Windows media handles.
pub fn execute_step2_real(audio: Option<&ProcessAudioVolume>, session: &SmtcSession, options: Step2Options) -> DriverResult<Step2Result> {
  let mut sink = RealPlaybackSink::new(audio, session);
  let executor = Step2Executor::new(options);
  executor.execute_guarded(&mut sink)
}

/// Convenience entry point to execute Step 2 against real Windows media handles with pre-read state.
pub fn execute_step2_real_with_prestate(
  audio: Option<&ProcessAudioVolume>,
  session: &SmtcSession,
  pre_volume: f32,
  pre_status: MediaPlaybackStatus,
  options: Step2Options,
) -> DriverResult<Step2Result> {
  let mut sink = RealPlaybackSink::new(audio, session);
  let executor = Step2Executor::new(options);
  executor.execute(&mut sink, pre_volume, pre_status)
}

/// In-memory mock implementing [`PlaybackActionSink`] for unit testing and command counting assertions.
#[derive(Debug, Clone, PartialEq)]
pub struct MockPlaybackSink {
  pub volume: f32,
  pub status: MediaPlaybackStatus,
  pub counts: Step2CommandCounts,
  pub fail_set_volume: bool,
  pub fail_play: bool,
}

impl MockPlaybackSink {
  pub fn new(volume: f32, status: MediaPlaybackStatus) -> Self {
    Self {
      volume,
      status,
      counts: Step2CommandCounts::default(),
      fail_set_volume: false,
      fail_play: false,
    }
  }
}

impl PlaybackActionSink for MockPlaybackSink {
  fn get_volume(&self) -> DriverResult<f32> {
    Ok(self.volume)
  }

  fn set_volume(&mut self, volume: f32) -> DriverResult<()> {
    self.counts.set_volume_calls += 1;
    if self.fail_set_volume {
      return Err(crate::error::backend("Simulated volume write failure"));
    }
    self.volume = volume;
    Ok(())
  }

  fn get_playback_status(&self) -> DriverResult<MediaPlaybackStatus> {
    Ok(self.status)
  }

  fn play(&mut self) -> DriverResult<()> {
    self.counts.play_calls += 1;
    if self.fail_play {
      return Err(crate::error::backend("Simulated play dispatch failure"));
    }
    self.status = MediaPlaybackStatus::Playing;
    Ok(())
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  // ROOT CAUSE:
  //
  // In unoptimized execution paths, Step 2 unconditionally dispatched SetMasterVolume
  // and Play calls even when the target was already Playing and volume was already at
  // 40%, wasting latency on redundant COM/WinRT roundtrips.
  //
  // Step2Executor guards writes with pre-state evaluation and tolerance bounds,
  // recording exact command dispatches and verifying only modified properties.

  #[test]
  fn test_already_playing_and_volume_40_skips_all_writes() {
    let mut sink = MockPlaybackSink::new(0.40, MediaPlaybackStatus::Playing);
    let executor = Step2Executor::default();

    let result = executor.execute_guarded(&mut sink).expect("execution should succeed");

    assert_eq!(result.command_counts.set_volume_calls, 0);
    assert_eq!(result.command_counts.play_calls, 0);
    assert!(result.skipped_volume_write);
    assert!(result.skipped_play_write);
    assert!(result.gate_passed);
    assert_eq!(result.final_volume, 0.40);
    assert_eq!(result.final_status, MediaPlaybackStatus::Playing);

    // Assert sink counters match executor counters
    assert_eq!(sink.counts.set_volume_calls, 0);
    assert_eq!(sink.counts.play_calls, 0);
  }

  #[test]
  fn test_paused_and_volume_40_executes_only_play() {
    let mut sink = MockPlaybackSink::new(0.40, MediaPlaybackStatus::Paused);
    let executor = Step2Executor::default();

    let result = executor.execute_guarded(&mut sink).expect("execution should succeed");

    assert_eq!(result.command_counts.set_volume_calls, 0);
    assert_eq!(result.command_counts.play_calls, 1);
    assert!(result.skipped_volume_write);
    assert!(!result.skipped_play_write);
    assert!(result.gate_passed);
    assert_eq!(result.final_volume, 0.40);
    assert_eq!(result.final_status, MediaPlaybackStatus::Playing);

    assert_eq!(sink.counts.set_volume_calls, 0);
    assert_eq!(sink.counts.play_calls, 1);
  }

  #[test]
  fn test_changing_waits_without_replaying() {
    struct ChangingSink {
      status: MediaPlaybackStatus,
      counts: Step2CommandCounts,
    }

    impl PlaybackActionSink for ChangingSink {
      fn get_volume(&self) -> DriverResult<f32> {
        Ok(0.40)
      }
      fn set_volume(&mut self, _volume: f32) -> DriverResult<()> {
        self.counts.set_volume_calls += 1;
        Ok(())
      }
      fn get_playback_status(&self) -> DriverResult<MediaPlaybackStatus> {
        Ok(self.status)
      }
      fn play(&mut self) -> DriverResult<()> {
        self.counts.play_calls += 1;
        self.status = MediaPlaybackStatus::Playing;
        Ok(())
      }
      fn wait_for_playing(&self, _timeout: Duration) -> DriverResult<MediaPlaybackStatus> {
        Ok(MediaPlaybackStatus::Playing)
      }
    }

    let mut sink = ChangingSink {
      status: MediaPlaybackStatus::Changing,
      counts: Step2CommandCounts::default(),
    };
    let result = Step2Executor::default().execute_guarded(&mut sink).expect("changing playback should be verifiable");

    assert_eq!(result.command_counts.play_calls, 0);
    assert!(result.skipped_play_write);
    assert!(result.gate_passed);
    assert_eq!(result.final_status, MediaPlaybackStatus::Playing);
  }

  #[test]
  fn test_volume_10_and_playing_executes_only_volume() {
    let mut sink = MockPlaybackSink::new(0.10, MediaPlaybackStatus::Playing);
    let executor = Step2Executor::default();

    let result = executor.execute_guarded(&mut sink).expect("execution should succeed");

    assert_eq!(result.command_counts.set_volume_calls, 1);
    assert_eq!(result.command_counts.play_calls, 0);
    assert!(!result.skipped_volume_write);
    assert!(result.skipped_play_write);
    assert!(result.gate_passed);
    assert!((result.final_volume - 0.40).abs() <= f32::EPSILON);
    assert_eq!(result.final_status, MediaPlaybackStatus::Playing);

    assert_eq!(sink.counts.set_volume_calls, 1);
    assert_eq!(sink.counts.play_calls, 0);
  }

  #[test]
  fn test_volume_43_within_tolerance_skips_volume_write() {
    // 0.43 vs target 0.40 -> diff 0.03 <= tolerance 0.05
    let mut sink = MockPlaybackSink::new(0.43, MediaPlaybackStatus::Playing);
    let executor = Step2Executor::default();

    let result = executor.execute_guarded(&mut sink).expect("execution should succeed");

    assert_eq!(result.command_counts.set_volume_calls, 0);
    assert_eq!(result.command_counts.play_calls, 0);
    assert!(result.skipped_volume_write);
    assert!(result.skipped_play_write);
    assert!(result.gate_passed);
    assert_eq!(result.final_volume, 0.43);
    assert_eq!(result.final_status, MediaPlaybackStatus::Playing);

    assert_eq!(sink.counts.set_volume_calls, 0);
    assert_eq!(sink.counts.play_calls, 0);
  }

  #[test]
  fn test_volume_46_outside_tolerance_executes_volume_write() {
    // 0.46 vs target 0.40 -> diff 0.06 > tolerance 0.05
    let mut sink = MockPlaybackSink::new(0.46, MediaPlaybackStatus::Playing);
    let executor = Step2Executor::default();

    let result = executor.execute_guarded(&mut sink).expect("execution should succeed");

    assert_eq!(result.command_counts.set_volume_calls, 1);
    assert_eq!(result.command_counts.play_calls, 0);
    assert!(!result.skipped_volume_write);
    assert!(result.skipped_play_write);
    assert!(result.gate_passed);
    assert!((result.final_volume - 0.40).abs() <= f32::EPSILON);
    assert_eq!(result.final_status, MediaPlaybackStatus::Playing);

    assert_eq!(sink.counts.set_volume_calls, 1);
    assert_eq!(sink.counts.play_calls, 0);
  }

  #[test]
  fn test_lower_boundary_tolerance() {
    // 0.35 vs target 0.40 -> diff 0.05 <= tolerance 0.05 -> skip write
    let mut sink_boundary = MockPlaybackSink::new(0.35, MediaPlaybackStatus::Playing);
    let executor = Step2Executor::default();
    let res1 = executor.execute_guarded(&mut sink_boundary).expect("execution should succeed");
    assert_eq!(res1.command_counts.set_volume_calls, 0);
    assert!(res1.skipped_volume_write);

    // 0.34 vs target 0.40 -> diff 0.06 > tolerance 0.05 -> execute write
    let mut sink_outside = MockPlaybackSink::new(0.34, MediaPlaybackStatus::Playing);
    let res2 = executor.execute_guarded(&mut sink_outside).expect("execution should succeed");
    assert_eq!(res2.command_counts.set_volume_calls, 1);
    assert!(!res2.skipped_volume_write);
    assert!((res2.final_volume - 0.40).abs() <= f32::EPSILON);
  }

  #[test]
  fn test_paused_and_volume_outside_tolerance_executes_both() {
    let mut sink = MockPlaybackSink::new(0.20, MediaPlaybackStatus::Paused);
    let executor = Step2Executor::default();

    let result = executor.execute_guarded(&mut sink).expect("execution should succeed");

    assert_eq!(result.command_counts.set_volume_calls, 1);
    assert_eq!(result.command_counts.play_calls, 1);
    assert!(!result.skipped_volume_write);
    assert!(!result.skipped_play_write);
    assert!(result.gate_passed);
    assert!((result.final_volume - 0.40).abs() <= f32::EPSILON);
    assert_eq!(result.final_status, MediaPlaybackStatus::Playing);

    assert_eq!(sink.counts.set_volume_calls, 1);
    assert_eq!(sink.counts.play_calls, 1);
  }

  #[test]
  fn test_idempotent_repeated_execution() {
    let mut sink = MockPlaybackSink::new(0.10, MediaPlaybackStatus::Paused);
    let executor = Step2Executor::default();

    // 1st run: both writes executed
    let r1 = executor.execute_guarded(&mut sink).expect("run 1 should succeed");
    assert_eq!(r1.command_counts.set_volume_calls, 1);
    assert_eq!(r1.command_counts.play_calls, 1);

    // 2nd run: both writes skipped because state is now satisfied
    let r2 = executor.execute_guarded(&mut sink).expect("run 2 should succeed");
    assert_eq!(r2.command_counts.set_volume_calls, 0);
    assert_eq!(r2.command_counts.play_calls, 0);
    assert!(r2.skipped_volume_write);
    assert!(r2.skipped_play_write);
    assert!(r2.gate_passed);

    // Cumulative calls in sink remain 1 each
    assert_eq!(sink.counts.set_volume_calls, 1);
    assert_eq!(sink.counts.play_calls, 1);
  }

  #[test]
  fn test_gate_fails_when_play_fails_to_transition() {
    struct NonTransitioningSink {
      volume: f32,
      counts: Step2CommandCounts,
    }
    impl PlaybackActionSink for NonTransitioningSink {
      fn get_volume(&self) -> DriverResult<f32> {
        Ok(self.volume)
      }
      fn set_volume(&mut self, volume: f32) -> DriverResult<()> {
        self.counts.set_volume_calls += 1;
        self.volume = volume;
        Ok(())
      }
      fn get_playback_status(&self) -> DriverResult<MediaPlaybackStatus> {
        Ok(MediaPlaybackStatus::Paused)
      }
      fn play(&mut self) -> DriverResult<()> {
        self.counts.play_calls += 1;
        // Do not update status to Playing
        Ok(())
      }
    }

    let mut sink = NonTransitioningSink {
      volume: 0.40,
      counts: Step2CommandCounts::default(),
    };
    let options = Step2Options::default().with_timeout(Duration::ZERO);
    let executor = Step2Executor::new(options);

    let result = executor.execute_guarded(&mut sink).expect("execution should succeed");
    assert_eq!(result.command_counts.play_calls, 1);
    assert!(!result.gate_passed, "gate must fail when status is not Playing");
    assert_eq!(result.final_status, MediaPlaybackStatus::Paused);
  }

  #[test]
  fn test_fire_and_forget_play() {
    // ROOT CAUSE:
    //
    // In Fast mode, polling wait_for_playing() for SMTC/QQ Music playback status transition
    // introduced 100-2000ms latency, even though action dispatch had already succeeded.
    //
    // Before the fix, Step 2 unconditionally polled wait_for_playing() whenever play was needed.
    // The fix introduces fire_and_forget_play: true which dispatches play() and returns
    // immediately (<100ms) with play_dispatched: true and gate_passed: true, while
    // fire_and_forget_play: false continues to wait and verify semantic playback state.

    struct SlowWaitMockSink {
      volume: f32,
      status: MediaPlaybackStatus,
      counts: Step2CommandCounts,
    }

    impl PlaybackActionSink for SlowWaitMockSink {
      fn get_volume(&self) -> DriverResult<f32> {
        Ok(self.volume)
      }
      fn set_volume(&mut self, volume: f32) -> DriverResult<()> {
        self.counts.set_volume_calls += 1;
        self.volume = volume;
        Ok(())
      }
      fn get_playback_status(&self) -> DriverResult<MediaPlaybackStatus> {
        Ok(self.status)
      }
      fn play(&mut self) -> DriverResult<()> {
        self.counts.play_calls += 1;
        // play succeeds immediately
        Ok(())
      }
      fn wait_for_playing(&self, _timeout: Duration) -> DriverResult<MediaPlaybackStatus> {
        // Blocks for 2 seconds
        std::thread::sleep(Duration::from_millis(2000));
        Ok(MediaPlaybackStatus::Playing)
      }
    }

    // 1. With fire_and_forget_play: true, completes in <100ms without calling wait_for_playing
    {
      let mut sink = SlowWaitMockSink {
        volume: 0.40,
        status: MediaPlaybackStatus::Paused,
        counts: Step2CommandCounts::default(),
      };
      let options = Step2Options {
        fire_and_forget_play: true,
        ..Default::default()
      };
      let executor = Step2Executor::new(options);

      let start = Instant::now();
      let result = executor.execute_guarded(&mut sink).expect("execution should succeed");
      let elapsed = start.elapsed();

      assert!(elapsed < Duration::from_millis(100), "Fast mode took {:?}, expected <100ms", elapsed);
      assert_eq!(result.command_counts.play_calls, 1);
      assert!(result.play_dispatched);
      assert!(result.gate_passed);
      assert_eq!(sink.counts.play_calls, 1);
    }

    // 2. With fire_and_forget_play: false, it actually waits and verifies as before
    {
      let mut sink = SlowWaitMockSink {
        volume: 0.40,
        status: MediaPlaybackStatus::Paused,
        counts: Step2CommandCounts::default(),
      };
      let options = Step2Options {
        fire_and_forget_play: false,
        ..Default::default()
      };
      let executor = Step2Executor::new(options);

      let start = Instant::now();
      let result = executor.execute_guarded(&mut sink).expect("execution should succeed");
      let elapsed = start.elapsed();

      assert!(elapsed >= Duration::from_millis(1900), "Verified mode should have waited ~2s, took {:?}", elapsed);
      assert_eq!(result.command_counts.play_calls, 1);
      assert!(result.play_dispatched);
      assert!(result.gate_passed);
      assert_eq!(result.final_status, MediaPlaybackStatus::Playing);
      assert_eq!(sink.counts.play_calls, 1);
    }
  }
}
