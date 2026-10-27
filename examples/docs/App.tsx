// The examples the documentation site embeds. The page's address names one,
// `?demo=<name>` (the web host, host.js, reads it): it fills the frame, with
// its controls. Without one, every example shows, a grid of tiles.
//
// The scenes are the check set (`vector-animation-check scenes examples/docs
// --gallery fixtures/gallery` writes animations/), and two of this app's own:
// `playback` and `events`. The gallery's files, compiled ahead, show as grids:
// `lottie-files`, `dotlottie-files` and `svg-files`; `file-<group>-<name>`
// plays one of them alone on the page's background (the home page's hero).
import { host, mount, Scroll, signal, Text, View } from "nsh";
import { Button } from "nsh/base";
import { VectorAnimation, type VectorAnimationRef } from "dxn-vector-animation";
import { scenes } from "./animations/index";
import { gallery } from "./animations/gallery";

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
