// The check set (`vector-animation-check scenes`) side by side, playing or
// held at one moment, a page of twelve at a time: what `check.test.tsx`
// photographs on each platform, to set beside the reference renderer.
import { mount, Scroll, signal, Text, View } from "nsh";
import { Button } from "nsh/base";
import { VectorAnimation } from "dxn-vector-animation";
import { scenes } from "./animations/index";

// Small enough for twelve on a phone's screen.
const TILE = 96;
const PAGE = 12;

function Control(props: { id: string; title: string; onPress: () => void }) {
  return (
    <Button
      testID={props.id}
      onPress={props.onPress}
      style={{ backgroundColor: "#e5e7eb", borderRadius: 8, paddingHorizontal: 10, paddingVertical: 6 }}
    >
      <Text style={{ fontSize: 13, fontWeight: "600" }}>{props.title}</Text>
    </Button>
  );
}

function App() {
  const progress = signal<number | undefined>(undefined);
  const page = signal(0);
  const loaded = signal(0);
  const errors = signal("");
  const pages = Math.ceil(scenes.length / PAGE);
  const shown = () => scenes.slice(page() * PAGE, page() * PAGE + PAGE);
  return (
    <Scroll style={{ backgroundColor: "#ffffff" }} contentStyle={{ padding: 12, gap: 10, paddingTop: 60 }}>
      <View style={{ flexDirection: "row", gap: 6, flexWrap: "wrap" }}>
        <Control id="play" title="play" onPress={() => progress.set(undefined)} />
        <Control id="at-25" title="25" onPress={() => progress.set(0.25)} />
        <Control id="at-50" title="50" onPress={() => progress.set(0.5)} />
        <Control id="at-75" title="75" onPress={() => progress.set(0.75)} />
        <Control
          id="next"
          title="next"
          onPress={() => {
            loaded.set(0);
            page.set((page() + 1) % pages);
          }}
        />
      </View>
      <Text style={{ fontSize: 12 }}>{`page ${page() + 1} of ${pages}, ${loaded()} loaded ${errors()}`}</Text>
      <View style={{ flexDirection: "row", flexWrap: "wrap", gap: 8 }}>
        {shown().map(([name, src]) => (
          <View style={{ gap: 2 }}>
            <VectorAnimation
              src={src}
              progress={progress()}
              style={{ width: TILE, height: TILE, backgroundColor: "#ffffff", borderWidth: 1, borderColor: "#d1d5db" }}
              onLoad={() => loaded.set(loaded() + 1)}
              onError={(e) => errors.set(`${errors()} ${name}: ${e}`)}
            />
            <Text style={{ fontSize: 11 }}>{name}</Text>
          </View>
        ))}
      </View>
    </Scroll>
  );
}

mount(App);
