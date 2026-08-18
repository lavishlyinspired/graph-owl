import GenericScreen from "../components/GenericScreen";
import { screenConfig } from "../lib/screenConfigs";

export default function ObligationsRoute() {
  return <GenericScreen config={screenConfig("obligations")} />;
}
