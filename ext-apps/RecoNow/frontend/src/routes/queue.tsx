import GenericScreen from "../components/GenericScreen";
import { screenConfig } from "../lib/screenConfigs";

export default function QueueRoute() {
  return <GenericScreen config={screenConfig("queue")} />;
}
