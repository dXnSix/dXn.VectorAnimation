// Holds each page of the check set at three moments and photographs it on
// the platform `nsh test --target` runs it on, into shots/:
// `vector-animation-check compare shots .` sets them beside the reference.
import { open, test } from "nsh/test";
import { scenes } from "./animations/index";

const PAGE = 12;

test("frames", async () => {
  const app = await open("./App.tsx");
  const pages = Math.ceil(scenes.length / PAGE);
  for (let page = 0; page < pages; page++) {
    const count = Math.min(PAGE, scenes.length - page * PAGE);
    await app.waitFor(`page ${page + 1} of ${pages}, ${count} loaded `, { timeout: 60000 });
    // The first hold after the app opens can take longer to show than the
    // wait below (the web's first page held at t = 0): one held first.
    await app.press("id:at-50");
    await app.wait(1500);
    for (const p of ["25", "50", "75"]) {
      await app.press(`id:at-${p}`);
      await app.wait(1500);
      await app.screenshot(`shots/frame-${page}-${p}.png`);
    }
    await app.press("id:play");
    if (page + 1 < pages) await app.press("id:next");
  }
}, 600000);
