import { beforeEach, describe, expect, it } from "vitest";

import {
  loadSavedDeck,
  loadSavedDeckBracket,
  saveSavedDeckBracket,
  STORAGE_KEY_PREFIX,
} from "../storage";
import { expandParsedDeck } from "../../services/deckParser";

beforeEach(() => {
  localStorage.clear();
});

describe("saved-deck bracket sidecar", () => {
  it("preserves sticker sheets when loading and expanding a saved deck", () => {
    localStorage.setItem(
      STORAGE_KEY_PREFIX + "Sticker Deck",
      JSON.stringify({
        main: [{ count: 1, name: "Sol Ring" }],
        sideboard: [],
        sticker_sheets: ["sheet-1", "sheet-2", "sheet-3"],
      }),
    );

    const loaded = loadSavedDeck("Sticker Deck");

    expect(loaded?.sticker_sheets).toEqual(["sheet-1", "sheet-2", "sheet-3"]);
    expect(loaded && expandParsedDeck(loaded).sticker_sheets).toEqual(["sheet-1", "sheet-2", "sheet-3"]);
  });

  it("returns null when the deck does not exist", () => {
    expect(loadSavedDeckBracket("Missing Deck")).toBeNull();
  });

  it("returns null when the persisted JSON has no bracket field", () => {
    localStorage.setItem(
      STORAGE_KEY_PREFIX + "Untagged",
      JSON.stringify({ main: [], sideboard: [], format: "Commander" }),
    );
    expect(loadSavedDeckBracket("Untagged")).toBeNull();
  });

  it("returns the bracket when persisted", () => {
    localStorage.setItem(
      STORAGE_KEY_PREFIX + "Tagged",
      JSON.stringify({ main: [], sideboard: [], format: "Commander", bracket: 3 }),
    );
    expect(loadSavedDeckBracket("Tagged")).toBe(3);
  });

  it("returns null when the persisted bracket is invalid (e.g. 0 or 'x')", () => {
    localStorage.setItem(
      STORAGE_KEY_PREFIX + "Bad",
      JSON.stringify({ main: [], sideboard: [], format: "Commander", bracket: 0 }),
    );
    expect(loadSavedDeckBracket("Bad")).toBeNull();
  });

  it("saveSavedDeckBracket merges the bracket into the existing persisted JSON", () => {
    localStorage.setItem(
      STORAGE_KEY_PREFIX + "Existing",
      JSON.stringify({ main: [{ count: 1, name: "Sol Ring" }], sideboard: [], format: "Commander" }),
    );
    saveSavedDeckBracket("Existing", 4);
    const raw = localStorage.getItem(STORAGE_KEY_PREFIX + "Existing")!;
    const parsed = JSON.parse(raw);
    expect(parsed.bracket).toBe(4);
    // Pre-existing fields must be preserved.
    expect(parsed.main).toEqual([{ count: 1, name: "Sol Ring" }]);
    expect(parsed.format).toBe("Commander");
  });

  it("saveSavedDeckBracket with null removes any existing bracket field", () => {
    localStorage.setItem(
      STORAGE_KEY_PREFIX + "Existing",
      JSON.stringify({ main: [], sideboard: [], format: "Commander", bracket: 4 }),
    );
    saveSavedDeckBracket("Existing", null);
    const parsed = JSON.parse(localStorage.getItem(STORAGE_KEY_PREFIX + "Existing")!);
    expect("bracket" in parsed).toBe(false);
  });

  it("saveSavedDeckBracket is a no-op when the deck does not exist", () => {
    saveSavedDeckBracket("Missing", 3);
    expect(localStorage.getItem(STORAGE_KEY_PREFIX + "Missing")).toBeNull();
  });
});
