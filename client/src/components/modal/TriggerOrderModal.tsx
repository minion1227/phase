import { useCallback, useEffect, useState } from "react";

import type { PendingTriggerSummary } from "../../adapter/types.ts";
import { useGameStore } from "../../stores/gameStore.ts";
import { DialogShell } from "./DialogShell.tsx";

const EMPTY_TRIGGER_SUMMARIES: PendingTriggerSummary[] = [];

/**
 * CR 603.3b: Surfaced when the local player must choose the order in which
 * their simultaneously-triggered abilities are placed on the stack. The engine
 * owns ALL ordering logic — this component only permutes an index array (the
 * `triggers` payload the engine provided) and dispatches the chosen
 * permutation via `GameAction::OrderTriggers`. No rules computation; no
 * re-derivation from `state.objects`. Labels at the list ends are pure
 * display formatting of CR 405.3 (LIFO).
 */
export function TriggerOrderModal() {
  const waitingFor = useGameStore((s) => s.waitingFor);
  const dispatch = useGameStore((s) => s.dispatch);

  const isOrderTriggers = waitingFor?.type === "OrderTriggers";
  const engineTriggers = isOrderTriggers
    ? waitingFor.data.triggers
    : EMPTY_TRIGGER_SUMMARIES;

  // Local UI state: the chosen permutation (indices into engineTriggers).
  // Starts as identity. Reset whenever the engine sends a new prompt because
  // successive CR 603.3b groups can have the same trigger count.
  const [order, setOrder] = useState<number[]>(() =>
    engineTriggers.map((_, i) => i),
  );
  useEffect(() => {
    setOrder(engineTriggers.map((_, i) => i));
  }, [engineTriggers, isOrderTriggers]);

  const move = useCallback((from: number, to: number) => {
    setOrder((prev) => {
      if (to < 0 || to >= prev.length) return prev;
      const next = prev.slice();
      const [item] = next.splice(from, 1);
      next.splice(to, 0, item);
      return next;
    });
  }, []);

  const handleConfirm = useCallback(() => {
    dispatch({ type: "OrderTriggers", data: { order } });
  }, [dispatch, order]);

  if (!isOrderTriggers || engineTriggers.length === 0) return null;

  return (
    <DialogShell
      eyebrow="Resolution Order"
      title="Order Your Triggered Abilities"
      subtitle="Choose the order in which these triggers go on the stack. The top of this list resolves last (CR 405.3)."
      size="md"
      scrollable
      footer={
        <button
          type="button"
          onClick={handleConfirm}
          className="min-h-11 rounded-[16px] bg-cyan-500/80 px-5 py-3 font-semibold text-white transition hover:bg-cyan-500"
        >
          Confirm Order
        </button>
      }
    >
      <div className="px-3 py-3 lg:px-5 lg:py-5">
        <div className="mb-2 text-xs uppercase tracking-wide text-white/50">
          Resolves last (bottom of stack)
        </div>
        <ol className="flex flex-col gap-2">
          {order.map((engineIndex, position) => {
            const trigger = engineTriggers[engineIndex];
            return (
              <li
                key={`${trigger.source_id}-${engineIndex}`}
                className="flex items-start gap-2 rounded-[16px] border border-white/8 bg-white/5 px-4 py-3"
              >
                <div className="flex-1 text-left">
                  <div className="font-semibold text-white">
                    {trigger.source_name || `Trigger ${engineIndex + 1}`}
                  </div>
                  {trigger.description && (
                    <div className="text-sm text-white/70">
                      {trigger.description}
                    </div>
                  )}
                </div>
                <div className="flex flex-col gap-1">
                  <button
                    type="button"
                    aria-label="Move up"
                    disabled={position === 0}
                    onClick={() => move(position, position - 1)}
                    className="min-h-8 rounded border border-white/10 px-2 text-white/80 transition hover:bg-white/10 disabled:opacity-30"
                  >
                    ▲
                  </button>
                  <button
                    type="button"
                    aria-label="Move down"
                    disabled={position === order.length - 1}
                    onClick={() => move(position, position + 1)}
                    className="min-h-8 rounded border border-white/10 px-2 text-white/80 transition hover:bg-white/10 disabled:opacity-30"
                  >
                    ▼
                  </button>
                </div>
              </li>
            );
          })}
        </ol>
        <div className="mt-2 text-xs uppercase tracking-wide text-white/50">
          Resolves first (top of stack)
        </div>
      </div>
    </DialogShell>
  );
}
