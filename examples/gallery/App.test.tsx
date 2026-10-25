// The gallery with `nsh test`: the TSX face of the crate (its props, events
// and types) in the headless runner `nsh` builds with it.
import { expect, open, test } from "nsh/test";

test("both animations load and the Lottie one ends", async () => {
  const app = await open("./App.tsx");
  await app.waitFor("Figma: 2.0 s, 200 × 100");
  await app.waitFor("Lottie: 2.0 s, 100 × 100");
  await app.waitFor("Ended 1 times");
});

test("pausing and scrubbing", async () => {
  const app = await open("./App.tsx");
  await app.press("Pause");
  await app.waitFor("Play");
  await app.press("Scrub to half");
  await app.press("Release");
  expect(app.shows("Play")).toBe(true);
});
