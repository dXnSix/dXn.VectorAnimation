// The page's host (`nsh.json` `web.host`): tells the app which example the
// page's address asks for, `?demo=<name>`, or none, and passes on the height
// of its content to the page that frames it (the docs site sizes the frame to it,
// and the page, not the frame, then takes the scrolling).
// The frame is as tall as the content, so the app has nothing to scroll: NSH's
// scroll views contain their overscroll (`overscroll-behavior: contain`), which
// would keep the wheel and touch scrolling over the frame from reaching the page.
let lifted = false;
function fitted() {
  if (lifted) return;
  lifted = true;
  const style = document.createElement("style");
  style.textContent = ".nsh-root .nsh-scroll { overscroll-behavior: auto; }";
  document.head.append(style);
}

export function createHost() {
  return {
    call(name, args) {
      if (name === "demo") return new URLSearchParams(location.search).get("demo") ?? "";
      if (name === "height") {
        if (parent !== window) {
          parent.postMessage({ type: "dxn-example-height", height: args }, location.origin);
          fitted();
        }
        return null;
      }
      throw new Error(`no host function ${name}`);
    },
  };
}
