import GenericScreen from "../components/GenericScreen";
import { screenConfig } from "../lib/screenConfigs";

export default function AtRiskRoute() {
  return <GenericScreen config={screenConfig("atrisk")} />;
}
