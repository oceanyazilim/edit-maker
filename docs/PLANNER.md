# Creative planner

The planner turns `(audio analysis, scenes, preset, controls, target, seed, variant, niche)` into a canonical timeline. The same inputs always produce an identical timeline (`deterministic_for_same_seed` test).

## Algorithm

1. **Audio window.** If the track is longer than the target duration, the planner picks a start time so that the main drop (or a secondary drop) lands at `preset.drop.position` × duration. The start is snapped to a downbeat. If there is no drop, it uses the highest-energy window that starts on a downbeat.
2. **Event hierarchy** (weight):

   | Event | Weight | Forced cut? |
   |---|---|---|
   | Section boundary | 1.0 | yes |
   | Drop | 1.0 | yes |
   | Phrase downbeat | 0.85 | no |
   | Downbeat | 0.75 | no |
   | Kick | 0.35 + 0.35 × intensity × confidence | no |
   | Beat | ≈ 0.2–0.4 | no |

3. **Cut points.**
   - Target shot length: `L = beat × base_shot_beats × section_multiplier × shot_scale`. In a build-up it shrinks progressively.
   - Each candidate cut costs `0.6·|d−L|/L + beat_sync·(1−weight) + temperature·noise`. The cheapest candidate wins.
   - No cuts are allowed inside the anticipation hold before a drop.
   - Shots shorter than the minimum are merged.
4. **Scene scoring.**

   score = `energy_match·(1−|rank−target|) + quality·q + action·peak + niche·tags + duration_fit·fit + continuity·cont − repetition·uses^1.3 − diversity·recent + temperature·noise`

   - Energy is compared on footage *percentile rank*, against section energy plus a bias.
   - On drops, the in-point is chosen so that the scene's action peak lands just after the cut.
   - Elsewhere, the least-reused source range is used.
5. **Effects.**
   - Drops: flash, zoom punch, RGB split and shake, depending on the preset.
   - Kicks: zoom punch or shake, with a cooldown, a per-bar limit, an intensity threshold, and no more than two identical effects in a row.
   - Grade and vignette.
   - Slow push-ins on calm shots.
   - Fades.
   - Soft transitions only in allowed sections. They are checked for source handles and removed automatically if handles are missing.
6. **Validation.** The timeline is validated (overlaps, source bounds, handles, effect ranges) before it is returned.

## Controls (0..1, 0.5 = preset as authored)

| Control | Exact effect |
|---|---|
| Beat sync strength | `beat_sync = preset.beat_sync + (v − 0.5)`. This is the weight of `(1 − event weight)` in the cut cost. |
| Cut density | `shot_scale = 2^((0.5 − v)·2)`. 1.0 halves shot length, 0.0 doubles it. |
| Drop impact | `preset.drop.impact × 2v`. Scales all drop effects. |
| Effect intensity | `preset.effects.intensity × 2v`. 0 disables kick effects. |
| Scene continuity | `scoring.continuity × 2v`. Bonus for the next scene of the same source; penalty for switching source. |
| Motion energy | `energy_bias = preset.energy_bias + (v − 0.5)`. Shifts the target footage energy. |
| Aggressiveness | Minimum shot length × (1.3 − 0.6v); kick threshold + (0.5 − v)·0.4. |
| Transition frequency | `preset.transitions.frequency × 2v` is the probability of a soft transition at an eligible boundary. |

Variants (Balanced, Maximum Impact, Smooth Cinematic, High Motion, Minimal Clean, Experimental) adjust the controls and the selection temperature. In tests, the six variants have a pairwise similarity of 0.37–0.78; the test asserts it stays below 0.85.
