import GenericScreen from "../components/GenericScreen";
import { screenConfig } from "../lib/screenConfigs";

export default function ImportsRoute() {
  return <GenericScreen config={screenConfig("imports")} />;
}
