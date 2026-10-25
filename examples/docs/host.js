// The page's host (`nsh.json` `web.host`): tells the app which example the
// page's address asks for, `?demo=<name>`, or none.
export function createHost() {
  return {
    call(name) {
      if (name === "demo") return new URLSearchParams(location.search).get("demo") ?? "";
      throw new Error(`no host function ${name}`);
    },
  };
}
