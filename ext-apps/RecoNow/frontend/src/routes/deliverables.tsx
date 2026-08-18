import GenericScreen from "../components/GenericScreen";
import { screenConfig } from "../lib/screenConfigs";

export default function DeliverablesRoute() {
  return <GenericScreen config={screenConfig("deliverables")} />;
}
