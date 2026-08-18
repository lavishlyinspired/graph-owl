import GenericScreen from "../components/GenericScreen";
import { screenConfig } from "../lib/screenConfigs";

export default function RulesRoute() {
  return <GenericScreen config={screenConfig("rules")} />;
}
