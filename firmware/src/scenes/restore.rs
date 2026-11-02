use super::data::{Effect, Snapshot};
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
                .amp
                .iter()
                .zip(actual)
                .all(|(&expected, actual)| expected.abs_diff(actual) <= 1)
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
    ModelReady { block: usize, effect: Effect },
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

fn enqueue_effect(steps: &mut VecDeque<Step>, block: usize, effect: &Effect) {
    if !effect.model.is_empty() {
        steps.push_back(Step::Write(Cmd::FxModelId {
            block: block as u8,
            id: effect.model.clone(),
        }));
        steps.push_back(Step::ModelReady {
            block,
            effect: effect.clone(),
        });
        for (wire, &norm) in effect.parameters.iter().enumerate() {
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
    pub fn scene(snapshot: Snapshot, preset: u8, now: i64) -> Result<Self, &'static str> {
        snapshot.validate()?;
        let mut steps = VecDeque::new();
        for (block, effect) in snapshot.effects.iter().enumerate() {
            enqueue_effect(&mut steps, block, effect);
        }
        for (knob, &raw) in snapshot.amp.iter().enumerate() {
            steps.push_back(Step::Write(Cmd::AmpKnob {
                knob: knob as u8,
                raw,
            }));
        }
        if let Some(raw) = snapshot.capture_volume {
            steps.push_back(Step::Write(Cmd::CaptureVolume(raw)));
        }
        if let Some(values) = snapshot.cab {
            for (which, normalized) in [
                nano_wire::editor::CabFloat::Level,
                nano_wire::editor::CabFloat::HighPass,
                nano_wire::editor::CabFloat::LowPass,
            ]
            .into_iter()
            .zip(values)
            {
                steps.push_back(Step::Write(Cmd::CabParam { which, normalized }));
            }
        }
        if let Some(amount) = snapshot.gate_amount {
            steps.push_back(Step::Write(Cmd::GateAmount(amount)));
        }
        steps.push_back(Step::Write(Cmd::Gate(snapshot.gate_on)));
        Ok(Self {
            steps,
            target: snapshot,
            preset,
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
                Some(Step::ModelReady { block, effect })
                    if !effect.model_ready(&view.effects.blocks[*block]) =>
                {
                    return Progress::Waiting
                }
                Some(Step::Write(Cmd::FxModelId { block, id }))
                    if view.effects.valid && view.effects.blocks[usize::from(*block)].valid
                        && view.effects.blocks[usize::from(*block)].params_loaded
                        && view.effects.blocks[usize::from(*block)].model_id() == id.as_slice() =>
                {
                    self.steps.pop_front();
                    continue;
                }
                Some(Step::Write(Cmd::Block { slot, on }))
                    if view.effects.valid && view.effects.blocks[usize::from(*slot)].valid
                        && view.effects.blocks[usize::from(*slot)].on == *on =>
                {
                    self.steps.pop_front();
                    continue;
                }
                Some(_) => match self.steps.pop_front() {
                    Some(Step::Write(command)) => return Progress::Write(command),
                    Some(Step::ModelReady { .. }) => continue,
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
            Some((generation, Some(cab_generation)))
                if generation != view.generation
                    && (self.target.cab.is_none() || view.cab_generation != cab_generation)
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
        let mut plan = Plan::scene(snapshot.clone(), 1, 0).unwrap();
        view.effects.blocks[0].params_loaded = false;
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
