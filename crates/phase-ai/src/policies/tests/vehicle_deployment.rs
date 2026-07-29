//! Unit tests for `policies::vehicle_deployment` — CR 702.122a crewable-Vehicle
//! deployment. No `#[cfg(test)]` in SOURCE files; tests live here.

use std::sync::Arc;

use engine::ai_support::{ActionMetadata, AiDecisionContext, CandidateAction, TacticalClass};
use engine::game::zones::create_object;
use engine::types::actions::GameAction;
use engine::types::card_type::CoreType;
use engine::types::format::FormatConfig;
use engine::types::game_state::{CastPaymentMode, GameState, WaitingFor};
use engine::types::identifiers::{CardId, ObjectId};
use engine::types::keywords::Keyword;
use engine::types::player::PlayerId;
use engine::types::zones::Zone;

use crate::config::AiConfig;
use crate::context::AiContext;
use crate::features::vehicles::{VehiclesFeature, VEHICLES_FLOOR};
use crate::features::DeckFeatures;
use crate::policies::context::{PolicyContext, SearchDepth};
use crate::policies::registry::{
    PolicyId, PolicyReason, PolicyRegistry, PolicyVerdict, TacticalPolicy,
};
use crate::policies::vehicle_deployment::*;
use crate::session::AiSession;

const AI: PlayerId = PlayerId(0);

fn state() -> GameState {
    GameState::new(FormatConfig::standard(), 2, 42)
}

/// A Vehicle in hand with `Crew N`.
fn vehicle_in_hand(state: &mut GameState, crew: u32) -> (ObjectId, CardId) {
    let card_id = CardId(state.next_object_id);
    let id = create_object(state, card_id, AI, "Copter".to_string(), Zone::Hand);
    let obj = state.objects.get_mut(&id).unwrap();
    obj.card_types.core_types.push(CoreType::Artifact);
    obj.card_types.subtypes.push("Vehicle".to_string());
    obj.keywords.push(Keyword::Crew {
        power: crew,
        once_per_turn: None,
    });
    state.players[AI.0 as usize].hand.push_back(id);
    (id, card_id)
}

/// A non-Vehicle artifact in hand.
fn artifact_in_hand(state: &mut GameState) -> (ObjectId, CardId) {
    let card_id = CardId(state.next_object_id);
    let id = create_object(state, card_id, AI, "Signet".to_string(), Zone::Hand);
    let obj = state.objects.get_mut(&id).unwrap();
    obj.card_types.core_types.push(CoreType::Artifact);
    state.players[AI.0 as usize].hand.push_back(id);
    (id, card_id)
}

/// An untapped creature the AI controls, with `power`.
fn creature(state: &mut GameState, power: i32, controller: PlayerId) -> ObjectId {
    let card_id = CardId(state.next_object_id);
    let id = create_object(
        state,
        card_id,
        controller,
        "Bear".to_string(),
        Zone::Battlefield,
    );
    let obj = state.objects.get_mut(&id).unwrap();
    obj.card_types.core_types.push(CoreType::Creature);
    obj.power = Some(power);
    obj.toughness = Some(power.max(1));
    id
}

fn feature(commitment: f32) -> VehiclesFeature {
    VehiclesFeature {
        vehicle_count: 5,
        total_crew_cost: 10,
        crew_body_count: 10,
        total_crew_power: 20,
        commitment,
    }
}

fn session(commitment: f32) -> AiSession {
    let features = DeckFeatures {
        vehicles: feature(commitment),
        ..Default::default()
    };
    let mut session = AiSession::empty();
    session.features.insert(AI, features);
    session
}

fn context(config: &AiConfig, session: AiSession) -> AiContext {
    let mut context = AiContext::empty(&config.weights);
    context.session = Arc::new(session);
    context.player = AI;
    context
}

fn cast(object_id: ObjectId, card_id: CardId) -> CandidateAction {
    CandidateAction {
        action: GameAction::CastSpell {
            object_id,
            card_id,
            targets: Vec::new(),
            payment_mode: CastPaymentMode::default(),
        },
        metadata: ActionMetadata::for_actor(Some(AI), TacticalClass::Spell),
    }
}

fn ctx<'a>(
    state: &'a GameState,
    candidate: &'a CandidateAction,
    decision: &'a AiDecisionContext,
    context: &'a AiContext,
    config: &'a AiConfig,
) -> PolicyContext<'a> {
    PolicyContext {
        state,
        decision,
        candidate,
        ai_player: AI,
        config,
        context,
        cast_facts: None,
        search_depth: SearchDepth::Root,
    }
}

fn priority_decision(candidate: &CandidateAction) -> AiDecisionContext {
    AiDecisionContext {
        waiting_for: WaitingFor::Priority { player: AI },
        candidates: vec![candidate.clone()],
    }
}

fn score_of(verdict: PolicyVerdict) -> (f64, PolicyReason) {
    match verdict {
        PolicyVerdict::Score { delta, reason } => (delta, reason),
        PolicyVerdict::Reject { reason } => panic!("unexpected Reject: {reason:?}"),
    }
}

fn verdict_for(
    st: &GameState,
    obj: ObjectId,
    card: CardId,
    commitment: f32,
) -> (f64, PolicyReason) {
    let config = AiConfig::default();
    let context = context(&config, session(commitment));
    let candidate = cast(obj, card);
    let decision = priority_decision(&candidate);
    score_of(VehicleDeploymentPolicy.verdict(&ctx(st, &candidate, &decision, &context, &config)))
}

// ─── activation ──────────────────────────────────────────────────────────────

#[test]
fn activation_opts_out_below_floor() {
    let features = DeckFeatures {
        vehicles: feature(VEHICLES_FLOOR - 0.01),
        ..Default::default()
    };
    assert!(VehicleDeploymentPolicy
        .activation(&features, &state(), AI)
        .is_none());
}

#[test]
fn activation_opts_in_above_floor() {
    let features = DeckFeatures {
        vehicles: feature(0.8),
        ..Default::default()
    };
    assert_eq!(
        VehicleDeploymentPolicy.activation(&features, &state(), AI),
        Some(0.8)
    );
}

// ─── verdict ─────────────────────────────────────────────────────────────────

#[test]
fn crewable_vehicle_scores_positive() {
    let mut st = state();
    creature(&mut st, 3, AI);
    let (obj, card) = vehicle_in_hand(&mut st, 2);
    let (delta, reason) = verdict_for(&st, obj, card, 0.8);
    assert_eq!(reason.kind, "vehicle_deployment_crewable");
    assert!(delta > 0.0, "expected positive credit, got {delta}");
}

#[test]
fn uncrewable_vehicle_is_neutral_not_penalized() {
    // No bodies: the Vehicle would enter as a blank. Withholding the bonus is the
    // whole signal — this policy never vetoes a deployment.
    let mut st = state();
    let (obj, card) = vehicle_in_hand(&mut st, 3);
    let (delta, reason) = verdict_for(&st, obj, card, 0.8);
    assert_eq!(reason.kind, "vehicle_deployment_uncrewable");
    assert_eq!(delta, 0.0, "must never penalize, only withhold");
}

#[test]
fn crew_power_sums_across_multiple_bodies() {
    // CR 702.122a: "any number of other untapped creatures with TOTAL power N".
    let mut st = state();
    creature(&mut st, 1, AI);
    creature(&mut st, 1, AI);
    creature(&mut st, 1, AI);
    let (obj, card) = vehicle_in_hand(&mut st, 3);
    let (_, reason) = verdict_for(&st, obj, card, 0.8);
    assert_eq!(reason.kind, "vehicle_deployment_crewable");
}

#[test]
fn insufficient_total_power_is_uncrewable() {
    let mut st = state();
    creature(&mut st, 1, AI);
    creature(&mut st, 1, AI);
    let (_, reason) = {
        let (obj, card) = vehicle_in_hand(&mut st, 5);
        verdict_for(&st, obj, card, 0.8)
    };
    assert_eq!(reason.kind, "vehicle_deployment_uncrewable");
}

#[test]
fn tapped_creatures_do_not_crew() {
    // CR 702.122a requires UNTAPPED creatures.
    let mut st = state();
    let body = creature(&mut st, 4, AI);
    st.objects.get_mut(&body).unwrap().tapped = true;
    let (obj, card) = vehicle_in_hand(&mut st, 2);
    let (_, reason) = verdict_for(&st, obj, card, 0.8);
    assert_eq!(reason.kind, "vehicle_deployment_uncrewable");
}

#[test]
fn opponent_creatures_do_not_crew() {
    // CR 702.122a: creatures YOU control.
    let mut st = state();
    creature(&mut st, 5, PlayerId(1));
    let (obj, card) = vehicle_in_hand(&mut st, 2);
    let (_, reason) = verdict_for(&st, obj, card, 0.8);
    assert_eq!(reason.kind, "vehicle_deployment_uncrewable");
}

#[test]
fn non_vehicle_is_not_applicable() {
    let mut st = state();
    creature(&mut st, 5, AI);
    let (obj, card) = artifact_in_hand(&mut st);
    let (delta, reason) = verdict_for(&st, obj, card, 0.8);
    assert_eq!(reason.kind, "vehicle_deployment_na");
    assert_eq!(delta, 0.0);
}

#[test]
fn surplus_credit_is_bounded_by_the_cap() {
    let mut st = state();
    for _ in 0..12 {
        creature(&mut st, 5, AI);
    }
    let (obj, card) = vehicle_in_hand(&mut st, 1);
    let (delta, _) = verdict_for(&st, obj, card, 0.8);
    let config = AiConfig::default();
    let ceiling = config.policy_penalties.vehicle_deployment_bonus * 2.0;
    assert!(
        delta <= ceiling + f64::EPSILON,
        "delta {delta} exceeded ceiling {ceiling}"
    );
}

#[test]
fn exact_crew_requirement_is_crewable() {
    // Boundary: total power exactly equals N (CR 702.122a says "N or greater").
    let mut st = state();
    creature(&mut st, 2, AI);
    let (obj, card) = vehicle_in_hand(&mut st, 2);
    let (_, reason) = verdict_for(&st, obj, card, 0.8);
    assert_eq!(reason.kind, "vehicle_deployment_crewable");
}

// ─── production seam ─────────────────────────────────────────────────────────

#[test]
fn registry_routes_cast_spell_to_this_policy() {
    let mut st = state();
    creature(&mut st, 3, AI);
    let (obj, card) = vehicle_in_hand(&mut st, 2);
    let config = AiConfig::default();
    let context = context(&config, session(0.8));
    let candidate = cast(obj, card);
    let decision = priority_decision(&candidate);
    let verdicts =
        PolicyRegistry::default().verdicts(&ctx(&st, &candidate, &decision, &context, &config));
    let found = verdicts
        .iter()
        .find(|(id, _)| *id == PolicyId::VehicleDeployment)
        .map(|(_, v)| v.clone())
        .expect("VehicleDeploymentPolicy must be registered and routed for CastSpell");
    let (delta, reason) = score_of(found);
    assert_eq!(reason.kind, "vehicle_deployment_crewable");
    assert!(delta > 0.0);
}

#[test]
fn registry_stays_silent_below_the_activation_floor() {
    let mut st = state();
    creature(&mut st, 3, AI);
    let (obj, card) = vehicle_in_hand(&mut st, 2);
    let config = AiConfig::default();
    let context = context(&config, session(VEHICLES_FLOOR - 0.01));
    let candidate = cast(obj, card);
    let decision = priority_decision(&candidate);
    let verdicts =
        PolicyRegistry::default().verdicts(&ctx(&st, &candidate, &decision, &context, &config));
    assert!(
        !verdicts
            .iter()
            .any(|(id, _)| *id == PolicyId::VehicleDeployment),
        "policy must not contribute below its activation floor"
    );
}
