// The examples the documentation site embeds. The page's address names one,
// `?demo=<name>` (the web host, host.js, reads it): it fills the frame, with
// its controls. Without one, every example shows, a grid of tiles.
//
// The scenes are the check set (`vector-animation-check scenes examples/docs
// --gallery fixtures/gallery` writes animations/), and two of this app's own:
// `playback` and `events`. The gallery's files, compiled ahead, show as grids:
// `lottie-files`, `dotlottie-files` and `svg-files`; `file-<group>-<name>`
// plays one of them alone on the page's background (the home page's hero).
// The Rive files (`--rive fixtures/docs`, written as their bytes) play live:
// `rive-switch` (a listener), `rive-events` (the cues it fires) and
// `rive-progress` (a view model set from the page); `rive-files`, a grid of
// community files fetched from beside the app (`rive/`, `--rive-files`).
import { mount, Scroll, signal, Text, View } from "nsh";
import { Button } from "nsh/base";
import { host } from "nsh/host";
import { VectorAnimation, type VectorAnimationRef } from "dxn-vector-animation";
import { scenes } from "./animations/index";
import { gallery } from "./animations/gallery";
import { rive } from "./animations/rive";
import { riveFiles } from "./animations/rive-files";

// Every example plays under reduced motion too (`ignoreReduceMotion`): a reader whose system turns
// animation effects off, as Windows' setting does, would otherwise see only the first frame. An app
// gets the still frame by default and opts in the same way.
const named = new Map<string, unknown>(scenes as [string, unknown][]);

// A button that plays on a click: SMIL's `begin="click"`, the event
// `btn.click`, which the ref's `trigger` plays.
const CLICKED = `<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100" viewBox="0 0 200 100">
  <rect width="200" height="100" fill="#F4F1EA"/>
  <rect id="btn" x="20" y="34" width="60" height="32" rx="16" fill="#5B8DEF">
    <animate attributeName="x" from="20" to="120" dur="0.6s" begin="click" fill="freeze" calcMode="spline" keyTimes="0;1" keySplines="0.4 0 0.2 1"/>
    <animate attributeName="fill" from="#5B8DEF" to="#2EAD5A" dur="0.6s" begin="click" fill="freeze"/>
  </rect>
</svg>`;

// Tells the page that frames this one that the player has its file, which starts the next frame there.
const ready = () => {
  host.call("ready").catch(() => {});
};

const BAR = { flexDirection: "row", flexWrap: "wrap", gap: 6, justifyContent: "center" } as const;

function Control(props: { title: string; onPress: () => void; selected?: boolean }) {
  return (
    <Button
      onPress={props.onPress}
      style={{
        backgroundColor: props.selected ? "#1f2937" : "#e5e7eb",
        borderRadius: 7,
        paddingHorizontal: 10,
        paddingVertical: 5,
      }}
      activeStyle={{ opacity: 0.7 }}
    >
      <Text style={{ fontSize: 12, fontWeight: "600", color: props.selected ? "#ffffff" : "#111827" }}>
        {props.title}
      </Text>
    </Button>
  );
}

const RATES = [0.5, 1, 2];

/** One scene filling the frame, with pause and speed. */
function Single(props: { src: unknown; name: string }) {
  const paused = signal(false);
  const rate = signal(1);
  return (
    <View style={{ flex: 1, padding: 10, gap: 8 }}>
      <View style={{ flex: 1 }}>
        <VectorAnimation
          ignoreReduceMotion
          src={props.src as never}
          style={{ width: "100%", height: "100%" }}
          paused={paused()}
          rate={rate()}
          accessibilityLabel={props.name}
          onLoad={ready}
        />
      </View>
      <View style={BAR}>
        <Control title={paused() ? "Play" : "Pause"} onPress={() => paused.set(!paused())} />
        {RATES.map((r) => (
          <Control title={`${r}×`} selected={rate() === r} onPress={() => rate.set(r)} />
        ))}
      </View>
    </View>
  );
}

/** The Figma Motion export with every playback control. */
function Playback() {
  const paused = signal(false);
  const rate = signal(1);
  const direction = signal<"normal" | "reverse" | "alternate">("normal");
  const progress = signal<number | undefined>(undefined);
  return (
    <View style={{ flex: 1, padding: 10, gap: 8 }}>
      <View style={{ flex: 1 }}>
        <VectorAnimation
          ignoreReduceMotion
          src={named.get("figma") as never}
          style={{ width: "100%", height: "100%" }}
          paused={paused()}
          rate={rate()}
          direction={direction()}
          progress={progress()}
          accessibilityLabel="A button popping in"
          onLoad={ready}
        />
      </View>
      <View style={BAR}>
        <Control title={paused() ? "Play" : "Pause"} onPress={() => paused.set(!paused())} />
        {RATES.map((r) => (
          <Control title={`${r}×`} selected={rate() === r} onPress={() => rate.set(r)} />
        ))}
        {(["normal", "reverse", "alternate"] as const).map((d) => (
          <Control title={d} selected={direction() === d} onPress={() => direction.set(d)} />
        ))}
      </View>
      <View style={BAR}>
        {[0, 0.25, 0.5, 0.75, 1].map((p) => (
          <Control title={`${p * 100}%`} selected={progress() === p} onPress={() => progress.set(p)} />
        ))}
        <Control title="Release" onPress={() => progress.set(undefined)} />
      </View>
    </View>
  );
}

/** What a file plays on an event, played by the ref's `trigger`. */
function Events() {
  let player: VectorAnimationRef | undefined;
  const events = signal("");
  return (
    <View style={{ flex: 1, padding: 10, gap: 8 }}>
      <View style={{ flex: 1 }}>
        <VectorAnimation
          ignoreReduceMotion
          src={CLICKED}
          style={{ width: "100%", height: "100%" }}
          ref={(r: VectorAnimationRef) => (player = r)}
          onLoad={(info) => {
            events.set(info.events.join(", "));
            ready();
          }}
          accessibilityLabel="A button that slides over when clicked"
        />
      </View>
      <View style={BAR}>
        <Control title="Trigger btn.click" onPress={() => player?.trigger("btn.click")} />
        <Text style={{ fontSize: 12, opacity: 0.6, alignSelf: "center" }}>{`events: ${events()}`}</Text>
      </View>
    </View>
  );
}

const riveDemos = new Map<string, Uint8Array>(rive);

/** A Rive switch: a click on it toggles its state machine's input "on", as does a button here. */
function RiveSwitch() {
  let player: VectorAnimationRef | undefined;
  const on = signal(false);
  return (
    <View style={{ flex: 1, padding: 10, gap: 8 }}>
      <View style={{ flex: 1 }}>
        <VectorAnimation
          ignoreReduceMotion
          src={riveDemos.get("switch") as never}
          style={{ width: "100%", height: "100%" }}
          ref={(r: VectorAnimationRef) => (player = r)}
          onLoad={ready}
          onInput={(i) => on.set(i.value === 1)}
          accessibilityLabel="A switch that turns on and off when clicked"
        />
      </View>
      <View style={BAR}>
        <Control
          title={on() ? 'setInput("on", false)' : 'setInput("on", true)'}
          onPress={() => {
            player?.setInput("on", !on());
            on.set(!on());
          }}
        />
        <Text style={{ fontSize: 12, opacity: 0.6, alignSelf: "center" }}>{`on: ${on()}`}</Text>
      </View>
    </View>
  );
}

/** A Rive ball firing events as it travels: the cues playback passes, newest first. */
function RiveEvents() {
  const cues = signal<string[]>([]);
  return (
    <View style={{ flex: 1, padding: 10, gap: 8 }}>
      <View style={{ flex: 1 }}>
        <VectorAnimation
          ignoreReduceMotion
          src={riveDemos.get("travel") as never}
          style={{ width: "100%", height: "100%" }}
          onLoad={ready}
          onCue={(c) => cues.set([`${c.name} at ${c.time.toFixed(2)} s`, ...cues()].slice(0, 4))}
          accessibilityLabel="A ball crossing back and forth"
        />
      </View>
      <View style={BAR}>
        <Text style={{ fontSize: 12, opacity: 0.6 }}>{`onCue: ${cues().join(" · ") || "…"}`}</Text>
      </View>
    </View>
  );
}

const TINTS: [string, string][] = [
  ["blue", "#3b82f6"],
  ["green", "#22c55e"],
  ["orange", "#f97316"],
];

/** A Rive progress bar bound to a view model: its value, tint and trigger set from here. */
function RiveProgress() {
  let player: VectorAnimationRef | undefined;
  const value = signal(30);
  const tint = signal("blue");
  const last = signal("");
  return (
    <View style={{ flex: 1, padding: 10, gap: 8 }}>
      <View style={{ flex: 1 }}>
        <VectorAnimation
          ignoreReduceMotion
          src={riveDemos.get("progress") as never}
          style={{ width: "100%", height: "100%" }}
          ref={(r: VectorAnimationRef) => (player = r)}
          onLoad={(info) => {
            last.set(info.values.map((v) => `${v.path}: ${v.kind}`).join(", "));
            ready();
          }}
          onValue={(v) => last.set(`onValue ${v.path} ${v.value === null ? "fired" : v.value}`)}
          accessibilityLabel="A progress bar"
        />
      </View>
      <View style={BAR}>
        {[10, 50, 90, 100].map((v) => (
          <Control
            title={`value ${v}`}
            selected={value() === v}
            onPress={() => {
              value.set(v);
              player?.setValue("value", v);
            }}
          />
        ))}
      </View>
      <View style={BAR}>
        {TINTS.map(([name, hex]) => (
          <Control
            title={name}
            selected={tint() === name}
            onPress={() => {
              tint.set(name);
              player?.setValue("tint", hex);
            }}
          />
        ))}
        <Control title={'fireValue("celebrate")'} onPress={() => player?.fireValue("celebrate")} />
      </View>
      <Text style={{ fontSize: 12, opacity: 0.6, textAlign: "center" }}>{last()}</Text>
    </View>
  );
}

/**
 * The Rive community files, a tile each, fetched from beside the app and
 * played as they are (a pointer over one reaches its listeners); the frame
 * is as tall as the grid.
 */
function RiveFiles() {
  // Each file's URL beside the page, made absolute by the page's host.
  const urls = signal<string[] | undefined>(undefined);
  // The tiles given their file so far: one after another, as the web reads
  // a file on the page's one thread (a few seconds for the largest).
  const shown = signal(1);
  const next = (i: number) => shown.set(Math.max(shown(), i + 2));
  Promise.all(riveFiles.map(([, url]) => host.call("url", url).then((u) => String(u))))
    .then((list) => urls.set(list))
    .catch(() => urls.set([]));
  return (
    <View
      style={{ padding: 12, flexDirection: "row", flexWrap: "wrap", gap: 14, justifyContent: "center" }}
      onLayout={(e) => {
        host.call("height", Math.ceil(e.size.height)).catch(() => {});
      }}
    >
      {(urls() ?? []).map((url, i) => {
        const name = riveFiles[i]?.[0] ?? url;
        const error = signal("");
        return (
          <View style={{ width: 240, flexShrink: 0, gap: 4, alignItems: "center" }}>
            <VectorAnimation
              ignoreReduceMotion
              src={i < shown() ? url : undefined}
              style={{ width: 240, height: 180, borderWidth: 1, borderColor: "#e5e7eb", borderRadius: 8 }}
              accessibilityLabel={name}
              onLoad={() => next(i)}
              onError={(e) => {
                error.set(e);
                next(i);
              }}
            />
            <Text style={{ fontSize: 12, opacity: 0.7 }}>{error() ? `${name}: ${error()}` : name}</Text>
          </View>
        );
      })}
      {riveFiles.length === 0 ? <Text style={{ opacity: 0.7 }}>No Rive files.</Text> : null}
    </View>
  );
}

const GALLERY = {
  "lottie-files": "lottie",
  "dotlottie-files": "dotlottie",
  "svg-files": "svg",
} as const;

/** One group of the gallery's files, a tile each; the frame is as tall as the grid. */
function Files(props: { group: "lottie" | "dotlottie" | "svg" }) {
  const list = gallery[props.group];
  return (
    <View
      style={{ padding: 12, flexDirection: "row", flexWrap: "wrap", gap: 14, justifyContent: "center" }}
      onLayout={(e) => {
        host.call("height", Math.ceil(e.size.height)).catch(() => {});
      }}
    >
      {list.map(([name, src]) => (
        <View style={{ width: 200, flexShrink: 0, gap: 4, alignItems: "center" }}>
          <VectorAnimation
            ignoreReduceMotion
            src={src as never}
            style={{ width: 200, height: 200, borderWidth: 1, borderColor: "#e5e7eb", borderRadius: 8 }}
            accessibilityLabel={name}
          />
          <Text style={{ fontSize: 12, opacity: 0.7 }}>{name}</Text>
        </View>
      ))}
      {list.length === 0 ? (
        <Text style={{ opacity: 0.7 }}>No files in this group.</Text>
      ) : null}
    </View>
  );
}

/**
 * Every scene, a tile each. The grid's height goes to the page that frames
 * it (the host's `height`), which sizes the frame to it, so the page scrolls
 * and no bar is wanted in the frame; the `Scroll` is for a frame left as it was.
 */
function All() {
  return (
    <Scroll>
      <View
        style={{ padding: 12, flexDirection: "row", flexWrap: "wrap", gap: 10 }}
        onLayout={(e) => {
          host.call("height", Math.ceil(e.size.height)).catch(() => {});
        }}
      >
        {(scenes as [string, unknown][]).map(([name, src]) => (
          <View style={{ gap: 4, alignItems: "center" }}>
            <VectorAnimation
              ignoreReduceMotion
              src={src as never}
              style={{ width: 120, height: 120, borderWidth: 1, borderColor: "#e5e7eb", borderRadius: 8 }}
              accessibilityLabel={name}
            />
            <Text style={{ fontSize: 11, opacity: 0.7 }}>{name}</Text>
          </View>
        ))}
      </View>
    </Scroll>
  );
}

/** One gallery file alone, no background and no controls: the home page's hero. */
function Bare(props: { src: unknown; name: string }) {
  return (
    <View style={{ flex: 1 }}>
      <VectorAnimation
        ignoreReduceMotion
        src={props.src as never}
        style={{ width: "100%", height: "100%" }}
        accessibilityLabel={props.name}
        onLoad={ready}
      />
    </View>
  );
}

function bareFile(demo: string): [string, unknown] | undefined {
  const m = /^file-(lottie|dotlottie|svg)-(.+)$/.exec(demo);
  const found = m && gallery[m[1] as keyof typeof gallery].find(([name]) => name === m[2]);
  return found ? [found[0], found[1]] : undefined;
}

function App() {
  const demo = signal<string | undefined>(undefined);
  host
    .call("demo")
    .then((d) => demo.set(typeof d === "string" ? d : ""))
    .catch(() => demo.set(""));
  return (
    <View style={{ flex: 1, backgroundColor: demo()?.startsWith("file-") ? "transparent" : "#ffffff" }}>
      {(() => {
        const d = demo();
        if (d === undefined) return null;
        const file = bareFile(d);
        if (file) return <Bare src={file[1]} name={file[0]} />;
        if (d === "playback") return <Playback />;
        if (d === "events") return <Events />;
        if (d === "rive-switch") return <RiveSwitch />;
        if (d === "rive-events") return <RiveEvents />;
        if (d === "rive-progress") return <RiveProgress />;
        if (d === "rive-files") return <RiveFiles />;
        if (d in GALLERY) return <Files group={GALLERY[d as keyof typeof GALLERY]} />;
        const src = named.get(d);
        if (src !== undefined) return <Single src={src} name={d} />;
        if (d === "" || d === "all") return <All />;
        return <Text style={{ margin: 12 }}>{`No example named "${d}".`}</Text>;
      })()}
    </View>
  );
}

mount(() => <App />);
