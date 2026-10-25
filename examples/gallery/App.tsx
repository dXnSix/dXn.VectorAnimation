// The player in a TSX app: a Lottie file imported as JSON, a Figma Motion
// SVG compiled ahead (`vector-animation compile`), and the controls apps
// use: pause, scrub, play a part, reverse.
import { mount, Scroll, signal, Text, View } from "nsh";
import { Button } from "nsh/base";
import { type AnimationInfo, VectorAnimation } from "dxn-vector-animation";
import lottie from "./animations/lottie-motion.json";
import figma from "./animations/figma-motion.vector-animation.json";

function Control(props: { title: string; onPress: () => void }) {
  return (
    <Button
      onPress={props.onPress}
      style={{ backgroundColor: "#e5e7eb", borderRadius: 8, paddingHorizontal: 12, paddingVertical: 6 }}
      activeStyle={{ opacity: 0.7 }}
    >
      <Text style={{ fontSize: 14, fontWeight: "600" }}>{props.title}</Text>
    </Button>
  );
}

function App() {
  const paused = signal(false);
  const reversed = signal(false);
  const progress = signal<number | undefined>(undefined);
  const figmaInfo = signal("Loading");
  const lottieInfo = signal("Loading");
  const ends = signal(0);

  const describe = (name: string, shown: (text: string) => void) => (info: AnimationInfo) =>
    shown(`${name}: ${info.duration.toFixed(1)} s, ${info.size.width} × ${info.size.height}`);

  return (
    <Scroll contentStyle={{ padding: 16, gap: 16 }}>
      <Text style={{ fontSize: 20, fontWeight: "700" }}>Vector animations</Text>

      <Text style={{ fontWeight: "600" }}>Figma Motion (SVG, compiled)</Text>
      <VectorAnimation
        src={figma}
        style={{ width: 320, height: 160, backgroundColor: "#f3f4f6", borderRadius: 12 }}
        paused={paused()}
        direction={reversed() ? "reverse" : "normal"}
        progress={progress()}
        onLoad={describe("Figma", (t) => figmaInfo.set(t))}
        accessibilityLabel="A button popping in"
      />
      <View style={{ flexDirection: "row", flexWrap: "wrap", gap: 8 }}>
        <Control title={paused() ? "Play" : "Pause"} onPress={() => paused.set(!paused())} />
        <Control title={reversed() ? "Forwards" : "Backwards"} onPress={() => reversed.set(!reversed())} />
        <Control title="Scrub to half" onPress={() => progress.set(0.5)} />
        <Control title="Release" onPress={() => progress.set(undefined)} />
      </View>

      <Text style={{ fontWeight: "600" }}>Lottie (JSON import), played twice</Text>
      <VectorAnimation
        src={lottie}
        style={{ width: 160, height: 160 }}
        repeat={2}
        onEnd={() => ends.set(ends() + 1)}
        onLoad={describe("Lottie", (t) => lottieInfo.set(t))}
      />
      <Text>{`Ended ${ends()} times`}</Text>
      <Text style={{ opacity: 0.6 }}>{figmaInfo()}</Text>
      <Text style={{ opacity: 0.6 }}>{lottieInfo()}</Text>
    </Scroll>
  );
}

mount(() => <App />);
