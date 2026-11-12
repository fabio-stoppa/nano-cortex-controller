use super::data::{CabSelection, Effect, Snapshot};
use crate::fx::types::{NanoFxBlock, NanoFxState};
use crate::link::{Cmd, ParamWrite};
use alloc::{collections::VecDeque, vec::Vec};
use nano_wire::types::{CabSettings, NanoAmpC};

pub struct View {
    pub amp: NanoAmpC,
    pub verified_amp: NanoAmpC,
    pub effects: NanoFxState,
    pub cab: CabSettings,
    pub generation: u32,
    pub cab_generation: u32,
    pub idle: bool,
    pub preset: u8,
}

impl Effect {
    pub fn capture(block: &NanoFxBlock, index: usize) -> Result<Self, &'static str> {
        if !block.valid {
            return Err("Wait for all FX blocks to load");
        }
        let Some(model) = (unsafe { block.model.as_ref() }) else {
            if block.on {
                return Err("Cannot capture an unknown enabled FX model");
            }
            return Ok(Self {
                model: Vec::new(),
                enabled: false,
                parameters: Vec::new(),
            });
        };
        let count = usize::from(model.param_count);
        if count != usize::from(block.param_count)
            || count > block.params.len()
            || (count > 0 && !block.params_loaded)
            || block.params[..count]
                .iter()
                .any(|parameter| !parameter.valid)
        {
            return Err("Wait for every FX parameter to load before saving");
        }
        let effect = Self {
            model: model.id_bytes().to_vec(),
            enabled: block.on,
            parameters: block.params[..count]
                .iter()
                .map(|parameter| parameter.normalized)
                .collect(),
        };
        effect.validate(index)?;
        Ok(effect)
    }

    fn model_ready(&self, current: &NanoFxBlock) -> bool {
        current.valid
            && current.model_id() == self.model
            && usize::from(current.param_count) == self.parameters.len()
            && (self.parameters.is_empty() || current.params_loaded)
            && current.params[..self.parameters.len()]
                .iter()
                .all(|parameter| parameter.valid)
    }

    pub fn matches(&self, current: &NanoFxBlock) -> bool {
        if self.model.is_empty() {
            return current.valid && !current.on;
        }
        current.on == self.enabled && self.parameters_match(current)
    }

    pub fn parameters_match(&self, current: &NanoFxBlock) -> bool {
        if self.model.is_empty() {
            return current.valid && current.model_id().is_empty();
        }
        let Some(model) = crate::fx::models::find_model(&self.model) else {
            return false;
        };
        self.model_ready(current)
            && self.parameters.iter().enumerate().all(|(index, &value)| {
                let expected = crate::fx::params::normalize_for_model(model, index, value);
                (current.params[index].normalized - expected).abs() <= 0.002
            })
    }
}

impl Snapshot {
    pub fn capture(view: &View) -> Result<Self, &'static str> {
        if !view.idle || !view.amp.valid || !view.effects.valid {
            return Err("Wait for the current Nano preset to finish loading");
        }
        let cab = if view.amp.active_cab_slot().is_some() {
            Some([
                view.cab.output.ok_or("Cab settings are not loaded")?,
                view.cab.high_pass.ok_or("Cab settings are not loaded")?,
                view.cab.low_pass.ok_or("Cab settings are not loaded")?,
            ])
        } else {
            None
        };
        let effects: Result<Vec<_>, _> = view
            .effects
            .blocks
            .iter()
            .enumerate()
            .map(|(index, block)| Effect::capture(block, index))
            .collect();
        let snapshot = Self {
            amp: [
                view.amp.gain,
                view.amp.level,
                view.amp.bass,
                view.amp.mid,
                view.amp.treble,
            ],
            capture_slot: view
                .amp
                .have_capture_slot
                .then_some(view.amp.capture_slot)
                .filter(|&slot| slot != 0),
            cab_selection: view
                .amp
                .active_cab_slot()
                .map(|slot| {
                    if !view.amp.have_cab_name {
                        return Err("Wait for the CAB/IR name before saving");
                    }
                    Ok(CabSelection {
                        slot,
                        name: view.amp.to_amp().cab_name,
                    })
                })
                .transpose()?,
            capture_volume: view
                .amp
                .have_capture_volume
                .then_some(view.amp.capture_volume),
            cab,
            gate_on: view.effects.gate.on,
            gate_amount: view
                .effects
                .gate
                .reduction_valid
                .then_some(view.effects.gate.reduction_pct),
            effects: effects?.try_into().map_err(|_| "Invalid FX block count")?,
        };
        if !view.effects.gate.on_known {
            return Err("Wait for gate bypass state");
        }
        snapshot.validate()?;
        Ok(snapshot)
    }

    pub fn matches(&self, view: &View) -> bool {
        let amp = &view.verified_amp;
        let actual = [amp.gain, amp.level, amp.bass, amp.mid, amp.treble];
        view.idle
            && amp.valid
            && self
                .capture_slot
                .is_none_or(|slot| amp.have_capture_slot && amp.capture_slot == slot)
            && self
                .cab_selection
                .as_ref()
                .is_none_or(|cab| cab.matches(amp))
            && self
                .amp
                .iter()
                .zip(actual)
                .enumerate()
                .all(|(knob, (&expected, actual))| knob == 1 || expected.abs_diff(actual) <= 1)
            && self.capture_volume.is_none_or(|expected| {
                amp.have_capture_volume && expected.abs_diff(amp.capture_volume) <= 1
            })
            && self.cab.is_none_or(|expected| {
                [view.cab.output, view.cab.high_pass, view.cab.low_pass]
                    .iter()
                    .zip(expected)
                    .all(|(actual, expected)| {
                        actual.is_some_and(|actual| (actual - expected).abs() <= 0.002)
                    })
            })
            && view.effects.gate.on_known
            && self.gate_on == view.effects.gate.on
            && self.gate_amount.is_none_or(|expected| {
                view.effects.gate.reduction_valid
                    && expected.abs_diff(view.effects.gate.reduction_pct) <= 1
            })
            && self
                .effects
                .iter()
                .zip(&view.effects.blocks)
                .all(|(expected, actual)| expected.matches(actual))
    }
}

enum Step {
    Write(Cmd),
    CaptureReady {
        slot: u8,
        generation: u32,
    },
    CabReady {
        selection: CabSelection,
        generation: u32,
    },
    ModelReady {
        block: usize,
        effect: Effect,
    },
}

pub enum Progress {
    Waiting,
    Write(Cmd),
    Complete,
    Failed(&'static str),
}

pub struct Plan {
    steps: VecDeque<Step>,
    target: Snapshot,
    preset: u8,
    readback: Option<(u32, Option<u32>)>,
    deadline: i64,
}

fn enqueue_effect(
    steps: &mut VecDeque<Step>,
    block: usize,
    effect: &Effect,
    current: Option<&NanoFxBlock>,
) {
    let ready = current.is_some_and(|current| effect.model_ready(current));
    if !effect.model.is_empty() {
        if !ready {
            steps.push_back(Step::Write(Cmd::FxModelId {
                block: block as u8,
                id: effect.model.clone(),
            }));
            steps.push_back(Step::ModelReady {
                block,
                effect: effect.clone(),
            });
        }
        for (wire, &norm) in effect.parameters.iter().enumerate() {
            if ready
                && current.is_some_and(|current| {
                    crate::fx::models::find_model(&effect.model).is_some_and(|model| {
                        current.params[wire].normalized
                            == crate::fx::params::normalize_for_model(model, wire, norm)
                    })
                })
            {
                continue;
            }
            steps.push_back(Step::Write(Cmd::Param(ParamWrite {
                block: block as u8,
                wire: wire as u8,
                norm,
            })));
        }
    }
    steps.push_back(Step::Write(Cmd::Block {
        slot: block as u8,
        on: effect.enabled,
    }));
}

impl Plan {
    pub fn scene(snapshot: Snapshot, current: &View, now: i64) -> Result<Self, &'static str> {
        snapshot.validate()?;
        let mut steps = VecDeque::new();
        let capture_changed = snapshot.capture_slot.is_some_and(|slot| {
            !current.idle
                || !current.amp.valid
                || !current.amp.have_capture_slot
                || current.amp.capture_slot != slot
                || !current.verified_amp.valid
                || !current.verified_amp.have_capture_slot
                || current.verified_amp.capture_slot != slot
        });
        if capture_changed {
            if let Some(slot) = snapshot.capture_slot {
                steps.push_back(Step::Write(Cmd::CaptureAt(slot)));
            }
        }
        let cab_changed = snapshot.cab_selection.as_ref().is_some_and(|cab| {
            capture_changed
                || !current.idle
                || !cab.matches(&current.amp)
                || !cab.matches(&current.verified_amp)
        });
        if cab_changed {
            if let Some(cab) = &snapshot.cab_selection {
                steps.push_back(Step::Write(Cmd::SceneCab {
                    slot: cab.slot,
                    name: cab.name.clone(),
                }));
            }
        }
        for (block, effect) in snapshot.effects.iter().enumerate() {
            enqueue_effect(
                &mut steps,
                block,
                effect,
                (current.idle && current.effects.valid).then_some(&current.effects.blocks[block]),
            );
        }
        let amp = &current.verified_amp;
        let actual = [amp.gain, amp.level, amp.bass, amp.mid, amp.treble];
        let live = [
            current.amp.gain,
            current.amp.level,
            current.amp.bass,
            current.amp.mid,
            current.amp.treble,
        ];
        for (knob, &raw) in snapshot.amp.iter().enumerate() {
            if knob == 1 {
                continue;
            }
            if current.idle
                && !capture_changed
                && amp.valid
                && current.amp.valid
                && actual[knob] == raw
                && live[knob] == raw
            {
                continue;
            }
            steps.push_back(Step::Write(Cmd::AmpKnob {
                knob: knob as u8,
                raw,
            }));
        }
        if let Some(raw) = snapshot.capture_volume.filter(|&raw| {
            !current.idle
                || capture_changed
                || !amp.valid
                || !amp.have_capture_volume
                || amp.capture_volume != raw
                || !current.amp.valid
                || !current.amp.have_capture_volume
                || current.amp.capture_volume != raw
        }) {
            steps.push_back(Step::Write(Cmd::CaptureVolume(raw)));
        }
        if let Some(values) = snapshot.cab {
            for ((which, normalized), actual) in [
                nano_wire::editor::CabFloat::Level,
                nano_wire::editor::CabFloat::HighPass,
                nano_wire::editor::CabFloat::LowPass,
            ]
            .into_iter()
            .zip(values)
            .zip([
                current.cab.output,
                current.cab.high_pass,
                current.cab.low_pass,
            ]) {
                if current.idle && !capture_changed && !cab_changed && actual == Some(normalized) {
                    continue;
                }
                steps.push_back(Step::Write(Cmd::CabParam { which, normalized }));
            }
        }
        if let Some(amount) = snapshot.gate_amount.filter(|&amount| {
            !current.idle
                || !current.effects.valid
                || !current.effects.gate.reduction_valid
                || current.effects.gate.reduction_pct != amount
        }) {
            steps.push_back(Step::Write(Cmd::GateAmount(amount)));
        }
        if !current.idle
            || !current.effects.valid
            || !current.effects.gate.on_known
            || current.effects.gate.on != snapshot.gate_on
        {
            steps.push_back(Step::Write(Cmd::Gate(snapshot.gate_on)));
        }
        Ok(Self {
            steps,
            target: snapshot,
            preset: current.preset,
            readback: None,
            deadline: now + 30_000_000,
        })
    }

    pub fn poll(&mut self, view: &View, now: i64) -> Progress {
        if now >= self.deadline {
            return Progress::Failed("Recall timed out; sound may be partially updated");
        }
        if view.preset != self.preset {
            return Progress::Failed(
                "Nano preset changed during recall; sound may be partially updated",
            );
        }
        if !view.idle {
            return Progress::Waiting;
        }
        loop {
            match self.steps.front() {
                Some(Step::CaptureReady { slot, generation })
                    if *generation == view.generation
                        || !view.verified_amp.valid
                        || !view.verified_amp.have_capture_slot
                        || view.verified_amp.capture_slot != *slot =>
                {
                    return Progress::Waiting
                }
                Some(Step::CabReady {
                    selection,
                    generation,
                }) if *generation == view.generation || !selection.matches(&view.verified_amp) => {
                    return Progress::Waiting
                }
                Some(Step::ModelReady { block, effect })
                    if !effect.model_ready(&view.effects.blocks[*block]) =>
                {
                    return Progress::Waiting
                }
                Some(Step::Write(Cmd::FxModelId { block, id }))
                    if view.effects.valid
                        && view.effects.blocks[usize::from(*block)].valid
                        && view.effects.blocks[usize::from(*block)].params_loaded
                        && view.effects.blocks[usize::from(*block)].model_id() == id.as_slice() =>
                {
                    self.steps.pop_front();
                    continue;
                }
                Some(Step::Write(Cmd::Block { slot, on }))
                    if view.effects.valid
                        && view.effects.blocks[usize::from(*slot)].valid
                        && view.effects.blocks[usize::from(*slot)].on == *on =>
                {
                    self.steps.pop_front();
                    continue;
                }
                Some(_) => match self.steps.pop_front() {
                    Some(Step::Write(command)) => {
                        match &command {
                            Cmd::CaptureAt(slot) => self.steps.push_front(Step::CaptureReady {
                                slot: *slot,
                                generation: view.generation,
                            }),
                            Cmd::SceneCab { slot, name } => self.steps.push_front(Step::CabReady {
                                selection: CabSelection {
                                    slot: *slot,
                                    name: name.clone(),
                                },
                                generation: view.generation,
                            }),
                            _ => {}
                        }
                        return Progress::Write(command);
                    }
                    Some(
                        Step::ModelReady { .. } | Step::CaptureReady { .. } | Step::CabReady { .. },
                    ) => continue,
                    None => unreachable!(),
                },
                None => break,
            }
        }
        match self.readback {
            None => {
                self.readback = Some((view.generation, None));
                Progress::Write(Cmd::SceneRefresh)
            }
            Some((generation, None)) if generation != view.generation => {
                self.readback = Some((generation, Some(view.cab_generation)));
                Progress::Write(Cmd::SceneFxRefresh)
            }
            Some((_, Some(cab_generation)))
                if (self.target.cab.is_none() || view.cab_generation != cab_generation)
                    && self.target.matches(view) =>
            {
                Progress::Complete
            }
            _ => Progress::Waiting,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fx::types::{NanoFxParam, NanoGate};

    fn fixture() -> View {
        let mut view = View {
            amp: NanoAmpC::from(&nano_wire::types::Amp::default()),
            verified_amp: NanoAmpC::from(&nano_wire::types::Amp::default()),
            effects: NanoFxState::reset_with_ids(),
            cab: CabSettings::default(),
            generation: 1,
            cab_generation: 1,
            preset: 1,
            idle: true,
        };
        view.amp.valid = true;
        view.amp.have_capture_volume = true;
        view.verified_amp = view.amp;
        view.effects.valid = true;
        view.effects.gate = NanoGate {
            on: false,
            on_known: true,
            valid: true,
            reduction_valid: false,
            reduction_pct: 0,
        };
        for block in &mut view.effects.blocks {
            block.valid = true;
        }
        view
    }

    fn loaded_model(view: &mut View, index: usize) {
        let model = (0..crate::fx::models::nano_fx_allowed_model_count(index as u32))
            .filter_map(|position| {
                crate::fx::models::nano_fx_get_allowed_model(index as u32, position)
            })
            .find(|model| model.param_count > 0)
            .unwrap();
        let block = &mut view.effects.blocks[index];
        block.model = model;
        block.model_id_raw[..model.id_bytes().len()].copy_from_slice(model.id_bytes());
        block.model_id_len = model.id_len;
        block.param_count = model.param_count;
        block.params_loaded = true;
        for (wire, parameter) in block
            .params
            .iter_mut()
            .take(usize::from(model.param_count))
            .enumerate()
        {
            *parameter = NanoFxParam {
                normalized: crate::fx::params::normalize_for_model(model, wire, 0.4),
                display: 0.0,
                valid: true,
            };
        }
    }

    fn loaded_cab(view: &mut View, slot: u8, name: &str) {
        view.amp.have_cab_slot = true;
        view.amp.cab_slot = slot;
        view.amp.have_cab_name = true;
        view.amp.cab_name.fill(0);
        view.amp.cab_name[..name.len()].copy_from_slice(name.as_bytes());
        view.verified_amp = view.amp;
        view.cab.output = Some(0.5);
        view.cab.high_pass = Some(0.0);
        view.cab.low_pass = Some(1.0);
    }

    fn writes_before_readback(plan: &mut Plan, view: &View) -> Vec<Cmd> {
        let mut writes = Vec::new();
        loop {
            match plan.poll(view, 1) {
                Progress::Write(Cmd::SceneRefresh) => return writes,
                Progress::Write(command) => writes.push(command),
                _ => panic!("Unexpected wait before readback"),
            }
        }
    }

    #[test]
    fn identical_scene_sends_no_sound_writes_but_still_verifies() {
        let mut view = fixture();
        loaded_model(&mut view, 0);
        let snapshot = Snapshot::capture(&view).unwrap();
        let mut plan = Plan::scene(snapshot, &view, 0).unwrap();
        assert!(writes_before_readback(&mut plan, &view).is_empty());
        assert!(matches!(plan.poll(&view, 2), Progress::Waiting));
        view.generation += 1;
        assert!(matches!(
            plan.poll(&view, 3),
            Progress::Write(Cmd::SceneFxRefresh)
        ));
        assert!(matches!(plan.poll(&view, 4), Progress::Complete));
    }

    #[test]
    fn nanos_230_scenes_restore_all_parameters_after_model_loading() {
        for name in [
            "Poly Octaver",
            "Wham",
            "Chief OC2",
            "Blossom (ST)",
            "Shimmer",
        ] {
            let model = crate::fx::models::find_model_by_name(name).unwrap();
            let index = if crate::fx::models::is_reverb_model(model) {
                4
            } else {
                0
            };
            let mut view = fixture();
            let block = &mut view.effects.blocks[index];
            block.model = model;
            block.model_id_raw[..model.id_bytes().len()].copy_from_slice(model.id_bytes());
            block.model_id_len = model.id_len;
            block.param_count = model.param_count;
            block.params_loaded = true;
            for (wire, param) in block
                .params
                .iter_mut()
                .take(usize::from(model.param_count))
                .enumerate()
            {
                param.normalized = crate::fx::params::normalize_for_model(model, wire, 0.4);
                param.valid = true;
            }
            let snapshot = Snapshot::capture(&view).unwrap();
            let expected = snapshot.effects[index].parameters.clone();
            let loaded = view.effects.blocks[index];
            view.effects.blocks[index] = NanoFxBlock::zeroed();
            let mut plan = Plan::scene(snapshot, &view, 0).unwrap();
            assert!(matches!(
                plan.poll(&view, 1),
                Progress::Write(Cmd::FxModelId { block, id })
                    if usize::from(block) == index && id == model.id_bytes()
            ));
            assert!(matches!(plan.poll(&view, 2), Progress::Waiting));
            view.effects.blocks[index] = loaded;
            let writes = writes_before_readback(&mut plan, &view);
            let parameters: Vec<_> = writes
                .iter()
                .filter_map(|command| {
                    if let Cmd::Param(parameter) = command {
                        Some(parameter)
                    } else {
                        None
                    }
                })
                .collect();
            assert_eq!(parameters.len(), expected.len(), "{name}");
            for (wire, parameter) in parameters.iter().enumerate() {
                assert_eq!(usize::from(parameter.block), index);
                assert_eq!(usize::from(parameter.wire), wire);
                assert_eq!(parameter.norm, expected[wire]);
            }
        }
    }

    #[test]
    fn verification_needs_fresh_cab_but_not_another_preset_dump() {
        let mut view = fixture();
        loaded_cab(&mut view, 1, "Saved IR");
        let snapshot = Snapshot::capture(&view).unwrap();
        let mut plan = Plan::scene(snapshot, &view, 0).unwrap();
        assert!(writes_before_readback(&mut plan, &view).is_empty());
        assert!(matches!(plan.poll(&view, 2), Progress::Waiting));
        view.generation += 1;
        assert!(matches!(
            plan.poll(&view, 3),
            Progress::Write(Cmd::SceneFxRefresh)
        ));
        assert!(matches!(plan.poll(&view, 4), Progress::Waiting));
        view.cab_generation += 1;
        assert!(matches!(plan.poll(&view, 5), Progress::Complete));
    }

    #[test]
    fn bypass_only_scene_sends_exactly_one_sound_write() {
        let mut view = fixture();
        loaded_model(&mut view, 0);
        let mut snapshot = Snapshot::capture(&view).unwrap();
        snapshot.effects[0].enabled = true;
        let mut plan = Plan::scene(snapshot, &view, 0).unwrap();
        let writes = writes_before_readback(&mut plan, &view);
        assert!(matches!(
            writes.as_slice(),
            [Cmd::Block { slot: 0, on: true }]
        ));
    }

    #[test]
    fn parameter_only_scene_sends_only_the_changed_parameter() {
        let mut view = fixture();
        loaded_model(&mut view, 0);
        let mut snapshot = Snapshot::capture(&view).unwrap();
        snapshot.effects[0].parameters[0] = 0.9;
        let mut plan = Plan::scene(snapshot, &view, 0).unwrap();
        let writes = writes_before_readback(&mut plan, &view);
        assert!(matches!(
            writes.as_slice(),
            [Cmd::Param(ParamWrite {
                block: 0,
                wire: 0,
                ..
            })]
        ));
    }

    #[test]
    fn scene_restores_gain_and_eq_but_never_master_level() {
        let view = fixture();
        let mut snapshot = Snapshot::capture(&view).unwrap();
        for raw in &mut snapshot.amp {
            *raw = raw.wrapping_add(10);
        }
        let mut plan = Plan::scene(snapshot, &view, 0).unwrap();
        let writes = writes_before_readback(&mut plan, &view);
        assert!(matches!(
            writes.as_slice(),
            [
                Cmd::AmpKnob { knob: 0, .. },
                Cmd::AmpKnob { knob: 2, .. },
                Cmd::AmpKnob { knob: 3, .. },
                Cmd::AmpKnob { knob: 4, .. },
            ]
        ));
    }

    #[test]
    fn master_level_is_neither_restored_nor_required_for_verification() {
        let mut view = fixture();
        let snapshot = Snapshot::capture(&view).unwrap();
        view.amp.level = view.amp.level.wrapping_add(50);
        view.verified_amp.level = view.amp.level;
        assert!(snapshot.matches(&view));
        let mut plan = Plan::scene(snapshot, &view, 0).unwrap();
        assert!(writes_before_readback(&mut plan, &view).is_empty());
    }

    #[test]
    fn scalar_delta_keeps_every_changed_control_and_uses_verified_amp() {
        let mut view = fixture();
        loaded_cab(&mut view, 1, "Saved IR");
        view.effects.gate.reduction_valid = true;
        let mut snapshot = Snapshot::capture(&view).unwrap();
        snapshot.amp[0] = view.verified_amp.gain.wrapping_add(1);
        view.amp.gain = snapshot.amp[0];
        snapshot.capture_volume = Some(view.verified_amp.capture_volume.wrapping_add(1));
        snapshot.cab.as_mut().unwrap()[0] = 0.6;
        snapshot.gate_amount = Some(50);
        snapshot.gate_on = true;
        let mut plan = Plan::scene(snapshot, &view, 0).unwrap();
        let writes = writes_before_readback(&mut plan, &view);
        assert!(matches!(
            writes.as_slice(),
            [
                Cmd::AmpKnob { knob: 0, .. },
                Cmd::CaptureVolume(_),
                Cmd::CabParam {
                    which: nano_wire::editor::CabFloat::Level,
                    ..
                },
                Cmd::GateAmount(50),
                Cmd::Gate(true)
            ]
        ));
    }

    #[test]
    fn stale_verified_amp_cannot_hide_recent_live_edits() {
        let mut view = fixture();
        let snapshot = Snapshot::capture(&view).unwrap();
        view.amp.gain = view.amp.gain.wrapping_add(1);
        view.amp.capture_volume = view.amp.capture_volume.wrapping_add(1);
        let mut plan = Plan::scene(snapshot, &view, 0).unwrap();
        let writes = writes_before_readback(&mut plan, &view);
        assert!(matches!(
            writes.as_slice(),
            [Cmd::AmpKnob { knob: 0, .. }, Cmd::CaptureVolume(_)]
        ));
    }

    #[test]
    fn selections_restore_before_controls_and_require_fresh_verified_state() {
        let mut view = fixture();
        view.amp.have_capture_slot = true;
        view.amp.capture_slot = 25;
        loaded_cab(&mut view, 5, "Saved IR");
        let snapshot = Snapshot::capture(&view).unwrap();
        assert_eq!(snapshot.capture_slot, Some(25));
        assert_eq!(snapshot.cab_selection.as_ref().unwrap().slot, 5);
        assert_eq!(snapshot.cab_selection.as_ref().unwrap().name, "Saved IR");
        view.amp.capture_slot = 1;
        loaded_cab(&mut view, 1, "Other IR");
        assert!(!snapshot.matches(&view));
        let mut plan = Plan::scene(snapshot.clone(), &view, 0).unwrap();
        assert!(matches!(
            plan.poll(&view, 1),
            Progress::Write(Cmd::CaptureAt(25))
        ));
        view.amp.capture_slot = 25;
        assert!(matches!(plan.poll(&view, 2), Progress::Waiting));
        view.generation += 1;
        assert!(matches!(plan.poll(&view, 3), Progress::Waiting));
        view.verified_amp = view.amp;
        assert!(matches!(
            plan.poll(&view, 4),
            Progress::Write(Cmd::SceneCab { slot: 5, name }) if name == "Saved IR"
        ));
        loaded_cab(&mut view, 5, "Saved IR");
        assert!(matches!(plan.poll(&view, 5), Progress::Waiting));
        view.generation += 1;
        let writes = writes_before_readback(&mut plan, &view);
        assert_eq!(
            writes
                .iter()
                .filter(|cmd| matches!(cmd, Cmd::AmpKnob { .. }))
                .count(),
            4
        );
        assert_eq!(
            writes
                .iter()
                .filter(|cmd| matches!(cmd, Cmd::CaptureVolume(_)))
                .count(),
            1
        );
        assert_eq!(
            writes
                .iter()
                .filter(|cmd| matches!(cmd, Cmd::CabParam { .. }))
                .count(),
            3
        );
        assert!(matches!(plan.poll(&view, 6), Progress::Waiting));
        view.generation += 1;
        assert!(matches!(
            plan.poll(&view, 7),
            Progress::Write(Cmd::SceneFxRefresh)
        ));
        view.cab_generation += 1;
        view.verified_amp.capture_slot = 1;
        assert!(matches!(plan.poll(&view, 8), Progress::Waiting));
        view.verified_amp = view.amp;
        view.verified_amp.cab_name[0] = b'X';
        assert!(matches!(plan.poll(&view, 9), Progress::Waiting));
        view.verified_amp = view.amp;
        assert!(matches!(plan.poll(&view, 10), Progress::Complete));
    }

    #[test]
    fn cab_change_in_same_slot_is_restored_and_unchanged_selection_is_skipped() {
        let mut view = fixture();
        loaded_cab(&mut view, 2, "Saved IR");
        let snapshot = Snapshot::capture(&view).unwrap();
        let mut plan = Plan::scene(snapshot.clone(), &view, 0).unwrap();
        assert!(writes_before_readback(&mut plan, &view).is_empty());
        loaded_cab(&mut view, 2, "Other IR");
        let mut plan = Plan::scene(snapshot, &view, 0).unwrap();
        assert!(matches!(
            plan.poll(&view, 1),
            Progress::Write(Cmd::SceneCab { slot: 2, .. })
        ));
        view.generation += 1;
        assert!(matches!(plan.poll(&view, 2), Progress::Waiting));
        loaded_cab(&mut view, 2, "Saved IR");
        let writes = writes_before_readback(&mut plan, &view);
        assert_eq!(writes.len(), 3);
        assert!(writes.iter().all(|cmd| matches!(cmd, Cmd::CabParam { .. })));
    }

    #[test]
    fn capture_selection_uses_both_live_and_verified_state() {
        let mut view = fixture();
        view.amp.have_capture_slot = true;
        view.amp.capture_slot = 3;
        view.verified_amp = view.amp;
        let snapshot = Snapshot::capture(&view).unwrap();
        let mut plan = Plan::scene(snapshot.clone(), &view, 0).unwrap();
        assert!(writes_before_readback(&mut plan, &view).is_empty());
        view.amp.capture_slot = 4;
        let mut plan = Plan::scene(snapshot.clone(), &view, 0).unwrap();
        assert!(matches!(
            plan.poll(&view, 1),
            Progress::Write(Cmd::CaptureAt(3))
        ));
        view.verified_amp.capture_slot = 4;
        view.amp.capture_slot = 3;
        let mut plan = Plan::scene(snapshot, &view, 0).unwrap();
        assert!(matches!(
            plan.poll(&view, 1),
            Progress::Write(Cmd::CaptureAt(3))
        ));
        assert!(matches!(plan.poll(&view, 30_000_000), Progress::Failed(_)));
    }

    #[test]
    fn bypassed_sources_remain_saveable_without_a_slot_selection() {
        let mut view = fixture();
        view.amp.have_capture_slot = true;
        view.amp.have_cab_slot = true;
        let snapshot = Snapshot::capture(&view).unwrap();
        assert!(snapshot.capture_slot.is_none());
        assert!(snapshot.cab_selection.is_none());
        view.amp.capture_slot = 26;
        assert!(Snapshot::capture(&view).is_err());
    }

    #[test]
    fn changed_model_restores_all_parameters_even_if_old_values_match() {
        let mut view = fixture();
        loaded_model(&mut view, 0);
        let snapshot = Snapshot::capture(&view).unwrap();
        let count = snapshot.effects[0].parameters.len();
        view.effects.blocks[0].model_id_raw[0] ^= 0xff;
        let mut plan = Plan::scene(snapshot, &view, 0).unwrap();
        assert!(matches!(
            plan.poll(&view, 1),
            Progress::Write(Cmd::FxModelId { block: 0, .. })
        ));
        assert!(matches!(plan.poll(&view, 2), Progress::Waiting));
        loaded_model(&mut view, 0);
        let writes = writes_before_readback(&mut plan, &view);
        assert_eq!(
            writes
                .iter()
                .filter(|cmd| matches!(cmd, Cmd::Param(_)))
                .count(),
            count
        );
    }

    #[test]
    fn loading_state_cannot_suppress_required_writes() {
        let mut view = fixture();
        loaded_model(&mut view, 0);
        let snapshot = Snapshot::capture(&view).unwrap();
        let count = snapshot.effects[0].parameters.len();
        view.idle = false;
        let mut plan = Plan::scene(snapshot, &view, 0).unwrap();
        assert!(matches!(plan.poll(&view, 1), Progress::Waiting));
        view.idle = true;
        let writes = writes_before_readback(&mut plan, &view);
        assert_eq!(
            writes
                .iter()
                .filter(|cmd| matches!(cmd, Cmd::Param(_)))
                .count(),
            count
        );
        assert_eq!(
            writes
                .iter()
                .filter(|cmd| matches!(cmd, Cmd::AmpKnob { .. }))
                .count(),
            4
        );
    }

    #[test]
    fn capture_rejects_unknown_or_missing_values_and_keeps_bypassed_parameters() {
        let mut view = fixture();
        loaded_model(&mut view, 0);
        let saved = Snapshot::capture(&view).unwrap();
        assert!(!saved.effects[0].enabled);
        assert!(!saved.effects[0].parameters.is_empty());
        view.effects.blocks[0].params[0].valid = false;
        assert!(Snapshot::capture(&view).is_err());
        view.effects.blocks[0].params[0].valid = true;
        view.effects.blocks[1].on = true;
        assert!(Snapshot::capture(&view).is_err());
        view.effects.blocks[1].on = false;
        view.amp.cab_slot = 1;
        view.amp.have_cab_slot = true;
        assert!(Snapshot::capture(&view).is_err());
    }

    #[test]
    fn recall_waits_for_models_and_requires_fresh_state_and_parameter_readback() {
        let mut view = fixture();
        loaded_model(&mut view, 0);
        let snapshot = Snapshot::capture(&view).unwrap();
        view.effects.blocks[0].params_loaded = false;
        let mut plan = Plan::scene(snapshot.clone(), &view, 0).unwrap();
        assert!(matches!(
            plan.poll(&view, 1),
            Progress::Write(Cmd::FxModelId { block: 0, .. })
        ));
        assert!(matches!(plan.poll(&view, 2), Progress::Waiting));
        view.effects.blocks[0].params_loaded = true;
        let mut parameters = 0;
        loop {
            match plan.poll(&view, 3) {
                Progress::Write(Cmd::SceneRefresh) => break,
                Progress::Write(Cmd::Param(_)) => parameters += 1,
                Progress::Write(_) => {}
                _ => panic!("Unexpected progress before state readback"),
            }
        }
        assert_eq!(parameters, snapshot.effects[0].parameters.len());
        assert!(
            matches!(plan.poll(&view, 4), Progress::Waiting),
            "Cached values cannot verify recall"
        );
        view.generation += 1;
        assert!(matches!(
            plan.poll(&view, 5),
            Progress::Write(Cmd::SceneFxRefresh)
        ));
        view.effects.blocks[0].params_loaded = false;
        assert!(matches!(plan.poll(&view, 6), Progress::Waiting));
        view.effects.blocks[0].params_loaded = true;
        view.effects.blocks[0].params[0].normalized = 1.0;
        assert!(
            matches!(plan.poll(&view, 7), Progress::Waiting),
            "Wrong actual parameter accepted"
        );
        view.effects.blocks[0].params[0].normalized = snapshot.effects[0].parameters[0];
        view.verified_amp.gain = view.amp.gain.wrapping_add(10);
        assert!(
            matches!(plan.poll(&view, 8), Progress::Waiting),
            "Optimistic amp values masked failed raw readback"
        );
        view.verified_amp = view.amp;
        assert!(matches!(plan.poll(&view, 8), Progress::Complete));
        assert!(matches!(plan.poll(&view, 30_000_000), Progress::Failed(_)));
    }

    #[test]
    fn scene_json_rejects_ranges_and_invalid_preset_numbers() {
        let mut view = fixture();
        loaded_model(&mut view, 0);
        let mut snapshot = Snapshot::capture(&view).unwrap();
        snapshot.effects[0].parameters[0] = f32::NAN;
        assert!(snapshot.validate().is_err());
        snapshot.effects[0].parameters[0] = 0.4;
        let mut config = super::super::data::Config::default();
        let mut collection = super::super::data::PresetScenes {
            preset: 1,
            eq_trim: [0; 3],
            scenes: Default::default(),
        };
        collection.scenes[0] = Some(super::super::data::Scene {
            name: "Clean".into(),
            snapshot,
        });
        config.preset_scenes.push(collection);
        assert!(super::super::data::Config::decode(&config.encode().unwrap()).is_ok());
        config.preset_scenes[0].preset = 0;
        assert!(config.encode().is_err());
    }
}
