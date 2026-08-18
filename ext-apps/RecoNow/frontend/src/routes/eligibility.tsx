import GenericScreen from "../components/GenericScreen";
import { screenConfig } from "../lib/screenConfigs";

export default function EligibilityRoute() {
  return <GenericScreen config={screenConfig("eligibility")} />;
}
