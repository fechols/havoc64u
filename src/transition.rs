//! Foreground-only scenic sound choreography. The IRQ owns sound envelopes;
//! this small clock follows rendered scenery and never replays a missed burst.
use crate::audio::{AudioEvent, Effect};

#[cfg_attr(target_arch = "mos", link_section = ".phase.scenery.data")]
pub static METEOR_STARTS: [u16; 8] = [100, 140, 170, 200, 230, 260, 290, 300];
pub const METEOR_DURATION: u16 = 80;
const CUE_WINDOW: u16 = 12;

#[derive(Clone, Copy)]
struct Cue {
    at: u16,
    event: AudioEvent,
}

const fn cue(at: u16, effect: Effect, pan: i8) -> Cue {
    Cue {
        at,
        event: AudioEvent { effect, pan },
    }
}

// Each scene's slice is sorted by onset, so later entries replace older cues
// on the same lane without a queue or a second clock in the interrupt.
#[cfg_attr(target_arch = "mos", link_section = ".phase.scenery.data")]
static CUES: [Cue; 34] = [
    cue(117, Effect::BirdCall, -100),
    cue(130, Effect::Footstep, -100),
    cue(156, Effect::Footstep, -100),
    cue(173, Effect::BirdCall, -100),
    cue(182, Effect::Footstep, -100),
    cue(208, Effect::Footstep, 100),
    cue(229, Effect::BirdCall, 100),
    cue(234, Effect::Footstep, 100),
    cue(260, Effect::Footstep, 100),
    cue(95, Effect::Transport, 100),
    cue(125, Effect::Transport, 100),
    cue(155, Effect::Transport, 100),
    cue(185, Effect::Transport, 100),
    cue(200, Effect::TransportAway, -100),
    cue(215, Effect::TransportAway, -100),
    cue(245, Effect::TransportAway, -100),
    cue(275, Effect::TransportAway, -100),
    cue(305, Effect::TransportAway, -100),
    cue(METEOR_STARTS[0] + 8, Effect::Meteor, -100),
    cue(METEOR_STARTS[1] + 8, Effect::Meteor, 100),
    cue(METEOR_STARTS[2] + 8, Effect::Meteor, -100),
    cue(METEOR_STARTS[3] + 8, Effect::Meteor, 100),
    cue(METEOR_STARTS[4] + 8, Effect::Meteor, -100),
    cue(METEOR_STARTS[5] + 8, Effect::Meteor, 100),
    cue(METEOR_STARTS[6] + 8, Effect::Meteor, -100),
    cue(METEOR_STARTS[7] + 8, Effect::Meteor, 100),
    cue(36, Effect::Transport, -100),
    cue(88, Effect::Transport, 100),
    cue(154, Effect::TransportAway, -100),
    cue(258, Effect::TransportAway, 100),
    cue(70, Effect::TransportAway, -100),
    cue(125, Effect::TransportAway, 100),
    cue(255, Effect::Transport, -100),
    cue(310, Effect::Transport, 100),
];

/// Owned by the foreground, never borrowed by the audio interrupt.
pub struct TransitionAudio {
    scene: u8,
    inspected: u16,
}

pub struct SceneCues {
    pub cancel: bool,
    pub events: [Option<AudioEvent>; 2],
}

impl TransitionAudio {
    pub const fn new() -> Self {
        Self {
            scene: 255,
            inspected: 0,
        }
    }

    /// Inspect a completed simulation frame. Old or identical timestamps do not
    /// retrigger; a forward seek retains only a recent cue on each stereo lane.
    #[inline(never)]
    #[cfg_attr(target_arch = "mos", link_section = ".phase.scenery")]
    pub fn update(&mut self, progress: Option<(u8, u16)>) -> SceneCues {
        let mut output = SceneCues {
            cancel: false,
            events: [None; 2],
        };
        let (scene, elapsed) = match progress {
            Some((scene, elapsed)) if scene < 5 && elapsed < 400 => (scene, elapsed),
            _ => {
                output.cancel = self.scene != 255;
                self.scene = 255;
                self.inspected = 0;
                return output;
            }
        };
        let changed = self.scene != scene;
        if !changed && elapsed <= self.inspected {
            return output;
        }
        output.cancel = changed;
        let after = if changed { 0 } else { self.inspected };
        self.scene = scene;
        self.inspected = elapsed;
        let (start, end) = match scene {
            0 => (0, 9),
            1 => (9, 18),
            2 => (18, 26),
            3 => (26, 30),
            _ => (30, 34),
        };
        for index in start..end {
            let cue = CUES[index];
            if cue.at > after && cue.at <= elapsed && elapsed - cue.at <= CUE_WINDOW {
                output.events[usize::from(cue.event.pan > 0)] = Some(cue.event);
            }
        }
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_backward_and_expired_frames_do_not_replay_sounds() {
        let mut clock = TransitionAudio::new();
        assert!(clock.update(Some((1, 95))).events[1].is_some());
        assert_eq!(clock.update(Some((1, 95))).events, [None; 2]);
        assert_eq!(clock.update(Some((1, 90))).events, [None; 2]);
        assert_eq!(clock.update(Some((1, 120))).events, [None; 2]);
        // Many skipped pulses do not form a queued burst.
        let cues = clock.update(Some((1, 310)));
        assert!(cues.events[0].is_some());
        assert!(cues.events[1].is_none());
        assert_eq!(clock.update(Some((1, 390))).events, [None; 2]);
    }

    #[test]
    fn exit_reset_and_new_scene_cancel_once_and_allow_a_fresh_run() {
        let mut clock = TransitionAudio::new();
        assert!(!clock.update(None).cancel);
        assert!(clock.update(Some((0, 130))).cancel);
        assert!(clock.update(None).cancel);
        assert!(!clock.update(None).cancel);
        assert!(clock.update(Some((0, 130))).events[0].is_some());
        assert!(clock.update(Some((2, 148))).cancel);
        assert!(clock.update(Some((2, 400))).cancel);
        assert!(!clock.update(Some((2, 400))).cancel);
    }

    #[test]
    fn cues_follow_the_scene_on_both_lanes_at_low_render_rates() {
        for scene in 0..3 {
            for stride in [1, 9, 10, 11, 12] {
                let mut clock = TransitionAudio::new();
                let mut lanes = [0; 2];
                for tick in (0..400).step_by(stride) {
                    for (lane, event) in clock
                        .update(Some((scene, tick)))
                        .events
                        .into_iter()
                        .enumerate()
                    {
                        if let Some(event) = event {
                            lanes[lane] += 1;
                            assert!(tick < 350, "last releases must finish before arrival");
                            assert!(match scene {
                                0 => matches!(event.effect, Effect::Footstep | Effect::BirdCall),
                                1 => matches!(
                                    event.effect,
                                    Effect::Transport | Effect::TransportAway
                                ),
                                _ => event.effect == Effect::Meteor,
                            });
                        }
                    }
                }
                assert!(
                    lanes.iter().all(|&count| count > 0),
                    "scene {scene}, stride {stride}: {lanes:?}"
                );
            }
        }
    }

    #[test]
    fn every_meteor_has_one_cue_after_its_head_becomes_visible() {
        let mut clock = TransitionAudio::new();
        let mut count = 0;
        for tick in 0..400 {
            for event in clock.update(Some((2, tick))).events.into_iter().flatten() {
                let index = METEOR_STARTS
                    .iter()
                    .position(|&start| tick == start + 8)
                    .unwrap();
                assert_eq!(event.pan, if index & 1 == 0 { -100 } else { 100 });
                assert!(tick + 24 < 400);
                count += 1;
            }
        }
        assert_eq!(count, 8);
    }

    #[test]
    fn ascent_and_orbital_crossings_reach_both_sides_without_stale_bursts() {
        for scene in [3, 4] {
            let mut clock = TransitionAudio::new();
            let mut lanes = [0; 2];
            for elapsed in (0..400).step_by(11) {
                for (lane, cue) in clock
                    .update(Some((scene, elapsed)))
                    .events
                    .iter()
                    .enumerate()
                {
                    if let Some(event) = cue {
                        assert!(matches!(
                            event.effect,
                            Effect::Transport | Effect::TransportAway
                        ));
                        lanes[lane] += 1;
                    }
                }
            }
            assert_eq!(lanes, [2, 2]);
            assert!(clock.update(None).cancel);
            assert_eq!(clock.update(Some((scene, 399))).events, [None; 2]);
        }
    }
}
