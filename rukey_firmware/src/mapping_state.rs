use crate::keyboard::{KEYBOARD_CHANNEL, KeyboardEvent};
use crate::mouse::{MOUSE_CHANNEL, MouseEvent};
use core::ops::ControlFlow;
use embassy_time::Instant;
use rukey_config::{ComputerInput, Mapping, MappingMode, RukeyControl};

use crate::input::State;

#[derive(Clone, Copy)]
pub struct MappingState {
    phase: MappingPhase,
    output: Option<RunningOutputSequence>,
}

#[derive(Clone, Copy)]
struct RunningOutputSequence {
    /// Index of the next output_sequence item to process.
    output_index: u8,
    /// Index of the first currently-held output. Items in held_from..output_index are "active".
    /// Updated when AfterMillisRelease releases a segment.
    held_from: u8,
    /// Set at creation and reset to Instant::now() each time an AfterMillis* timer expires.
    waiting_since: Instant,
}

impl MappingState {
    pub fn new() -> Self {
        MappingState {
            phase: MappingPhase::Initial,
            output: None,
        }
    }

    /// Process one tick for a single mapping.
    /// ControlFlow::Continue returns the modified MappingState
    /// ControlFlow::Break indicates that all mapping states are now invalid and need to be recreated.
    ///
    /// arguments:
    /// * mapping: The mapping to be processed
    /// * all_pressed: are all of the mappings inputs currently held?
    /// * supressed: is the mapping supressed by another higher priority mapping?
    /// * state to be modified
    pub async fn process(
        mut self,
        mapping: &Mapping,
        all_pressed: bool,
        suppressed: bool,
        state: &mut State,
    ) -> ControlFlow<(), MappingState> {
        // progress the phase state machine
        self.process_phase(mapping, all_pressed, suppressed).await;

        // output sequences are run independently of the phase state machine
        self.process_output_sequence(mapping, state).await?;

        ControlFlow::Continue(self)
    }

    async fn process_phase(&mut self, mapping: &Mapping, all_pressed: bool, suppressed: bool) {
        self.phase = match mapping.mode {
            MappingMode::OnPress => match (self.phase, all_pressed, suppressed) {
                (MappingPhase::Initial, false, _) => MappingPhase::Released,
                (MappingPhase::Released, true, false) => {
                    self.start_outputs();
                    MappingPhase::Pressed
                }
                (MappingPhase::Pressed, false, false) => {
                    self.stop_outputs(mapping).await;
                    MappingPhase::Released
                }
                (MappingPhase::Pressed, _, true) => {
                    self.stop_outputs(mapping).await;
                    MappingPhase::Initial
                }
                (other, _, _) => other,
            },

            MappingMode::OnHold { hold_ms: threshold } => {
                match (self.phase, all_pressed, suppressed) {
                    (MappingPhase::Initial, false, _) => MappingPhase::Released,
                    (MappingPhase::Released, true, false) => MappingPhase::HeldPending {
                        since: Instant::now(),
                    },
                    (MappingPhase::HeldPending { .. }, _, true) => MappingPhase::Initial,
                    (MappingPhase::HeldPending { since }, true, false) => {
                        if since.elapsed().as_millis() >= threshold as u64 {
                            self.start_outputs();
                            MappingPhase::Pressed
                        } else {
                            self.phase
                        }
                    }
                    (MappingPhase::HeldPending { .. }, false, false) => MappingPhase::Released,
                    (MappingPhase::Pressed, false, false) => {
                        self.stop_outputs(mapping).await;
                        MappingPhase::Released
                    }
                    (MappingPhase::Pressed, _, true) => {
                        self.stop_outputs(mapping).await;
                        MappingPhase::Initial
                    }
                    (other, _, _) => other,
                }
            }

            MappingMode::Toggle => match (self.phase, all_pressed, suppressed) {
                (MappingPhase::Initial, false, _) => MappingPhase::Released,
                (MappingPhase::Released, true, false) => {
                    self.start_outputs();
                    MappingPhase::ToggleOnAwaitingRelease
                }
                (MappingPhase::ToggleOnAwaitingRelease, false, false) => MappingPhase::Pressed,
                (MappingPhase::ToggleOnAwaitingRelease, _, true) => {
                    self.stop_outputs(mapping).await;
                    MappingPhase::Initial
                }
                (MappingPhase::Pressed, true, false) => {
                    self.stop_outputs(mapping).await;
                    MappingPhase::AwaitingRelease
                }
                (MappingPhase::AwaitingRelease, false, _) => MappingPhase::Released,
                (other, _, _) => other,
            },

            MappingMode::MacroOnPress => match (self.phase, all_pressed, suppressed) {
                (MappingPhase::Initial, false, _) => MappingPhase::Released,
                (MappingPhase::Released, true, false) => {
                    self.start_outputs();
                    MappingPhase::Pressed
                }
                (MappingPhase::Pressed, false, false) => MappingPhase::Released,
                (MappingPhase::Pressed, _, true) => {
                    self.stop_outputs(mapping).await;
                    MappingPhase::Initial
                }
                (other, _, _) => other,
            },

            MappingMode::MacroOnRelease => match (self.phase, all_pressed, suppressed) {
                (MappingPhase::Initial, false, _) => MappingPhase::Released,
                (MappingPhase::Released, true, false) => MappingPhase::Pressed,
                (MappingPhase::Pressed, false, false) => {
                    self.start_outputs();
                    MappingPhase::Released
                }
                (MappingPhase::Pressed, _, true) => MappingPhase::Initial,
                (other, _, _) => other,
            },

            MappingMode::MacroOnTap { tap_ms: threshold } => {
                match (self.phase, all_pressed, suppressed) {
                    (MappingPhase::Initial, false, _) => MappingPhase::Released,
                    (MappingPhase::Released, true, false) => MappingPhase::HeldPending {
                        since: Instant::now(),
                    },
                    (MappingPhase::HeldPending { .. }, _, true) => MappingPhase::Initial,
                    (MappingPhase::HeldPending { since }, true, false) => {
                        if since.elapsed().as_millis() >= threshold as u64 {
                            MappingPhase::AwaitingRelease
                        } else {
                            self.phase
                        }
                    }
                    (MappingPhase::HeldPending { .. }, false, false) => {
                        self.start_outputs();
                        MappingPhase::Released
                    }
                    (MappingPhase::AwaitingRelease, false, _) => MappingPhase::Released,
                    (other, _, _) => other,
                }
            }

            MappingMode::MacroOnDoubleTap { tap_ms: threshold } => {
                match (self.phase, all_pressed, suppressed) {
                    (MappingPhase::Initial, false, _) => MappingPhase::Released,
                    (MappingPhase::Released, true, false) => MappingPhase::HeldPending {
                        since: Instant::now(),
                    },
                    (MappingPhase::HeldPending { .. }, _, true) => MappingPhase::Initial,
                    (MappingPhase::HeldPending { since }, true, false) => {
                        if since.elapsed().as_millis() >= threshold as u64 {
                            MappingPhase::AwaitingRelease
                        } else {
                            self.phase
                        }
                    }
                    (MappingPhase::HeldPending { .. }, false, false) => {
                        MappingPhase::DoubleTapGap {
                            since: Instant::now(),
                        }
                    }
                    (MappingPhase::DoubleTapGap { .. }, _, true) => MappingPhase::Initial,
                    (MappingPhase::DoubleTapGap { since }, false, false) => {
                        if since.elapsed().as_millis() >= threshold as u64 {
                            MappingPhase::Released
                        } else {
                            self.phase
                        }
                    }
                    (MappingPhase::DoubleTapGap { .. }, true, false) => {
                        MappingPhase::DoubleTapSecondPressed {
                            since: Instant::now(),
                        }
                    }
                    (MappingPhase::DoubleTapSecondPressed { .. }, _, true) => MappingPhase::Initial,
                    (MappingPhase::DoubleTapSecondPressed { since }, true, false) => {
                        if since.elapsed().as_millis() >= threshold as u64 {
                            MappingPhase::AwaitingRelease
                        } else {
                            self.phase
                        }
                    }
                    (MappingPhase::DoubleTapSecondPressed { .. }, false, false) => {
                        self.start_outputs();
                        MappingPhase::Released
                    }
                    (MappingPhase::AwaitingRelease, false, _) => MappingPhase::Released,
                    (other, _, _) => other,
                }
            }

            MappingMode::MacroOnHold { hold_ms: threshold } => {
                match (self.phase, all_pressed, suppressed) {
                    (MappingPhase::Initial, false, _) => MappingPhase::Released,
                    (MappingPhase::Released, true, false) => MappingPhase::HeldPending {
                        since: Instant::now(),
                    },
                    (MappingPhase::HeldPending { .. }, _, true) => MappingPhase::Initial,
                    (MappingPhase::HeldPending { since }, true, false) => {
                        if since.elapsed().as_millis() >= threshold as u64 {
                            self.start_outputs();
                            MappingPhase::AwaitingRelease
                        } else {
                            self.phase
                        }
                    }
                    (MappingPhase::HeldPending { .. }, false, false) => MappingPhase::Released,
                    (MappingPhase::AwaitingRelease, false, _) => MappingPhase::Released,
                    (other, _, _) => other,
                }
            }
        };
    }

    /// Process the output sequence in chunks terminated by RukeyControl::AfterMillis*
    async fn process_output_sequence(
        &mut self,
        mapping: &Mapping,
        state: &mut State,
    ) -> ControlFlow<()> {
        if let Some(output) = self.output.as_mut() {
            while (output.output_index as usize) < mapping.output_sequence.len() {
                match &mapping.output_sequence[output.output_index as usize] {
                    ComputerInput::Keyboard(key) => {
                        KEYBOARD_CHANNEL.send(KeyboardEvent::Pressed(*key)).await;
                        output.output_index += 1;
                    }
                    ComputerInput::Mouse(mouse) => {
                        MOUSE_CHANNEL.send(MouseEvent::Pressed(*mouse)).await;
                        output.output_index += 1;
                    }
                    ComputerInput::Control(RukeyControl::AfterMillisHold(millis)) => {
                        let millis = *millis;
                        if output.waiting_since.elapsed().as_millis() < millis as u64 {
                            return ControlFlow::Continue(());
                        }
                        // Timer expired, proceed but do not release held inputs
                        output.waiting_since = Instant::now();
                        output.output_index += 1;
                    }
                    ComputerInput::Control(RukeyControl::AfterMillisRelease(millis)) => {
                        let millis = *millis;
                        if output.waiting_since.elapsed().as_millis() < millis as u64 {
                            return ControlFlow::Continue(());
                        }
                        // Timer expired, proceed and release held inputs
                        MappingState::release_held(output, mapping).await;
                        output.held_from = output.output_index + 1;
                        output.waiting_since = Instant::now();
                        output.output_index += 1;
                    }
                    ComputerInput::Control(RukeyControl::Restart) => {
                        MappingState::release_held(output, mapping).await;
                        output.output_index = 0;
                        output.held_from = 0;
                        output.waiting_since = Instant::now();
                        return ControlFlow::Continue(());
                    }
                    ComputerInput::Control(RukeyControl::SetProfile(profile)) => {
                        let profile = *profile;

                        state.current_profile = profile;

                        // after setting the profile we have invalidated all our state,
                        // so we need to clear mapping state and skip further processing
                        state.mapping_states.clear();
                        return ControlFlow::Break(());
                    }
                }
            }

            // terminate macros when the sequence comes to an end.
            if mapping.mode.is_macro()
                && output.output_index as usize >= mapping.output_sequence.len()
                && let Some(last_output) = mapping.output_sequence.last()
            {
                // The user configured an AfterMillis as the last output,
                // so a wait has already occured for the last output and we can stop immediately
                if let ComputerInput::Control(
                    RukeyControl::AfterMillisHold(_) | RukeyControl::AfterMillisRelease(_),
                ) = last_output
                {
                    self.stop_outputs(mapping).await;
                }
                // If the mapping is not configured with a final RukeyControl::AfterMillis* then wait for 50ms,
                // This is a little magic but gives the user a reasonable experience by ensuring the final output is triggered.
                else if output.waiting_since.elapsed().as_millis() > 50 {
                    self.stop_outputs(mapping).await;
                }
            }
        }
        ControlFlow::Continue(())
    }

    /// Start the output_sequence
    fn start_outputs(&mut self) {
        // if macros are still running, just leave them running
        if self.output.is_none() {
            self.output = Some(RunningOutputSequence {
                output_index: 0,
                held_from: 0,
                waiting_since: Instant::now(),
            });
        }
    }

    /// Send release events for all unreleased inputs.
    /// Also the output_sequence is terminated, so any remaining elements in the sequence are skipped
    async fn stop_outputs(&mut self, mapping: &Mapping) {
        if let Some(mut output) = self.output.take() {
            MappingState::release_held(&mut output, mapping).await;
        }
    }

    async fn release_held(output: &mut RunningOutputSequence, mapping: &Mapping) {
        let start = output.held_from as usize;
        let end = (output.output_index as usize).min(mapping.output_sequence.len());
        for output in &mapping.output_sequence[start..end] {
            match output {
                ComputerInput::Keyboard(key) => {
                    KEYBOARD_CHANNEL.send(KeyboardEvent::Released(*key)).await;
                }
                ComputerInput::Mouse(mouse) => {
                    MOUSE_CHANNEL.send(MouseEvent::Released(*mouse)).await;
                }
                ComputerInput::Control(_) => {}
            }
        }
    }
}

#[derive(Clone, Copy)]
enum MappingPhase {
    /// Waits for the physical key(s) to be fully released before allowing the mapping to activate.
    ///
    /// Entered on construction, and re-entered when a more-specific combo mapping suppresses
    /// this one mid-hold. In both cases the invariant is the same: don't activate until the
    /// user performs a clean press from a fully-released state.
    ///
    /// Without this state, a profile-switch mapping and the first mapping in the new profile
    /// sharing the same key would both fire from a single press.
    Initial,
    /// The input is currently considered released.
    Released,
    /// The input is currently considered held since the Instant.
    HeldPending { since: Instant },
    /// The input is currently considered pressed.
    Pressed,
    /// Toggle activated, waiting for physical release before accepting next toggle press.
    ToggleOnAwaitingRelease,
    /// Waiting for full release before returning to Released.
    AwaitingRelease,
    /// DoubleTap: first tap complete, awaiting second press.
    DoubleTapGap { since: Instant },
    /// DoubleTap: second press, awaiting second release to fire.
    DoubleTapSecondPressed { since: Instant },
}
