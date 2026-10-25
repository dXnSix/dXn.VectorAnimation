// The page's host (`nsh.json` `web.host`): tells the app which example the
// page's address asks for, `?demo=<name>`, or none, and passes on the height
// of its content to the page that frames it (the docs site sizes the frame to it).
export function createHost() {
  return {
    call(name, args) {
      if (name === "demo") return new URLSearchParams(location.search).get("demo") ?? "";
      if (name === "height") {
        if (parent !== window) parent.postMessage({ type: "dxn-example-height", height: args }, location.origin);
        return null;
      }
      throw new Error(`no host function ${name}`);
    },
  };
}
