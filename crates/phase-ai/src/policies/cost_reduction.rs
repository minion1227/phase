//! `CostReductionPolicy` — makes a cost-reduction permanent a reason the AI can
//! see to deploy the discount BEFORE the spells it discounts.
//!
//! ## The gap this closes
//!
//! CR 601.2f: Goblin Electromancer, Baral, Foundry Inspector and the Medallion
//! cycle are acceleration that never taps for mana — every later spell costs
//! less for as long as the permanent survives. The engine already applies the
//! discount at cast time (`casting::collect_battlefield_cost_modifiers`), so the AI is never
//! overcharged; what it lacks is any reason to *sequence the reducer first*.
//! `RampTimingPolicy` supplies exactly that signal for permanents that add mana
//! (`Effect::Mana`, land fetch, extra land drops) and structurally cannot see a
//! cost reducer, so a deck whose entire acceleration plan is cost reduction gets
//! no sequencing guidance at all. This policy adds it.
//!
//! ## Performance
//!
//! `verdict()` runs per candidate per search node. The card-local check — do
//! this candidate's OWN statics carry a board-wide reduction of your spells —
//! runs FIRST and rejects every non-reducer candidate after reading one card's
//! AST. Only a confirmed reducer pays for the hand walk, which is bounded by
//! hand size and touches no battlefield sweep, no `find_legal_targets`, and no
//! affordability query.

use engine::types::card_type::CoreType;
use engine::types::game_state::GameState;
use engine::types::identifiers::ObjectId;
use engine::types::player::PlayerId;

use crate::features::cost_reduction::{your_spell_discount_parts, COST_REDUCTION_FLOOR};
use crate::features::DeckFeatures;

use super::context::PolicyContext;
use super::registry::{DecisionKind, PolicyId, PolicyReason, PolicyVerdict, TacticalPolicy};

pub struct CostReductionPolicy;

/// Cap on how many future casts one deployment is credited for, so a full grip
/// cannot push a single reducer out of the intended band.
///
/// `pub(crate)` so the bounded-score regression asserts against this constant
/// rather than a copied literal — raising the cap must move the test with it.
pub(crate) const MAX_REWARDED_FUTURE_CASTS: u32 = 4;

/// Cap on the per-application generic discount credited, so a misparsed or
/// unusually large `amount` cannot dominate the candidate's prior.
pub(crate) const MAX_REWARDED_DISCOUNT: u32 = 3;

impl TacticalPolicy for CostReductionPolicy {
    fn id(&self) -> PolicyId {
        PolicyId::CostReduction
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
        if features.cost_reduction.commitment < COST_REDUCTION_FLOOR {
            None
        } else {
            Some(features.cost_reduction.commitment)
        }
    }

    fn verdict(&self, ctx: &PolicyContext<'_>) -> PolicyVerdict {
        let Some(facts) = ctx.cast_facts() else {
            // CR 601.2 cast-shaped siblings (madness, miracle, foretell, copies)
            // do not populate `cast_facts`, so there is no AST to classify.
            return PolicyVerdict::neutral(PolicyReason::new("cost_reduction_na"));
        };

        // Card-local first: does THIS candidate carry the discount engine?
        let discount = your_spell_discount_parts(facts.object.static_definitions.iter_unchecked());
        if discount > 0 {
            // A discount only pays off on spells still to be cast. With an empty
            // grip the reducer is a vanilla permanent this turn.
            let future_casts = castable_cards_in_hand(ctx, Some(facts.object.id));
            if future_casts == 0 {
                return PolicyVerdict::neutral(PolicyReason::new("cost_reduction_no_future_casts"));
            }
            let rewarded_casts = future_casts.min(MAX_REWARDED_FUTURE_CASTS);
            let rewarded_discount = discount.min(MAX_REWARDED_DISCOUNT);
            // CR 601.2f: each future cast saves `discount` generic mana; the
            // configured weight converts saved mana into card-equivalents.
            let saved_mana = f64::from(rewarded_discount * rewarded_casts);
            return PolicyVerdict::score(
                ctx.config.policy_penalties.cost_reduction_deploy_bonus * saved_mana,
                PolicyReason::new("cost_reduction_deploy_engine")
                    .with_fact("discount", i64::from(discount))
                    .with_fact("future_casts", i64::from(future_casts)),
            );
        }

        // Otherwise: are we casting past an unplayed reducer that is cheaper than
        // this spell? Deploying the discount first is strictly better sequencing
        // — the same shape as `RampTimingPolicy`'s `defer_to_ramp`. The mana-value
        // gate keeps this to cases where the reducer plausibly fits first.
        if hand_holds_cheaper_reducer(ctx, facts.mana_value, facts.object.id) {
            return PolicyVerdict::score(
                ctx.config.policy_penalties.cost_reduction_defer_penalty,
                PolicyReason::new("cost_reduction_defer_to_engine")
                    .with_fact("mana_value", i64::from(facts.mana_value)),
            );
        }

        PolicyVerdict::neutral(PolicyReason::new("cost_reduction_na"))
    }
}

/// Nonland cards in the AI's hand that a discount could still apply to,
/// excluding `exclude` (the candidate itself, which is being spent now).
///
/// Lands are excluded because CR 305.1 land plays are not spells and are never
/// discounted by a CR 601.2f cost reducer.
///
/// This counts remaining casts, NOT the subset the reducer's `spell_filter`
/// admits — narrowing per candidate would mean evaluating that filter against
/// every card in hand inside the search inner loop. The narrowing is applied
/// once per game instead: `CostReductionFeature::commitment` folds in the
/// deck-wide fraction of cards the reducers actually discount, and the registry
/// multiplies this verdict by that commitment via `activation`. So a deck whose
/// reducers cover little of its own list is already scaled down here, and a
/// reducer that covers nothing deactivates the policy outright.
fn castable_cards_in_hand(ctx: &PolicyContext<'_>, exclude: Option<ObjectId>) -> u32 {
    let Some(player) = ctx.state.players.get(ctx.ai_player.0 as usize) else {
        return 0;
    };
    player
        .hand
        .iter()
        .filter(|id| Some(**id) != exclude)
        .filter(|id| {
            ctx.state
                .objects
                .get(id)
                .is_some_and(|obj| !obj.card_types.core_types.contains(&CoreType::Land))
        })
        .count()
        .try_into()
        .unwrap_or(u32::MAX)
}

/// True when the AI's hand still holds a cost-reduction permanent whose mana
/// value is strictly below `mana_value` — i.e. the engine could have been
/// deployed instead of, and later alongside, this spell.
fn hand_holds_cheaper_reducer(ctx: &PolicyContext<'_>, mana_value: u32, exclude: ObjectId) -> bool {
    let Some(player) = ctx.state.players.get(ctx.ai_player.0 as usize) else {
        return false;
    };
    player.hand.iter().any(|id| {
        *id != exclude
            && ctx.state.objects.get(id).is_some_and(|obj| {
                obj.effective_mana_value() < mana_value
                    && your_spell_discount_parts(obj.static_definitions.iter_unchecked()) > 0
            })
    })
}
