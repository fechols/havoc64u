//! Foreground-only scenic sound choreography. The IRQ owns sound envelopes;
//! this small clock follows rendered scenery and never replays a missed burst.
use crate::audio::{AudioEvent, Effect};

#[cfg_attr(target_arch = "mos", link_section = ".phase.scenery.data")]
pub static METEOR_STARTS: [u16; 8] = [100, 140, 170, 200, 230, 260, 290, 300];
pub const METEOR_DURATION: u16 = 80;
const CUE_WINDOW: u16 = 12;
const ROUTE_FIXED: u8 = 15;
const ROUTE_CENTER_FIRST: u8 = 16;
const ROUTE_LEFT: u8 = 128;

#[derive(Clone, Copy)]
struct Cue {
    at: u16,
    effect: Effect,
    // Low nibble: crossing age, or 15 for a fixed route. The other bits
    // identify which side plays and whether the center precedes the crossing.
    route: u8,
}

const fn cue(at: u16, effect: Effect, pan: i8) -> Cue {
    Cue {
        at,
        effect,
        route: ROUTE_FIXED
            | if pan == 0 { ROUTE_CENTER_FIRST } else { 0 }
            | if pan < 0 { ROUTE_LEFT } else { 0 },
    }
}

// Source motion is linear throughout each cue's twelve-tick acceptance window.
// Resolve its exact screen-side crossings at compile time, including depth
// changes, rather than loading projection arithmetic with every scenery frame.
const fn moving_cue(at: u16, effect: Effect, x: i32, z: i32, dx: i32, dz: i32) -> Cue {
    let mut initial_pan = 0;
    let mut previous_pan = 0;
    let mut side = 0;
    let mut crossing = ROUTE_FIXED;
    let mut age = 0;
    while age <= CUE_WINDOW {
        let current_x = x + dx * age as i32;
        let current_z = z + dz * age as i32;
        assert!(current_z > 0);
        let pan = if current_x.abs() * 20 <= current_z {
            0
        } else if current_x < 0 {
            -100
        } else {
            100
        };
        if age == 0 {
            initial_pan = pan;
        } else if pan != previous_pan {
            // Authored windows cross only one edge of the narrow center band.
            // Fail compilation if future motion needs a second crossing.
            assert!(crossing == ROUTE_FIXED);
            assert!(pan == 0 || previous_pan == 0);
            crossing = age as u8;
        }
        if pan != 0 {
            side = pan;
        }
        previous_pan = pan;
        age += 1;
    }
    Cue {
        at,
        effect,
        route: crossing
            | if initial_pan == 0 { ROUTE_CENTER_FIRST } else { 0 }
            | if side < 0 { ROUTE_LEFT } else { 0 },
    }
}

const fn footstep(at: u16) -> Cue {
    moving_cue(
        at,
        Effect::Footstep,
        -3200 + at as i32 * 16,
        3000 - at as i32 * 8,
        16,
        -8,
    )
}
const fn bird(at: u16, index: i32) -> Cue {
    moving_cue(
        at,
        Effect::BirdCall,
        -4700 + at as i32 * 24 + index * 180,
        2800 + index * 160,
        24,
        0,
    )
}
const fn transport(at: u16, effect: Effect) -> Cue {
    moving_cue(at, effect, 4800 - at as i32 * 24, 2000, -24, 0)
}
const fn meteor(index: usize) -> Cue {
    let origin = if index & 1 == 0 {
        -600 + index as i32 / 2 * 200
    } else {
        2200 - index as i32 / 2 * 240
    };
    moving_cue(
        METEOR_STARTS[index] + 8,
        Effect::Meteor,
        origin - 8 * 12,
        2600,
        -12,
        0,
    )
}

impl Cue {
    #[inline(always)]
    fn event(self, elapsed: u16) -> AudioEvent {
        let age = (elapsed - self.at) as u8;
        AudioEvent {
            effect: self.effect,
            pan: if (age < self.route & ROUTE_FIXED)
                == (self.route & ROUTE_CENTER_FIRST != 0)
            {
                0
            } else if self.route & ROUTE_LEFT != 0 {
                -100
            } else {
                100
            },
        }
    }
}

// Each scene's slice is sorted by onset, so later entries replace older cues
// on the same lane without a queue or a second clock in the interrupt.
#[cfg_attr(target_arch = "mos", link_section = ".phase.scenery.data")]
static CUES: [Cue; 34] = [
    bird(117, 0),
    footstep(130),
    footstep(156),
    bird(173, 1),
    footstep(182),
    footstep(208),
    bird(229, 2),
    footstep(234),
    footstep(260),
    transport(95, Effect::Transport),
    transport(125, Effect::Transport),
    transport(155, Effect::Transport),
    transport(185, Effect::Transport),
    transport(200, Effect::TransportAway),
    transport(215, Effect::TransportAway),
    transport(245, Effect::TransportAway),
    transport(275, Effect::TransportAway),
    transport(305, Effect::TransportAway),
    meteor(0),
    meteor(1),
    meteor(2),
    meteor(3),
    meteor(4),
    meteor(5),
    meteor(6),
    meteor(7),
    // Ascent/orbital swells have no visible moving transport source.
    // Retain their authored ambience instead of inventing a position.
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
            // A future onset wraps to a large age, outside the short window.
            if cue.at > after && elapsed.wrapping_sub(cue.at) <= CUE_WINDOW {
                let event = cue.event(elapsed);
                // A center cue occupies both lanes. The newest overlapping
                // cue wins, just as two successive cues on one side do.
                if event.pan == 0 || matches!(output.events[0], Some(AudioEvent { pan: 0, .. })) {
                    output.events = [None; 2];
                }
                output.events[usize::from(event.pan > 0)] = Some(event);
            }
        }
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::vec::Vec;

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
                assert_eq!(
                    event.pan,
                    if index == 6 {
                        0
                    } else if index & 1 == 0 {
                        -100
                    } else {
                        100
                    }
                );
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

    #[test]
    fn transport_repeated_cues_follow_its_right_center_left_crossing() {
        let mut clock = TransitionAudio::new();
        for (tick, effect, pan) in [
            (185, Effect::Transport, 100),
            (200, Effect::TransportAway, 0),
            (215, Effect::TransportAway, -100),
        ] {
            let output = clock.update(Some((1, tick)));
            let events: Vec<_> = output.events.into_iter().flatten().collect();
            assert_eq!(events, [AudioEvent { effect, pan }]);
            assert_eq!(clock.update(Some((1, tick))).events, [None; 2]);
        }
    }

    #[test]
    fn delayed_cues_sample_the_visible_position_when_they_actually_start() {
        // The transport passes through the entire center band during this
        // twelve-tick acceptance window. Do not reuse its nominal-onset side.
        for (tick, pan) in [(200, 0), (204, 0), (205, -100), (212, -100)] {
            let mut clock = TransitionAudio::new();
            let events: Vec<_> = clock
                .update(Some((1, tick)))
                .events
                .into_iter()
                .flatten()
                .collect();
            assert_eq!(
                events,
                [AudioEvent {
                    effect: Effect::TransportAway,
                    pan
                }]
            );
        }
        // Meteor six initially lies in the center band. By a delayed frame it
        // has moved left, so meteor seven can occupy the right lane too.
        let mut clock = TransitionAudio::new();
        assert_eq!(
            clock.update(Some((2, 298))).events,
            [
                Some(AudioEvent {
                    effect: Effect::Meteor,
                    pan: 0
                }),
                None
            ]
        );
        let mut delayed = TransitionAudio::new();
        assert_eq!(
            delayed.update(Some((2, 310))).events,
            [
                Some(AudioEvent {
                    effect: Effect::Meteor,
                    pan: -100
                }),
                Some(AudioEvent {
                    effect: Effect::Meteor,
                    pan: 100
                }),
            ]
        );
    }

    #[test]
    fn newest_directional_cue_replaces_an_older_center_cue() {
        // Bird one has just entered the center band while the creature still
        // stands left. Its newer footstep displaces the old two-lane bird cue.
        assert_eq!(CUES[3].event(184).pan, 0);
        let mut clock = TransitionAudio::new();
        assert_eq!(
            clock.update(Some((0, 184))).events,
            [
                Some(AudioEvent {
                    effect: Effect::Footstep,
                    pan: -100
                }),
                None
            ]
        );
    }

    #[test]
    fn compact_crossing_times_match_every_source_through_its_late_window() {
        assert_eq!(core::mem::size_of::<Cue>(), 4);
        for (index, cue) in CUES[..26].iter().enumerate() {
            for late in 0..=CUE_WINDOW {
                let elapsed = cue.at + late;
                let tick = elapsed as i16;
                // Independent transcription of the rendered trajectories,
                // evaluated at runtime rather than using the stored thresholds.
                let (x, z) = if index < 9 {
                    if cue.effect == Effect::Footstep {
                        (-3200 + tick * 16, 3000 - tick * 8)
                    } else {
                        let bird = match index {
                            0 => 0,
                            3 => 1,
                            6 => 2,
                            _ => unreachable!(),
                        };
                        (-4700 + tick * 24 + bird * 180, 2800 + bird * 160)
                    }
                } else if index < 18 {
                    (4800 - tick * 24, 2000)
                } else {
                    let meteor = index - 18;
                    let origin = if meteor & 1 == 0 {
                        -600 + meteor as i16 / 2 * 200
                    } else {
                        2200 - meteor as i16 / 2 * 240
                    };
                    (origin - (elapsed - METEOR_STARTS[meteor]) as i16 * 12, 2600)
                };
                assert_eq!(
                    cue.event(elapsed).pan,
                    crate::audio::spatial_pan(x, z),
                    "cue {index}, late {late}, position ({x},{z})"
                );
            }
        }
    }
}
