import GenericScreen from "../components/GenericScreen";
import { screenConfig } from "../lib/screenConfigs";

export default function RiskRoute() {
  return <GenericScreen config={screenConfig("risk")} />;
}
