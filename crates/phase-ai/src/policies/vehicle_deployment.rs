//! `VehicleDeploymentPolicy` — value casting a Vehicle against whether the board
//! can actually crew it.
//!
//! ## The gap this closes
//!
//! CR 702.122a: an uncrewed Vehicle is not a creature. It sits on the battlefield
//! doing nothing until its controller taps *other* untapped creatures with total
//! power N or greater. So the same card is a threat on a developed board and a
//! blank on an empty one — and nothing in the AI distinguishes those two casts.
//!
//! `CrewTimingPolicy` decides whether a specific crew activation is worth it,
//! which is the question *after* the Vehicle is already down. This policy asks
//! the earlier one: is deploying it now going to produce a body, or a brick?
//!
//! It is deliberately one-directional in the same way `DrawPayoffPolicy` is: it
//! ADDS value when the crew requirement is already met and withholds it
//! otherwise. It never vetoes a Vehicle cast — holding a Vehicle for a board that
//! may never arrive is its own mistake, and the mana-efficiency and board-
//! development policies already price deploying a permanent.
//!
//! ## Performance
//!
//! `verdict()` runs per candidate per search node. The card-local check — does
//! this candidate carry a crew requirement at all — reads one card's keywords and
//! rejects every non-Vehicle candidate. Only a confirmed Vehicle pays for the
//! battlefield walk, which is bounded by the AI's own permanents and delegates
//! per-creature power to the engine's `object_crew_power_contribution` authority.
//! No affordability sweep, no `find_legal_targets`.

use engine::game::static_abilities::{object_crew_power_contribution, object_has_cant_crew};
use engine::types::actions::GameAction;
use engine::types::card_type::CoreType;
use engine::types::game_state::GameState;
use engine::types::player::PlayerId;
use engine::types::statics::CrewAction;

use crate::features::vehicles::{crew_requirement_parts, VEHICLES_FLOOR};
use crate::features::DeckFeatures;

use super::context::PolicyContext;
use super::registry::{DecisionKind, PolicyId, PolicyReason, PolicyVerdict, TacticalPolicy};

pub struct VehicleDeploymentPolicy;

/// Cap on the surplus crew power rewarded, so a huge board cannot push one
/// Vehicle cast out of the preference band.
///
/// `pub(crate)` so the bounded-score regression asserts against this constant
/// rather than a copied literal.
pub(crate) const MAX_REWARDED_SURPLUS: i32 = 3;

impl TacticalPolicy for VehicleDeploymentPolicy {
    fn id(&self) -> PolicyId {
        PolicyId::VehicleDeployment
    }

    fn decision_kinds(&self) -> &'static [DecisionKind] {
        &[DecisionKind::CastSpell]
    }

    fn activation(
        &self,
        features: &DeckFeatures,
        _state: &GameState,
        _player: PlayerId,
    ) -> Option<f32> {
        if features.vehicles.commitment < VEHICLES_FLOOR {
            None
        } else {
            Some(features.vehicles.commitment)
        }
    }

    fn verdict(&self, ctx: &PolicyContext<'_>) -> PolicyVerdict {
        // Card-local first: is this candidate even a Vehicle?
        let Some(facts) = ctx.cast_facts() else {
            return PolicyVerdict::neutral(PolicyReason::new("vehicle_deployment_na"));
        };
        if !matches!(ctx.candidate.action, GameAction::CastSpell { .. }) {
            return PolicyVerdict::neutral(PolicyReason::new("vehicle_deployment_na"));
        }
        // CR 702.122a: Crew is an ACTIVATED ABILITY. A Vehicle subtype without
        // the keyword grants no crew ability, so it has no requirement to meet —
        // routing through the strict authority keeps a `Crew 0` from being
        // synthesised and trivially satisfied by an empty board.
        let Some(crew_cost) = crew_requirement_parts(facts.object.keywords.iter()) else {
            return PolicyVerdict::neutral(PolicyReason::new("vehicle_deployment_na"));
        };

        // Only now pay for the board walk. CR 702.122a: crew taps OTHER untapped
        // creatures the controller owns, so an already-tapped body, a creature
        // under a `CantCrew` static (CR 702.122d), and the Vehicle itself all
        // contribute nothing. Per-creature power goes through the engine's
        // authority so a Vehicle crewed "as though its power were N greater", or
        // by toughness instead of power, is counted the way the crew payment
        // itself would count it.
        let available: i32 = ctx
            .state
            .battlefield
            .iter()
            .filter_map(|id| {
                let obj = ctx.state.objects.get(id)?;
                if obj.controller != ctx.ai_player
                    || obj.tapped
                    || !obj.card_types.core_types.contains(&CoreType::Creature)
                {
                    return None;
                }
                if object_has_cant_crew(ctx.state, *id) {
                    return None;
                }
                Some(object_crew_power_contribution(
                    ctx.state,
                    *id,
                    CrewAction::Crew,
                ))
            })
            .sum();

        let required = i32::try_from(crew_cost).unwrap_or(i32::MAX);
        if available < required {
            // The Vehicle would enter as a blank. No penalty — see the module
            // docs: withholding the bonus is the whole signal.
            return PolicyVerdict::neutral(
                PolicyReason::new("vehicle_deployment_uncrewable")
                    .with_fact("required", i64::from(required))
                    .with_fact("available", i64::from(available)),
            );
        }

        // The board can turn this into a creature the turn it lands. Scale mildly
        // with surplus power, because spare bodies mean crewing costs less of the
        // attack step.
        let surplus = (available - required).min(MAX_REWARDED_SURPLUS);
        let scaled = 1.0 + f64::from(surplus) / f64::from(MAX_REWARDED_SURPLUS);
        PolicyVerdict::score(
            ctx.config.policy_penalties.vehicle_deployment_bonus * scaled,
            PolicyReason::new("vehicle_deployment_crewable")
                .with_fact("required", i64::from(required))
                .with_fact("available", i64::from(available)),
        )
    }
}
